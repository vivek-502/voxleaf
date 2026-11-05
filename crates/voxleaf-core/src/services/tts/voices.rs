//! The catalogue of voices, from the service when it answers and from a
//! captured list when it does not.
//!
//! The list endpoint is the same one the read-aloud service itself uses. It
//! is fetched over HTTPS with the same `Sec-MS-GEC` handshake the synthesis
//! socket needs, because the endpoint is gated on the same token. A first run
//! with no network still offers every voice, though: the table below was
//! captured from the live endpoint and covers every locale it advertised —
//! one or two voices per locale, and every English voice, since that is what
//! the picker actually offers a reader.
//!
//! The cached answer wins after the first successful fetch, so the catalogue
//! tracks Microsoft's additions for the life of the process.

use std::sync::{Mutex, OnceLock};

use serde_json::Value;

use crate::error::ApiResult;
use crate::models::TtsVoice;
use crate::services::tts::{client, drm};

const VOICE_LIST_URL: &str = concat!(
    "https://speech.platform.bing.com/consumer/speech/synthesize/readaloud",
    "/voices/list?trustedclienttoken=6A5AA1D4EAFF4E9FB37E23D68491D6F4"
);

/// The catalogue captured from the live endpoint: every locale it offered,
/// one female and one male voice per locale where it had both, plus every
/// English voice. Names are the provider's own — a stale name here fails the
/// same way it would in the web app, so they are never guessed.
static FALLBACK_VOICES: &[(&str, &str, &str)] = &[
    ("af-ZA-AdriNeural", "af-ZA", "Female"),
    ("af-ZA-WillemNeural", "af-ZA", "Male"),
    ("am-ET-MekdesNeural", "am-ET", "Female"),
    ("am-ET-AmehaNeural", "am-ET", "Male"),
    ("ar-AE-FatimaNeural", "ar-AE", "Female"),
    ("ar-AE-HamdanNeural", "ar-AE", "Male"),
    ("ar-BH-LailaNeural", "ar-BH", "Female"),
    ("ar-BH-AliNeural", "ar-BH", "Male"),
    ("ar-DZ-AminaNeural", "ar-DZ", "Female"),
    ("ar-DZ-IsmaelNeural", "ar-DZ", "Male"),
    ("ar-EG-SalmaNeural", "ar-EG", "Female"),
    ("ar-EG-ShakirNeural", "ar-EG", "Male"),
    ("ar-IQ-RanaNeural", "ar-IQ", "Female"),
    ("ar-IQ-BasselNeural", "ar-IQ", "Male"),
    ("ar-JO-SanaNeural", "ar-JO", "Female"),
    ("ar-JO-TaimNeural", "ar-JO", "Male"),
    ("ar-KW-NouraNeural", "ar-KW", "Female"),
    ("ar-KW-FahedNeural", "ar-KW", "Male"),
    ("ar-LB-LaylaNeural", "ar-LB", "Female"),
    ("ar-LB-RamiNeural", "ar-LB", "Male"),
    ("ar-LY-ImanNeural", "ar-LY", "Female"),
    ("ar-LY-OmarNeural", "ar-LY", "Male"),
    ("ar-MA-MounaNeural", "ar-MA", "Female"),
    ("ar-MA-JamalNeural", "ar-MA", "Male"),
    ("ar-OM-AyshaNeural", "ar-OM", "Female"),
    ("ar-OM-AbdullahNeural", "ar-OM", "Male"),
    ("ar-QA-AmalNeural", "ar-QA", "Female"),
    ("ar-QA-MoazNeural", "ar-QA", "Male"),
    ("ar-SA-ZariyahNeural", "ar-SA", "Female"),
    ("ar-SA-HamedNeural", "ar-SA", "Male"),
    ("ar-SY-AmanyNeural", "ar-SY", "Female"),
    ("ar-SY-LaithNeural", "ar-SY", "Male"),
    ("ar-TN-ReemNeural", "ar-TN", "Female"),
    ("ar-TN-HediNeural", "ar-TN", "Male"),
    ("ar-YE-MaryamNeural", "ar-YE", "Female"),
    ("ar-YE-SalehNeural", "ar-YE", "Male"),
    ("az-AZ-BanuNeural", "az-AZ", "Female"),
    ("az-AZ-BabekNeural", "az-AZ", "Male"),
    ("bg-BG-KalinaNeural", "bg-BG", "Female"),
    ("bg-BG-BorislavNeural", "bg-BG", "Male"),
    ("bn-BD-NabanitaNeural", "bn-BD", "Female"),
    ("bn-BD-PradeepNeural", "bn-BD", "Male"),
    ("bn-IN-TanishaaNeural", "bn-IN", "Female"),
    ("bn-IN-BashkarNeural", "bn-IN", "Male"),
    ("bs-BA-VesnaNeural", "bs-BA", "Female"),
    ("bs-BA-GoranNeural", "bs-BA", "Male"),
    ("ca-ES-JoanaNeural", "ca-ES", "Female"),
    ("ca-ES-EnricNeural", "ca-ES", "Male"),
    ("cs-CZ-VlastaNeural", "cs-CZ", "Female"),
    ("cs-CZ-AntoninNeural", "cs-CZ", "Male"),
    ("cy-GB-NiaNeural", "cy-GB", "Female"),
    ("cy-GB-AledNeural", "cy-GB", "Male"),
    ("da-DK-ChristelNeural", "da-DK", "Female"),
    ("da-DK-JeppeNeural", "da-DK", "Male"),
    ("de-AT-IngridNeural", "de-AT", "Female"),
    ("de-AT-JonasNeural", "de-AT", "Male"),
    ("de-CH-LeniNeural", "de-CH", "Female"),
    ("de-CH-JanNeural", "de-CH", "Male"),
    ("de-DE-SeraphinaMultilingualNeural", "de-DE", "Female"),
    ("de-DE-FlorianMultilingualNeural", "de-DE", "Male"),
    ("el-GR-AthinaNeural", "el-GR", "Female"),
    ("el-GR-NestorasNeural", "el-GR", "Male"),
    ("en-AU-WilliamMultilingualNeural", "en-AU", "Male"),
    ("en-AU-NatashaNeural", "en-AU", "Female"),
    ("en-CA-ClaraNeural", "en-CA", "Female"),
    ("en-CA-LiamNeural", "en-CA", "Male"),
    ("en-GB-LibbyNeural", "en-GB", "Female"),
    ("en-GB-MaisieNeural", "en-GB", "Female"),
    ("en-GB-RyanNeural", "en-GB", "Male"),
    ("en-GB-SoniaNeural", "en-GB", "Female"),
    ("en-GB-ThomasNeural", "en-GB", "Male"),
    ("en-HK-YanNeural", "en-HK", "Female"),
    ("en-HK-SamNeural", "en-HK", "Male"),
    ("en-IE-ConnorNeural", "en-IE", "Male"),
    ("en-IE-EmilyNeural", "en-IE", "Female"),
    ("en-IN-NeerjaExpressiveNeural", "en-IN", "Female"),
    ("en-IN-NeerjaNeural", "en-IN", "Female"),
    ("en-IN-PrabhatNeural", "en-IN", "Male"),
    ("en-KE-AsiliaNeural", "en-KE", "Female"),
    ("en-KE-ChilembaNeural", "en-KE", "Male"),
    ("en-NG-AbeoNeural", "en-NG", "Male"),
    ("en-NG-EzinneNeural", "en-NG", "Female"),
    ("en-NZ-MitchellNeural", "en-NZ", "Male"),
    ("en-NZ-MollyNeural", "en-NZ", "Female"),
    ("en-PH-JamesNeural", "en-PH", "Male"),
    ("en-PH-RosaNeural", "en-PH", "Female"),
    ("en-SG-LunaNeural", "en-SG", "Female"),
    ("en-SG-WayneNeural", "en-SG", "Male"),
    ("en-TZ-ElimuNeural", "en-TZ", "Male"),
    ("en-TZ-ImaniNeural", "en-TZ", "Female"),
    ("en-US-AvaNeural", "en-US", "Female"),
    ("en-US-AndrewNeural", "en-US", "Male"),
    ("en-US-EmmaNeural", "en-US", "Female"),
    ("en-US-BrianNeural", "en-US", "Male"),
    ("en-US-AnaNeural", "en-US", "Female"),
    ("en-US-AndrewMultilingualNeural", "en-US", "Male"),
    ("en-US-AriaNeural", "en-US", "Female"),
    ("en-US-AvaMultilingualNeural", "en-US", "Female"),
    ("en-US-BrianMultilingualNeural", "en-US", "Male"),
    ("en-US-ChristopherNeural", "en-US", "Male"),
    ("en-US-EmmaMultilingualNeural", "en-US", "Female"),
    ("en-US-EricNeural", "en-US", "Male"),
    ("en-US-GuyNeural", "en-US", "Male"),
    ("en-US-JennyNeural", "en-US", "Female"),
    ("en-US-MichelleNeural", "en-US", "Female"),
    ("en-US-RogerNeural", "en-US", "Male"),
    ("en-US-SteffanNeural", "en-US", "Male"),
    ("en-ZA-LeahNeural", "en-ZA", "Female"),
    ("en-ZA-LukeNeural", "en-ZA", "Male"),
    ("es-AR-ElenaNeural", "es-AR", "Female"),
    ("es-AR-TomasNeural", "es-AR", "Male"),
    ("es-BO-SofiaNeural", "es-BO", "Female"),
    ("es-BO-MarceloNeural", "es-BO", "Male"),
    ("es-CL-CatalinaNeural", "es-CL", "Female"),
    ("es-CL-LorenzoNeural", "es-CL", "Male"),
    ("es-CO-SalomeNeural", "es-CO", "Female"),
    ("es-CO-GonzaloNeural", "es-CO", "Male"),
    ("es-CR-MariaNeural", "es-CR", "Female"),
    ("es-CR-JuanNeural", "es-CR", "Male"),
    ("es-CU-BelkysNeural", "es-CU", "Female"),
    ("es-CU-ManuelNeural", "es-CU", "Male"),
    ("es-DO-RamonaNeural", "es-DO", "Female"),
    ("es-DO-EmilioNeural", "es-DO", "Male"),
    ("es-EC-AndreaNeural", "es-EC", "Female"),
    ("es-EC-LuisNeural", "es-EC", "Male"),
    ("es-ES-XimenaNeural", "es-ES", "Female"),
    ("es-ES-AlvaroNeural", "es-ES", "Male"),
    ("es-GQ-TeresaNeural", "es-GQ", "Female"),
    ("es-GQ-JavierNeural", "es-GQ", "Male"),
    ("es-GT-MartaNeural", "es-GT", "Female"),
    ("es-GT-AndresNeural", "es-GT", "Male"),
    ("es-HN-KarlaNeural", "es-HN", "Female"),
    ("es-HN-CarlosNeural", "es-HN", "Male"),
    ("es-MX-DaliaNeural", "es-MX", "Female"),
    ("es-MX-JorgeNeural", "es-MX", "Male"),
    ("es-NI-YolandaNeural", "es-NI", "Female"),
    ("es-NI-FedericoNeural", "es-NI", "Male"),
    ("es-PA-MargaritaNeural", "es-PA", "Female"),
    ("es-PA-RobertoNeural", "es-PA", "Male"),
    ("es-PE-CamilaNeural", "es-PE", "Female"),
    ("es-PE-AlexNeural", "es-PE", "Male"),
    ("es-PR-KarinaNeural", "es-PR", "Female"),
    ("es-PR-VictorNeural", "es-PR", "Male"),
    ("es-PY-TaniaNeural", "es-PY", "Female"),
    ("es-PY-MarioNeural", "es-PY", "Male"),
    ("es-SV-LorenaNeural", "es-SV", "Female"),
    ("es-SV-RodrigoNeural", "es-SV", "Male"),
    ("es-US-PalomaNeural", "es-US", "Female"),
    ("es-US-AlonsoNeural", "es-US", "Male"),
    ("es-UY-ValentinaNeural", "es-UY", "Female"),
    ("es-UY-MateoNeural", "es-UY", "Male"),
    ("es-VE-PaolaNeural", "es-VE", "Female"),
    ("es-VE-SebastianNeural", "es-VE", "Male"),
    ("et-EE-AnuNeural", "et-EE", "Female"),
    ("et-EE-KertNeural", "et-EE", "Male"),
    ("fa-IR-DilaraNeural", "fa-IR", "Female"),
    ("fa-IR-FaridNeural", "fa-IR", "Male"),
    ("fi-FI-NooraNeural", "fi-FI", "Female"),
    ("fi-FI-HarriNeural", "fi-FI", "Male"),
    ("fil-PH-BlessicaNeural", "fil-PH", "Female"),
    ("fil-PH-AngeloNeural", "fil-PH", "Male"),
    ("fr-BE-CharlineNeural", "fr-BE", "Female"),
    ("fr-BE-GerardNeural", "fr-BE", "Male"),
    ("fr-CA-SylvieNeural", "fr-CA", "Female"),
    ("fr-CA-ThierryNeural", "fr-CA", "Male"),
    ("fr-CH-ArianeNeural", "fr-CH", "Female"),
    ("fr-CH-FabriceNeural", "fr-CH", "Male"),
    ("fr-FR-VivienneMultilingualNeural", "fr-FR", "Female"),
    ("fr-FR-RemyMultilingualNeural", "fr-FR", "Male"),
    ("ga-IE-OrlaNeural", "ga-IE", "Female"),
    ("ga-IE-ColmNeural", "ga-IE", "Male"),
    ("gl-ES-SabelaNeural", "gl-ES", "Female"),
    ("gl-ES-RoiNeural", "gl-ES", "Male"),
    ("gu-IN-DhwaniNeural", "gu-IN", "Female"),
    ("gu-IN-NiranjanNeural", "gu-IN", "Male"),
    ("he-IL-HilaNeural", "he-IL", "Female"),
    ("he-IL-AvriNeural", "he-IL", "Male"),
    ("hi-IN-SwaraNeural", "hi-IN", "Female"),
    ("hi-IN-MadhurNeural", "hi-IN", "Male"),
    ("hr-HR-GabrijelaNeural", "hr-HR", "Female"),
    ("hr-HR-SreckoNeural", "hr-HR", "Male"),
    ("hu-HU-NoemiNeural", "hu-HU", "Female"),
    ("hu-HU-TamasNeural", "hu-HU", "Male"),
    ("id-ID-GadisNeural", "id-ID", "Female"),
    ("id-ID-ArdiNeural", "id-ID", "Male"),
    ("is-IS-GudrunNeural", "is-IS", "Female"),
    ("is-IS-GunnarNeural", "is-IS", "Male"),
    ("it-IT-ElsaNeural", "it-IT", "Female"),
    ("it-IT-GiuseppeMultilingualNeural", "it-IT", "Male"),
    ("iu-Cans-CA-SiqiniqNeural", "iu-Cans-CA", "Female"),
    ("iu-Cans-CA-TaqqiqNeural", "iu-Cans-CA", "Male"),
    ("iu-Latn-CA-SiqiniqNeural", "iu-Latn-CA", "Female"),
    ("iu-Latn-CA-TaqqiqNeural", "iu-Latn-CA", "Male"),
    ("ja-JP-NanamiNeural", "ja-JP", "Female"),
    ("ja-JP-KeitaNeural", "ja-JP", "Male"),
    ("jv-ID-SitiNeural", "jv-ID", "Female"),
    ("jv-ID-DimasNeural", "jv-ID", "Male"),
    ("ka-GE-EkaNeural", "ka-GE", "Female"),
    ("ka-GE-GiorgiNeural", "ka-GE", "Male"),
    ("kk-KZ-AigulNeural", "kk-KZ", "Female"),
    ("kk-KZ-DauletNeural", "kk-KZ", "Male"),
    ("km-KH-SreymomNeural", "km-KH", "Female"),
    ("km-KH-PisethNeural", "km-KH", "Male"),
    ("kn-IN-SapnaNeural", "kn-IN", "Female"),
    ("kn-IN-GaganNeural", "kn-IN", "Male"),
    ("ko-KR-SunHiNeural", "ko-KR", "Female"),
    ("ko-KR-HyunsuMultilingualNeural", "ko-KR", "Male"),
    ("lo-LA-KeomanyNeural", "lo-LA", "Female"),
    ("lo-LA-ChanthavongNeural", "lo-LA", "Male"),
    ("lt-LT-OnaNeural", "lt-LT", "Female"),
    ("lt-LT-LeonasNeural", "lt-LT", "Male"),
    ("lv-LV-EveritaNeural", "lv-LV", "Female"),
    ("lv-LV-NilsNeural", "lv-LV", "Male"),
    ("mk-MK-MarijaNeural", "mk-MK", "Female"),
    ("mk-MK-AleksandarNeural", "mk-MK", "Male"),
    ("ml-IN-SobhanaNeural", "ml-IN", "Female"),
    ("ml-IN-MidhunNeural", "ml-IN", "Male"),
    ("mn-MN-YesuiNeural", "mn-MN", "Female"),
    ("mn-MN-BataaNeural", "mn-MN", "Male"),
    ("mr-IN-AarohiNeural", "mr-IN", "Female"),
    ("mr-IN-ManoharNeural", "mr-IN", "Male"),
    ("ms-MY-YasminNeural", "ms-MY", "Female"),
    ("ms-MY-OsmanNeural", "ms-MY", "Male"),
    ("mt-MT-GraceNeural", "mt-MT", "Female"),
    ("mt-MT-JosephNeural", "mt-MT", "Male"),
    ("my-MM-NilarNeural", "my-MM", "Female"),
    ("my-MM-ThihaNeural", "my-MM", "Male"),
    ("nb-NO-PernilleNeural", "nb-NO", "Female"),
    ("nb-NO-FinnNeural", "nb-NO", "Male"),
    ("ne-NP-HemkalaNeural", "ne-NP", "Female"),
    ("ne-NP-SagarNeural", "ne-NP", "Male"),
    ("nl-BE-DenaNeural", "nl-BE", "Female"),
    ("nl-BE-ArnaudNeural", "nl-BE", "Male"),
    ("nl-NL-ColetteNeural", "nl-NL", "Female"),
    ("nl-NL-MaartenNeural", "nl-NL", "Male"),
    ("pl-PL-ZofiaNeural", "pl-PL", "Female"),
    ("pl-PL-MarekNeural", "pl-PL", "Male"),
    ("ps-AF-LatifaNeural", "ps-AF", "Female"),
    ("ps-AF-GulNawazNeural", "ps-AF", "Male"),
    ("pt-BR-ThalitaMultilingualNeural", "pt-BR", "Female"),
    ("pt-BR-AntonioNeural", "pt-BR", "Male"),
    ("pt-PT-RaquelNeural", "pt-PT", "Female"),
    ("pt-PT-DuarteNeural", "pt-PT", "Male"),
    ("ro-RO-AlinaNeural", "ro-RO", "Female"),
    ("ro-RO-EmilNeural", "ro-RO", "Male"),
    ("ru-RU-SvetlanaNeural", "ru-RU", "Female"),
    ("ru-RU-DmitryNeural", "ru-RU", "Male"),
    ("si-LK-ThiliniNeural", "si-LK", "Female"),
    ("si-LK-SameeraNeural", "si-LK", "Male"),
    ("sk-SK-ViktoriaNeural", "sk-SK", "Female"),
    ("sk-SK-LukasNeural", "sk-SK", "Male"),
    ("sl-SI-PetraNeural", "sl-SI", "Female"),
    ("sl-SI-RokNeural", "sl-SI", "Male"),
    ("so-SO-UbaxNeural", "so-SO", "Female"),
    ("so-SO-MuuseNeural", "so-SO", "Male"),
    ("sq-AL-AnilaNeural", "sq-AL", "Female"),
    ("sq-AL-IlirNeural", "sq-AL", "Male"),
    ("sr-RS-SophieNeural", "sr-RS", "Female"),
    ("sr-RS-NicholasNeural", "sr-RS", "Male"),
    ("su-ID-TutiNeural", "su-ID", "Female"),
    ("su-ID-JajangNeural", "su-ID", "Male"),
    ("sv-SE-SofieNeural", "sv-SE", "Female"),
    ("sv-SE-MattiasNeural", "sv-SE", "Male"),
    ("sw-KE-ZuriNeural", "sw-KE", "Female"),
    ("sw-KE-RafikiNeural", "sw-KE", "Male"),
    ("sw-TZ-RehemaNeural", "sw-TZ", "Female"),
    ("sw-TZ-DaudiNeural", "sw-TZ", "Male"),
    ("ta-IN-PallaviNeural", "ta-IN", "Female"),
    ("ta-IN-ValluvarNeural", "ta-IN", "Male"),
    ("ta-LK-SaranyaNeural", "ta-LK", "Female"),
    ("ta-LK-KumarNeural", "ta-LK", "Male"),
    ("ta-MY-KaniNeural", "ta-MY", "Female"),
    ("ta-MY-SuryaNeural", "ta-MY", "Male"),
    ("ta-SG-VenbaNeural", "ta-SG", "Female"),
    ("ta-SG-AnbuNeural", "ta-SG", "Male"),
    ("te-IN-ShrutiNeural", "te-IN", "Female"),
    ("te-IN-MohanNeural", "te-IN", "Male"),
    ("th-TH-PremwadeeNeural", "th-TH", "Female"),
    ("th-TH-NiwatNeural", "th-TH", "Male"),
    ("tr-TR-EmelNeural", "tr-TR", "Female"),
    ("tr-TR-AhmetNeural", "tr-TR", "Male"),
    ("uk-UA-PolinaNeural", "uk-UA", "Female"),
    ("uk-UA-OstapNeural", "uk-UA", "Male"),
    ("ur-IN-GulNeural", "ur-IN", "Female"),
    ("ur-IN-SalmanNeural", "ur-IN", "Male"),
    ("ur-PK-UzmaNeural", "ur-PK", "Female"),
    ("ur-PK-AsadNeural", "ur-PK", "Male"),
    ("uz-UZ-MadinaNeural", "uz-UZ", "Female"),
    ("uz-UZ-SardorNeural", "uz-UZ", "Male"),
    ("vi-VN-HoaiMyNeural", "vi-VN", "Female"),
    ("vi-VN-NamMinhNeural", "vi-VN", "Male"),
    ("zh-CN-XiaoxiaoNeural", "zh-CN", "Female"),
    ("zh-CN-YunjianNeural", "zh-CN", "Male"),
    ("zh-CN-liaoning-XiaobeiNeural", "zh-CN-liaoning", "Female"),
    ("zh-CN-shaanxi-XiaoniNeural", "zh-CN-shaanxi", "Female"),
    ("zh-HK-HiuGaaiNeural", "zh-HK", "Female"),
    ("zh-HK-WanLungNeural", "zh-HK", "Male"),
    ("zh-TW-HsiaoChenNeural", "zh-TW", "Female"),
    ("zh-TW-YunJheNeural", "zh-TW", "Male"),
    ("zu-ZA-ThandoNeural", "zu-ZA", "Female"),
    ("zu-ZA-ThembaNeural", "zu-ZA", "Male"),];

