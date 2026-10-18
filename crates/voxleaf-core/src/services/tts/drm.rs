//! The `Sec-MS-GEC` token Microsoft's read-aloud service demands on every
//! request.
//!
//! This is a line-for-line port of `edge_tts/drm.py` (v2 token). The
//! construction is a custom SHA-256 mid-state resume combined with a "CS64"
//! checksum: it is *not* plain SHA-256 of the timestamp (verified: the pinned
//! test vectors below differ from `sha256(data)`), so every constant and every
//! truncation order here matters and was pinned against the Python
//! implementation that the web app shipped with.
//!
//! The token covers the current time in Windows file-time ticks (100 ns units
//! since 1601), rounded down to a five-minute window, hashed together with the
//! service's trusted client token. A wrong hash is answered with a 403, which
//! is why the tests below pin the algorithm against Python-computed vectors.

use std::sync::atomic::{AtomicU64, Ordering};

use crate::services::tts::client::TRUSTED_CLIENT_TOKEN;

/// Seconds between the Windows epoch (1601-01-01) and the Unix epoch.
const WIN_EPOCH: f64 = 11_644_473_600.0;
/// Nanoseconds per second, scaled the way the original divides it (S_TO_NS/100).
const S_TO_NS_DIV_100: f64 = 1e9 / 100.0;
/// The token window: timestamps are rounded down to a multiple of this.
const WINDOW_SECONDS: f64 = 300.0;

const SHA256_INITIAL: [u32; 8] = [
    0x6A09E667, 0xBB67AE85, 0x3C6EF372, 0xA54FF53A, 0x510E527F, 0x9B05688C, 0x1F83D9AB, 0x5BE0CD19,
];
const SHA256_STATE: [u32; 8] = [
    0xFD9FBEA2, 0x91243CE7, 0xBE499094, 0xCD2E1E0A, 0x6C006F49, 0x04BD35AB, 0xFEB0A7E4, 0x03EFFBD2,
];
/// Expanded schedule words 16..29 of the trusted first block.
const SHA256_SCHEDULE: [u32; 14] = [
    0xEB24BF9E, 0x1558691F, 0xA0C09EE2, 0x0CFA32BA, 0xA622EE8C, 0xD1DD9491, 0xC0DD25A0, 0xF9E70AB6,
    0xF0C31573, 0x7058CE4E, 0x7188E606, 0x00E5AB33, 0xD258B060, 0x72766564,
];
#[rustfmt::skip]
const SHA256_ROUND_CONSTANTS: [u32; 64] = [
    0x428A2F98, 0x71374491, 0xB5C0FBCF, 0xE9B5DBA5, 0x3956C25B, 0x59F111F1, 0x923F82A4, 0xAB1C5ED5,
    0xD807AA98, 0x12835B01, 0x243185BE, 0x550C7DC3, 0x72BE5D74, 0x80DEB1FE, 0x9BDC06A7, 0xC19BF174,
    0xE49B69C1, 0xEFBE4786, 0x0FC19DC6, 0x240CA1CC, 0x2DE92C6F, 0x4A7484AA, 0x5CB0A9DC, 0x76F988DA,
    0x983E5152, 0xA831C66D, 0xB00327C8, 0xBF597FC7, 0xC6E00BF3, 0xD5A79147, 0x06CA6351, 0x14292967,
    0x27B70A85, 0x2E1B2138, 0x4D2C6DFC, 0x53380D13, 0x650A7354, 0x766A0ABB, 0x81C2C92E, 0x92722C85,
    0xA2BFE8A1, 0xA81A664B, 0xC24B8B70, 0xC76C51A3, 0xD192E819, 0xD6990624, 0xF40E3585, 0x106AA070,
    0x19A4C116, 0x1E376C08, 0x2748774C, 0x34B0BCB5, 0x391C0CB3, 0x4ED8AA4A, 0x5B9CCA4F, 0x682E6FF3,
    0x748F82EE, 0x78A5636F, 0x84C87814, 0x8CC70208, 0x90BEFFFA, 0xA4506CEB, 0xBEF9A3F7, 0xC67178F2,
];
/// Round 14's constant is swapped when the first block resumes mid-way.
const SHA256_ADJUSTMENT: u32 = 0xC9DC06A7;
#[rustfmt::skip]
fn sha256_block_constants() -> [u32; 64] {
    let mut constants = SHA256_ROUND_CONSTANTS;
    constants[14] = SHA256_ADJUSTMENT;
    constants
}
const SHA256_SIGMA0_ADJUSTMENT: u64 = 0x059C_0B80;
const SHA256_SIGMA1_ADJUSTMENT: u64 = 0x000B_92C0;

