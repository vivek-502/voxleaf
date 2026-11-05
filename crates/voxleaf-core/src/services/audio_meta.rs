//! Audio container inspection using `lofty`.
//!
//! Upload validation reads the *bytes*, never the filename or the `Content-Type`
//! the browser sent: the file is parsed to confirm this really is audio before
//! anything is written to storage, and the duration it reports is what the
//! player's timeline is built from.

use std::io::Cursor;

use lofty::file::{AudioFile, FileType, TaggedFile, TaggedFileExt};
use lofty::probe::Probe;

/// What an upload turned out to be, once parsed.
#[derive(Debug, Clone, PartialEq)]
pub struct AudioProperties {
    /// Seconds, rounded to three decimals — the player's timeline only ever
    /// shows tenths, and a float that never ends is not worth a row.
    pub duration: f64,
    /// Bits per second, matching what the web app's metadata parser reported.
    pub bit_rate: Option<i64>,
    pub sample_rate: Option<i64>,
    pub channels: Option<i64>,
    pub mime_type: String,
}

/// A refusal with enough context for the API layer to name the offending file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioMetaError {
    pub field: &'static str,
    pub message: String,
}

impl AudioMetaError {
    fn content(message: &str) -> Self {
        Self {
            field: "content",
            message: message.to_string(),
        }
    }
}

/// The container's own MIME type, derived from the bytes rather than believed
/// from the upload.
fn mime_for_file_type(file_type: FileType) -> &'static str {
    match file_type {
        FileType::Mpeg => "audio/mpeg",
        FileType::Mp4 => "audio/mp4",
        FileType::Aac => "audio/aac",
        FileType::Flac => "audio/flac",
        FileType::Wav => "audio/wav",
        FileType::Vorbis | FileType::Opus | FileType::Speex => "audio/ogg",
        FileType::Aiff => "audio/aiff",
        FileType::Ape => "audio/x-ape",
        FileType::Mpc => "audio/x-musepack",
        FileType::WavPack => "audio/x-wavpack",
        // `FileType` is non-exhaustive, and a custom resolver's types are not
        // ours to name.
        _ => "application/octet-stream",
    }
}

/// A ceiling on what a broken header may claim: thirty days, which no chapter
/// audio file has ever approached.
const MAX_PLAUSIBLE_SECONDS: f64 = 60.0 * 60.0 * 24.0 * 30.0;

#[derive(Debug, Default, Clone)]
pub struct AudioMeta;

impl AudioMeta {
    pub fn new() -> Self {
        Self
    }

    /// Parse `bytes`, returning the container's properties.
    ///
    /// The three refusals mirror the web app's, in the same order: unreadable
    /// bytes, no readable audio track, and an implausible duration.
    pub fn inspect(&self, bytes: &[u8], _filename: &str) -> Result<AudioProperties, AudioMetaError> {
        let refusal = || {
            tracing::warn!("audio upload could not be parsed");
            AudioMetaError::content(
                "That file could not be read as audio. It may be corrupt or renamed from another format.",
            )
        };
        let tagged: TaggedFile = Probe::new(Cursor::new(bytes))
            .guess_file_type()
            .map_err(|_| refusal())?
            .read()
            .map_err(|_| refusal())?;

        let file_type = tagged.file_type();
        let properties = tagged.properties();
        let duration = properties.duration().as_secs_f64();

        if !duration.is_finite() || duration <= 0.0 {
            return Err(AudioMetaError::content(
                "That file has no readable audio track. If it is a video container, export the audio only.",
            ));
        }
        if duration > MAX_PLAUSIBLE_SECONDS {
            return Err(AudioMetaError::content("That file reports an implausible duration."));
        }

        Ok(AudioProperties {
            // `Math.round(duration * 1000) / 1000`, exactly.
            duration: (duration * 1000.0).round() / 1000.0,
            // Lofty counts kilobits; the web app's parser counted bits.
            bit_rate: properties.overall_bitrate().map(|kbps| i64::from(kbps) * 1000),
            sample_rate: properties.sample_rate().map(i64::from),
            channels: properties.channels().map(i64::from),
            mime_type: mime_for_file_type(file_type).to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A one-second, 8 kHz mono WAV of silence: real audio, built here so the
    /// test never depends on a fixture file.
    fn tiny_wav() -> Vec<u8> {
        let sample_rate: u32 = 8_000;
        let samples: u32 = sample_rate; // one second
        let data_size = samples * 2; // 16-bit mono
        let mut wav = Vec::with_capacity(44 + data_size as usize);
        wav.extend_from_slice(b"RIFF");
        wav.extend_from_slice(&(36 + data_size).to_le_bytes());
        wav.extend_from_slice(b"WAVE");
        wav.extend_from_slice(b"fmt ");
        wav.extend_from_slice(&16u32.to_le_bytes());
        wav.extend_from_slice(&1u16.to_le_bytes()); // PCM
        wav.extend_from_slice(&1u16.to_le_bytes()); // mono
        wav.extend_from_slice(&sample_rate.to_le_bytes());
        wav.extend_from_slice(&(sample_rate * 2).to_le_bytes()); // byte rate
        wav.extend_from_slice(&2u16.to_le_bytes()); // block align
        wav.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&data_size.to_le_bytes());
        wav.resize(44 + data_size as usize, 0);
        wav
    }

    #[test]
    fn a_real_wav_is_read_with_its_properties() {
        let properties = AudioMeta::new()
            .inspect(&tiny_wav(), "chapter.wav")
            .expect("a wav is audio");
        assert_eq!(properties.duration, 1.0);
        assert_eq!(properties.mime_type, "audio/wav");
        assert_eq!(properties.sample_rate, Some(8_000));
        assert_eq!(properties.channels, Some(1));
        assert!(properties.bit_rate.is_some());
    }

    #[test]
    fn duration_is_rounded_to_thousandths() {
        // 8010 samples at 8 kHz is 1.00125 seconds.
        let mut wav = tiny_wav();
        let extra = [0u8; 20];
        wav[40..44].copy_from_slice(&((8_000 + 10) * 2u32).to_le_bytes());
        wav[4..8].copy_from_slice(&((36 + (8_010 * 2)) as u32).to_le_bytes());
        wav.extend_from_slice(&extra);

        let properties = AudioMeta::new().inspect(&wav, "odd.wav").expect("a wav is audio");
        assert_eq!(properties.duration, 1.001);
    }

    #[test]
    fn garbage_is_refused_with_a_readable_message() {
        let error = AudioMeta::new()
            .inspect(b"this is not audio at all, not even slightly", "song.mp3")
            .expect_err("garbage is not audio");
        assert_eq!(error.field, "content");
        assert_eq!(
            error.message,
            "That file could not be read as audio. It may be corrupt or renamed from another format."
        );
    }

    #[test]
    fn an_empty_file_is_refused() {
        let error = AudioMeta::new()
            .inspect(b"", "empty.wav")
            .expect_err("nothing to parse");
        assert_eq!(error.field, "content");
    }

    #[test]
    fn a_renamed_text_file_is_refused() {
        let error = AudioMeta::new()
            .inspect(&b"Chapter one. It was a dark and stormy night.".repeat(50), "book.mp3")
            .expect_err("text is not audio");
        assert_eq!(error.field, "content");
    }
}