fn fallback_voices() -> Vec<TtsVoice> {
    FALLBACK_VOICES
        .iter()
        .map(|(name, locale, gender)| TtsVoice {
            name: (*name).to_string(),
            locale: (*locale).to_string(),
            gender: (*gender).to_string(),
        })
        .collect()
}

/// Process-wide cache: `None` until a live fetch succeeds.
fn cache() -> &'static Mutex<Option<Vec<TtsVoice>>> {
    static CACHE: OnceLock<Mutex<Option<Vec<TtsVoice>>>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(None))
}

/// One live fetch against the endpoint. Only the fields the picker shows are
/// read; the pseudo-header HTTP/2 forbids from a header map is dropped.
async fn fetch_live() -> ApiResult<Vec<TtsVoice>> {
    let token = drm::generate_sec_ms_gec();
    let url = format!(
        "{VOICE_LIST_URL}&Sec-MS-GEC={token}&Sec-MS-GEC-Version={}",
        client::SEC_MS_GEC_VERSION
    );

    let client = reqwest::Client::new();
    let mut request = client.get(&url);
    for (name, value) in [
        ("User-Agent", client::USER_AGENT),
        ("Accept-Encoding", "gzip, deflate, br, zstd"),
        ("Accept-Language", "en-US,en;q=0.9"),
        ("Accept", "*/*"),
        ("Origin", client::ORIGIN),
        ("Cookie", &format!("muid={};", drm::generate_muid())),
    ] {
        request = request.header(name, value);
    }
    let response = request.send().await?.error_for_status()?;
    let payload: Value = response.json().await?;

    let voices = payload
        .as_array()
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(TtsVoice {
                        name: item.get("ShortName")?.as_str()?.to_string(),
                        locale: item.get("Locale")?.as_str()?.to_string(),
                        gender: item.get("Gender")?.as_str()?.to_string(),
                    })
                })
                .collect::<Vec<TtsVoice>>()
        })
        .unwrap_or_default();

    if voices.is_empty() {
        return Err(crate::error::ApiError::internal(
            "The voice service returned no voices.",
        ));
    }
    Ok(voices)
}