#[rustfmt::skip]
const CS64_COEFFICIENTS: [(u32, u32); 14] = [
    (0xE82E91B1, 0xB32700D0),
    (0x44DE24C6, 0x88DFF51D),
    (0x3BEB65BC, 0x29514038),
    (0xC89C5933, 0xB328CBDF),
    (0x8D1EC211, 0xD405B4D5),
    (0x314EF5D5, 0x26E55F3C),
    (0xE92491B1, 0x607801C6),
    (0xF0A5B9D5, 0x82B2C1FB),
    (0x06064CF3, 0xFA51A3DA),
    (0xDA79FA60, 0x207CC464),
    (0xBED20560, 0x3C3C74F3),
    (0x89F9C9F8, 0xD2D14B61),
    (0x35FA38C6, 0xBB04A2C1),
    (0xCB49E3C6, 0xC5626167),
];
const CS64_FACTOR: u32 = 0x8289_3933;
const CS64_ADJUSTMENT0: u32 = 0xF2F9_0B1A;
const CS64_ADJUSTMENT1: u32 = 0x0B1A_0000;

/// Clock skew between this machine and Microsoft's, in seconds, accumulated
/// from `Date` headers on 403 responses exactly like the Python `DRM` class.
static CLOCK_SKEW_BITS: AtomicU64 = AtomicU64::new(0);

fn clock_skew() -> f64 {
    f64::from_bits(CLOCK_SKEW_BITS.load(Ordering::Relaxed))
}

/// Fold a server-reported skew into the clock used for tokens.
pub fn adjust_clock_skew(seconds: f64) {
    let _ = CLOCK_SKEW_BITS.try_update(Ordering::Relaxed, Ordering::Relaxed, |bits| {
        Some((f64::from_bits(bits) + seconds).to_bits())
    });
}

/// Current UTC timestamp (Unix seconds, fractional) plus the skew.
pub fn unix_timestamp() -> f64 {
    let now = chrono::Utc::now();
    now.timestamp() as f64 + f64::from(now.timestamp_subsec_nanos()) / 1e9 + clock_skew()
}

/// Parse an RFC 2616 `Date` header into a Unix timestamp.
pub fn parse_rfc2616_date(date: &str) -> Option<f64> {
    let parsed = chrono::DateTime::parse_from_rfc2822(date.trim()).ok()?;
    Some(parsed.timestamp() as f64 + f64::from(parsed.timestamp_subsec_nanos()) / 1e9)
}

/// Adjust the clock skew from a server `Date` header, as the Python client
/// does when the service answers a request with a 403.
pub fn adjust_from_server_date(date: &str) {
    if let Some(server) = parse_rfc2616_date(date) {
        adjust_clock_skew(server - unix_timestamp());
    }
}

/// Rotate a 32-bit value left by 16 bits.
fn rol16(value: u32) -> u32 {
    value.rotate_left(16)
}