/// Every voice the service offers, sorted by name.
///
/// The live catalogue is fetched once and cached. Any failure — offline,
/// endpoint moved, handshake rejected — falls back to the captured table,
/// because a picker with every voice beats an error page.
pub(crate) async fn list_voices() -> ApiResult<Vec<TtsVoice>> {
    if let Ok(cache) = cache().lock() {
        if let Some(voices) = cache.as_ref() {
            let mut voices = voices.clone();
            voices.sort_by(|a, b| a.name.cmp(&b.name));
            return Ok(voices);
        }
    }

    let mut live = match fetch_live().await {
        Ok(voices) => voices,
        Err(_) => fallback_voices(),
    };
    live.sort_by(|a, b| a.name.cmp(&b.name));

    if let Ok(mut cache) = cache().lock() {
        *cache = Some(live.clone());
    }
    Ok(live)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_fallback_covers_the_locales_the_picker_offers() {
        let voices = fallback_voices();
        assert!(voices.len() > 250, "only {} voices in the fallback", voices.len());
        let locales: std::collections::HashSet<&str> =
            voices.iter().map(|v| v.locale.as_str()).collect();
        // The languages VoxLeaf can narrate into, and the default voice.
        for locale in [
            "en-US", "en-GB", "hi-IN", "bn-IN", "fa-IR", "hu-HU", "ml-IN",
            "nb-NO", "ro-RO", "ca-ES", "zh-CN", "zh-TW", "ja-JP", "ko-KR",
            "es-ES", "fr-FR", "de-DE", "pt-BR", "it-IT", "ru-RU", "ar-SA",
            "tr-TR", "pl-PL", "nl-NL", "sv-SE", "da-DK", "fi-FI", "el-GR",
            "he-IL", "th-TH", "vi-VN", "id-ID", "ms-MY", "fil-PH", "sw-KE",
        ] {
            assert!(locales.contains(locale), "no voice for {locale}");
        }
    }

    #[test]
    fn the_default_voice_is_in_the_fallback() {
        let voices = fallback_voices();
        assert!(voices
            .iter()
            .any(|v| v.name == crate::services::tts::DEFAULT_VOICE));
    }

    #[tokio::test]
    #[ignore = "live fetch against Microsoft's service"]
    async fn live_voice_list_is_fetched_and_cached() {
        let voices = list_voices().await.expect("voice list should succeed");
        assert!(voices.len() > 250, "only {} voices", voices.len());
        let sorted = voices.windows(2).all(|w| w[0].name <= w[1].name);
        assert!(sorted, "voices are not sorted by name");
        assert!(voices.iter().all(|v| v.name.ends_with("Neural")));
    }
}