/// Split data into little-endian 32-bit words, zero-padded to an even count.
fn words_le4_even(data: &[u8]) -> Vec<u32> {
    let mut padded = data.to_vec();
    let rem = padded.len() % 4;
    if rem != 0 {
        padded.extend(std::iter::repeat_n(0u8, 4 - rem));
    }
    let mut words: Vec<u32> = padded
        .chunks_exact(4)
        .map(|chunk| u32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
        .collect();
    if words.len() % 2 == 1 {
        words.push(0);
    }
    words
}

/// SHA-256 small sigma zero, truncating the combined rotations once.
fn sigma0(value: u64) -> u64 {
    (value >> 7) ^ (value << 25) ^ (value >> 18) ^ (value << 14) ^ (value >> 3)
}

/// SHA-256 small sigma one, truncating the combined rotations once.
fn sigma1(value: u64) -> u64 {
    (value >> 17) ^ (value << 15) ^ (value >> 19) ^ (value << 13) ^ (value >> 10)
}

/// Run SHA-256 rounds, optionally resuming inside the first block.
///
/// `words` is the full 64-word expanded schedule; `first_round` selects where
/// to resume and which constant table to use.
fn sha256_rounds(state: [u32; 8], words: &[u32], first_round: usize) -> [u32; 8] {
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
    let block_constants = sha256_block_constants();
    let constants: &[u32; 64] = if first_round == 14 {
        &block_constants
    } else {
        &SHA256_ROUND_CONSTANTS
    };

    // Multiplying a 32-bit word by 2**32 + 1 repeats it in 64 bits.
    // A right shift then has ROTR's low 32 bits, with no separate left shift.
    let aligned = std::cmp::min(64, ((first_round + 7) / 8) * 8);
    for index in first_round..aligned {
        let rot_e = u64::from(e) * 0x1_0000_0001;
        let rot_a = u64::from(a) * 0x1_0000_0001;
        let t1 = u64::from(h)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(g ^ (e & (f ^ g)))
            + u64::from(constants[index])
            + u64::from(words[index]);
        let t2 = ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((a & b) | (c & (a | b)));
        let (a_old, b_old, c_old, d_old, e_old, f_old, g_old, _h_old) = (a, b, c, d, e, f, g, h);
        a = t1.wrapping_add(t2) as u32;
        b = a_old;
        c = b_old;
        d = c_old;
        e = d_old.wrapping_add(t1 as u32);
        f = e_old;
        g = f_old;
        h = g_old;
    }

    // Eight rounds at a time, with the working registers addressed in their
    // rotated roles rather than shuffled after every round.
    for index in (aligned..64).step_by(8) {
        // Round index + 0.
        let rot_e = u64::from(e) * 0x1_0000_0001;
        let rot_a = u64::from(a) * 0x1_0000_0001;
        let t1 = u64::from(h)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(g ^ (e & (f ^ g)))
            + u64::from(constants[index])
            + u64::from(words[index]);
        d = d.wrapping_add(t1 as u32);
        h = (t1
            + ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((a & b) | (c & (a | b)))) as u32;

        // Round index + 1.
        let rot_e = u64::from(d) * 0x1_0000_0001;
        let rot_a = u64::from(h) * 0x1_0000_0001;
        let t1 = u64::from(g)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(f ^ (d & (e ^ f)))
            + u64::from(constants[index + 1])
            + u64::from(words[index + 1]);
        c = c.wrapping_add(t1 as u32);
        g = (t1
            + ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((h & a) | (b & (h | a)))) as u32;

        // Round index + 2.
        let rot_e = u64::from(c) * 0x1_0000_0001;
        let rot_a = u64::from(g) * 0x1_0000_0001;
        let t1 = u64::from(f)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(e ^ (c & (d ^ e)))
            + u64::from(constants[index + 2])
            + u64::from(words[index + 2]);
        b = b.wrapping_add(t1 as u32);
        f = (t1
            + ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((g & h) | (a & (g | h)))) as u32;

        // Round index + 3.
        let rot_e = u64::from(b) * 0x1_0000_0001;
        let rot_a = u64::from(f) * 0x1_0000_0001;
        let t1 = u64::from(e)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(d ^ (b & (c ^ d)))
            + u64::from(constants[index + 3])
            + u64::from(words[index + 3]);
        a = a.wrapping_add(t1 as u32);
        e = (t1
            + ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((f & g) | (h & (f | g)))) as u32;

        // Round index + 4.
        let rot_e = u64::from(a) * 0x1_0000_0001;
        let rot_a = u64::from(e) * 0x1_0000_0001;
        let t1 = u64::from(d)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(c ^ (a & (b ^ c)))
            + u64::from(constants[index + 4])
            + u64::from(words[index + 4]);
        h = h.wrapping_add(t1 as u32);
        d = (t1
            + ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((e & f) | (g & (e | f)))) as u32;

        // Round index + 5.
        let rot_e = u64::from(h) * 0x1_0000_0001;
        let rot_a = u64::from(d) * 0x1_0000_0001;
        let t1 = u64::from(c)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(b ^ (h & (a ^ b)))
            + u64::from(constants[index + 5])
            + u64::from(words[index + 5]);
        g = g.wrapping_add(t1 as u32);
        c = (t1
            + ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((d & e) | (f & (d | e)))) as u32;

        // Round index + 6.
        let rot_e = u64::from(g) * 0x1_0000_0001;
        let rot_a = u64::from(c) * 0x1_0000_0001;
        let t1 = u64::from(b)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(a ^ (g & (h ^ a)))
            + u64::from(constants[index + 6])
            + u64::from(words[index + 6]);
        f = f.wrapping_add(t1 as u32);
        b = (t1
            + ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((c & d) | (e & (c | d)))) as u32;

        // Round index + 7.
        let rot_e = u64::from(f) * 0x1_0000_0001;
        let rot_a = u64::from(b) * 0x1_0000_0001;
        let t1 = u64::from(a)
            + ((rot_e >> 6) ^ (rot_e >> 11) ^ (rot_e >> 25))
            + u64::from(h ^ (f & (g ^ h)))
            + u64::from(constants[index + 7])
            + u64::from(words[index + 7]);
        e = e.wrapping_add(t1 as u32);
        a = (t1
            + ((rot_a >> 2) ^ (rot_a >> 13) ^ (rot_a >> 22))
            + u64::from((b & c) | (d & (b | c)))) as u32;
    }

    [a, b, c, d, e, f, g, h]
}

/// Expand SHA-256 schedule words, combining rotations before the modular
/// addition — high bits are kept until the final mask, exactly as the
/// reference does.
fn expand_sha256_schedule(words: &mut Vec<u32>, first_word: usize) {
    for index in first_word..64 {
        let x = u64::from(words[index - 2]);
        let y = u64::from(words[index - 15]);
        let s1 = (x >> 17) ^ (x << 15) ^ (x >> 19) ^ (x << 13) ^ (x >> 10);
        let s0 = (y >> 7) ^ (y << 25) ^ (y >> 18) ^ (y << 14) ^ (y >> 3);
        let value = s1
            + u64::from(words[index - 7])
            + s0
            + u64::from(words[index - 16]);
        words.push(value as u32);
    }
}

/// Compute the intermediate SHA-256 state for token data: the trusted first
/// block's mid-state, resumed with the token's first seven bytes.
fn sha256_init(data: &[u8]) -> [u32; 8] {
    let mut words: Vec<u32> = vec![0; 14];
    words.push(
        (u32::from(data[0]) << 16) | (u32::from(data[1]) << 8) | u32::from(data[2]),
    );
    words.push(u32::from_be_bytes([data[3], data[4], data[5], data[6]]));

    for index in 16..30 {
        let mut value = u64::from(SHA256_SCHEDULE[index - 16]);
        if index - 2 >= 14 {
            value += if index == 16 {
                sigma1(u64::from(words[index - 2])) ^ SHA256_SIGMA1_ADJUSTMENT
            } else {
                sigma1(u64::from(words[index - 2]))
            };
        }
        if index - 7 >= 14 {
            value += u64::from(words[index - 7]);
        }
        if index - 15 >= 14 {
            value += if index == 29 {
                sigma0(u64::from(words[index - 15])) ^ SHA256_SIGMA0_ADJUSTMENT
            } else {
                sigma0(u64::from(words[index - 15]))
            };
        }
        words.push(value as u32);
    }

    // The block constant for round 14 is folded into word 30 here, so the
    // resumed rounds can use the adjusted table instead.
    words.push(
        (sigma1(u64::from(words[28]))
            + u64::from(words[23])
            + sigma0(u64::from(words[15]))
            + u64::from(words[14])
            + u64::from(SHA256_ADJUSTMENT)
            .wrapping_sub(u64::from(SHA256_ROUND_CONSTANTS[14]))) as u32,
    );
    expand_sha256_schedule(&mut words, 31);

    let working = sha256_rounds(SHA256_STATE, &words, 14);
    let mut state = [0u32; 8];
    for index in 0..8 {
        state[index] = SHA256_INITIAL[index].wrapping_add(working[index]);
    }
    state
}

/// Compute the (non-standard) SHA-256 digest for token data.
///
/// The padding matches SHA-256, but the encoded length is that of the data
/// plus the trusted seven-byte prefix — this is the service's own
/// construction, reproduced faithfully.
fn sha256_digest(data: &[u8]) -> [u8; 32] {
    assert!(data.len() >= 7, "Token salt must contain at least seven bytes");
    let length = 57 + data.len();
    let mut padded = data[7..].to_vec();
    padded.push(0x80);
    padded.extend(std::iter::repeat_n(
        0u8,
        (55i64 - length as i64).rem_euclid(64) as usize,
    ));
    padded.extend_from_slice(&((length as u64) * 8).to_be_bytes());

    let mut state = sha256_init(&data[..7]);
    for block in padded.chunks_exact(64) {
        let mut words: Vec<u32> = block
            .chunks_exact(4)
            .map(|chunk| u32::from_be_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();
        expand_sha256_schedule(&mut words, 16);
        let working = sha256_rounds(state, &words, 0);
        for index in 0..8 {
            state[index] = state[index].wrapping_add(working[index]);
        }
    }

    let mut out = [0u8; 32];
    for index in 0..8 {
        out[index * 4..index * 4 + 4].copy_from_slice(&state[index].to_be_bytes());
    }
    out
}

/// Compute the CS64 digest words for token data.
fn cs64_digest(data: &[u8], digest: &[u8]) -> (u32, u32) {
    // The trusted-value masking: the two key words are forced odd.
    let key_a = u32::from_le_bytes([digest[0], digest[1], digest[2], digest[3]]) | 1;
    let key_b = u32::from_le_bytes([digest[4], digest[5], digest[6], digest[7]]) | 1;
    let factors = [key_a.wrapping_mul(CS64_FACTOR), key_b.wrapping_mul(CS64_FACTOR)];

    let mut terms: Vec<u32> = CS64_COEFFICIENTS
        .iter()
        .enumerate()
        .map(|(index, (coefficient, _))| coefficient.wrapping_mul(factors[index % 2]))
        .collect();
    let mut offsets: Vec<u32> = CS64_COEFFICIENTS.iter().map(|(_, offset)| *offset).collect();

    let mut prefixed = vec![0u8];
    prefixed.extend_from_slice(data);
    for (index, word) in words_le4_even(&prefixed).into_iter().enumerate() {
        let mut term = word.wrapping_mul(if index % 2 == 0 { key_a } else { key_b });
        let mut offset = rol16(word).wrapping_mul(if index % 2 == 0 {
            0xEF05_69FB
        } else {
            0xC317_13DB
        });
        if index == 0 {
            term = term.wrapping_add(CS64_ADJUSTMENT0.wrapping_mul(factors[0]));
            offset = offset.wrapping_add(CS64_ADJUSTMENT1);
        }
        terms.push(term);
        offsets.push(offset);
    }

    let (mut swap_acc, mut swap_last, mut state, mut acc) = (0u32, 0u32, 0u32, 0u32);
    for index in (0..terms.len()).step_by(2) {
        let (x, y) = (terms[index], terms[index + 1]);

        let t = rol16(offsets[index].wrapping_add(x));
        let q = t.wrapping_mul(0x76F5_0F34);
        swap_acc = swap_acc.wrapping_add(rol16(q).wrapping_mul(0xC3EF_EA97).wrapping_add(q));
        let t = rol16(offsets[index + 1].wrapping_add(y));
        let q = t.wrapping_mul(0x1881_CE3C);
        swap_last = rol16(q).wrapping_mul(0x35BD_1EC9).wrapping_add(q);
        swap_acc = swap_acc.wrapping_add(swap_last);

        state = state.wrapping_mul(key_a).wrapping_add(x);
        state = rol16(state).wrapping_mul(0xCF98_B111);
        state = rol16(state).wrapping_mul(0x8708_5B9F);
        state = rol16(state).wrapping_mul(0x12CE_B96D);
        state = rol16(state).wrapping_mul(0x257E_1D83);
        acc = acc.wrapping_add(state);

        let mut t = state.wrapping_mul(key_b).wrapping_add(y);
        t = rol16(t).wrapping_mul(0xA274_16F5);
        t = rol16(t).wrapping_mul(0xD383_96FF);
        state = rol16(rol16(t).wrapping_mul(0x7C93_2B89)).wrapping_mul(0xBFA4_9F69);
        acc = acc.wrapping_add(state);
    }

    (swap_last ^ state, swap_acc ^ acc)
}

/// The two token hash components: `<16 hex cs64>-<64 hex sha256>`, upper-case.
fn token_hash(data: &[u8]) -> (String, String) {
    let digest = sha256_digest(data);
    let (word_a, word_b) = cs64_digest(data, &digest);
    let mut packed = [0u8; 8];
    packed[0..4].copy_from_slice(&word_a.to_le_bytes());
    packed[4..8].copy_from_slice(&word_b.to_le_bytes());
    (hex::encode_upper(packed), hex::encode_upper(digest))
}

/// Generate a v2 `Sec-MS-GEC` token for an explicit Unix timestamp
/// (pass `None` for the current, skew-corrected time).
///
/// The token format is `"<16 hex>-<64 hex>"`.
pub fn generate_sec_ms_gec_v2(unix_timestamp: Option<f64>) -> String {
    let mut ticks = unix_timestamp.unwrap_or_else(super::drm::unix_timestamp);
    ticks += WIN_EPOCH;
    ticks -= ticks % WINDOW_SECONDS;
    ticks *= S_TO_NS_DIV_100;
    // `{ticks:.0}` rounds the double the same way Python's `f"{ticks:.0f}"`
    // does (ties-to-even on the exact binary value), which matters because the
    // value has long since left the range where a double is exact.
    let str_to_hash = format!("{ticks:.0}{TRUSTED_CLIENT_TOKEN}");
    let (cs64, sha256) = token_hash(str_to_hash.as_bytes());
    format!("{cs64}-{sha256}")
}

/// Generate the token the service expects right now.
pub fn generate_sec_ms_gec() -> String {
    generate_sec_ms_gec_v2(None)
}

/// A random machine id, upper-case hex, sent as a `muid` cookie.
pub fn generate_muid() -> String {
    use rand::RngCore;
    let mut bytes = [0u8; 16];
    rand::rng().fill_bytes(&mut bytes);
    hex::encode_upper(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sha2::Digest;

    /// Tokens computed against the reference Python implementation
    /// (`edge_tts.drm.generate_sec_ms_gec_v2`) that the original web app
    /// shipped with, e.g.:
    /// `python -c "from edge_tts.drm import generate_sec_ms_gec_v2 as g; print(g(0.0))"`
    /// These pin the whole pipeline: float formatting, the 300-second window,
    /// the custom SHA-256 mid-state resume, CS64, and the hex packing order.
    const VECTORS: [(f64, &str); 5] = [
        (
            0.0,
            "52BC7A17B50DFEC9-28C4394775C8751CD2F6859C030C06DB1489ACAB3B3A7651831FC26895177ED6",
        ),
        (
            1_000_000_000.0,
            "5E9153719FADA922-14D2A75113EC5862E17303287DCDB197E3A671DF9FCC53C100D6B27928384B98",
        ),
        (
            1_700_000_000.0,
            "2CD23A2CA0B2595A-F3C846C370E05F9CC83ACC5F995C5F5D87A4D0F14CE8936A0F902E64099E38A2",
        ),
        (
            1_760_000_123.0,
            "BAF4076F572BFE4E-2D6FBE5C0E24F56622EB94F37395F2D153C6C0C451DAAE2249A459BB3AFAD62C",
        ),
        (
            123_456_789.123_456,
            "A15ACB230E2826CE-5B9EA8174ABA0AE736D5AB47803D7958CB03226746F0951EB604FA9AB0BF1ED4",
        ),
    ];

    #[test]
    fn tokens_match_the_python_reference() {
        for (timestamp, expected) in VECTORS {
            assert_eq!(
                generate_sec_ms_gec_v2(Some(timestamp)),
                expected,
                "token mismatch for timestamp {timestamp}"
            );
        }
    }

    #[test]
    fn token_shape_is_16_hex_dash_64_hex_uppercase() {
        for (timestamp, _) in VECTORS {
            let token = generate_sec_ms_gec_v2(Some(timestamp));
            let (cs64, sha) = token.split_once('-').expect("token has one dash");
            assert_eq!(cs64.len(), 16);
            assert_eq!(sha.len(), 64);
            assert!(cs64.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(sha.chars().all(|c| c.is_ascii_hexdigit()));
            assert_eq!(cs64, cs64.to_uppercase());
            assert_eq!(sha, sha.to_uppercase());
        }
    }

    #[test]
    fn timestamps_inside_one_window_share_a_token() {
        // The window floors `unix + WIN_EPOCH` to a multiple of 300, and
        // `WIN_EPOCH` is itself a multiple of 300 — so an aligned Unix
        // timestamp such as 1_700_000_100 anchors its own five-minute window.
        assert_eq!(
            generate_sec_ms_gec_v2(Some(1_700_000_100.0)),
            generate_sec_ms_gec_v2(Some(1_700_000_399.0)),
            "same window, one token"
        );
        assert_ne!(
            generate_sec_ms_gec_v2(Some(1_700_000_100.0)),
            generate_sec_ms_gec_v2(Some(1_700_000_099.0)),
            "the window before is a different token"
        );
        assert_ne!(
            generate_sec_ms_gec_v2(Some(1_700_000_100.0)),
            generate_sec_ms_gec_v2(Some(1_700_000_400.0)),
            "the window after is a different token"
        );
    }

    #[test]
    fn the_digest_is_not_plain_sha256() {
        // Guard against a "simplification" that would silently break the
        // handshake: the token hash is a bespoke construction, not SHA-256 of
        // the timestamp string.
        let data = b"1334447340000000006A5AA1D4EAFF4E9FB37E23D68491D6F4";
        let digest = sha256_digest(data);
        let plain = sha2::Sha256::digest(data);
        assert_ne!(digest.as_slice(), plain.as_slice());
        // Pinned against Python's `_sha256_digest`.
        assert_eq!(
            hex::encode_upper(digest),
            "F3C846C370E05F9CC83ACC5F995C5F5D87A4D0F14CE8936A0F902E64099E38A2"
        );
    }

    #[test]
    fn rfc2616_dates_parse_to_the_right_instant() {
        let parsed = parse_rfc2616_date("Wed, 21 Oct 2015 07:28:00 GMT").expect("parse");
        assert_eq!(parsed as i64, 1_445_412_480);
        assert!(parse_rfc2616_date("not a date").is_none());
    }
}
