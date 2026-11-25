//! Pure-Rust AES-128-CTR stream cipher for Yandex Music's `encraw` audio transport.
//!
//! Yandex encrypts `encraw` streams (such as lossless `flac-mp4`) with AES-128 in CTR mode using a
//! 16-byte all-zero initial counter (`nonce = 0`, 128-bit big-endian block counter). Because CTR
//! mode derives each 16-byte keystream block independently from `byte_offset / 16`, decryption is
//! streaming, zero-allocation, and directly seekable to any byte offset for HTTP Range requests.

/// Standard FIPS-197 AES forward S-box.
#[rustfmt::skip]
const SBOX: [u8; 256] = [
    0x63, 0x7c, 0x77, 0x7b, 0xf2, 0x6b, 0x6f, 0xc5, 0x30, 0x01, 0x67, 0x2b, 0xfe, 0xd7, 0xab, 0x76,
    0xca, 0x82, 0xc9, 0x7d, 0xfa, 0x59, 0x47, 0xf0, 0xad, 0xd4, 0xa2, 0xaf, 0x9c, 0xa4, 0x72, 0xc0,
    0xb7, 0xfd, 0x93, 0x26, 0x36, 0x3f, 0xf7, 0xcc, 0x34, 0xa5, 0xe5, 0xf1, 0x71, 0xd8, 0x31, 0x15,
    0x04, 0xc7, 0x23, 0xc3, 0x18, 0x96, 0x05, 0x9a, 0x07, 0x12, 0x80, 0xe2, 0xeb, 0x27, 0xb2, 0x75,
    0x09, 0x83, 0x2c, 0x1a, 0x1b, 0x6e, 0x5a, 0xa0, 0x52, 0x3b, 0xd6, 0xb3, 0x29, 0xe3, 0x2f, 0x84,
    0x53, 0xd1, 0x00, 0xed, 0x20, 0xfc, 0xb1, 0x5b, 0x6a, 0xcb, 0xbe, 0x39, 0x4a, 0x4c, 0x58, 0xcf,
    0xd0, 0xef, 0xaa, 0xfb, 0x43, 0x4d, 0x33, 0x85, 0x45, 0xf9, 0x02, 0x7f, 0x50, 0x3c, 0x9f, 0xa8,
    0x51, 0xa3, 0x40, 0x8f, 0x92, 0x9d, 0x38, 0xf5, 0xbc, 0xb6, 0xda, 0x21, 0x10, 0xff, 0xf3, 0xd2,
    0xcd, 0x0c, 0x13, 0xec, 0x5f, 0x97, 0x44, 0x17, 0xc4, 0xa7, 0x7e, 0x3d, 0x64, 0x5d, 0x19, 0x73,
    0x60, 0x81, 0x4f, 0xdc, 0x22, 0x2a, 0x90, 0x88, 0x46, 0xee, 0xb8, 0x14, 0xde, 0x5e, 0x0b, 0xdb,
    0xe0, 0x32, 0x3a, 0x0a, 0x49, 0x06, 0x24, 0x5c, 0xc2, 0xd3, 0xac, 0x62, 0x91, 0x95, 0xe4, 0x79,
    0xe7, 0xc8, 0x37, 0x6d, 0x8d, 0xd5, 0x4e, 0xa9, 0x6c, 0x56, 0xf4, 0xea, 0x65, 0x7a, 0xae, 0x08,
    0xba, 0x78, 0x25, 0x2e, 0x1c, 0xa6, 0xb4, 0xc6, 0xe8, 0xdd, 0x74, 0x1f, 0x4b, 0xbd, 0x8b, 0x8a,
    0x70, 0x3e, 0xb5, 0x66, 0x48, 0x03, 0xf6, 0x0e, 0x61, 0x35, 0x57, 0xb9, 0x86, 0xc1, 0x1d, 0x9e,
    0xe1, 0xf8, 0x98, 0x11, 0x69, 0xd9, 0x8e, 0x94, 0x9b, 0x1e, 0x87, 0xe9, 0xce, 0x55, 0x28, 0xdf,
    0x8c, 0xa1, 0x89, 0x0d, 0xbf, 0xe6, 0x42, 0x68, 0x41, 0x99, 0x2d, 0x0f, 0xb0, 0x54, 0xbb, 0x16,
];

const RCON: [u8; 10] = [0x01, 0x02, 0x04, 0x08, 0x10, 0x20, 0x40, 0x80, 0x1b, 0x36];

/// Expanded AES-128 key schedule for CTR-mode decryption at any byte offset.
#[derive(Clone, Copy)]
pub(crate) struct Aes128Ctr {
    rk: [[u8; 16]; 11],
}

impl Aes128Ctr {
    pub(crate) fn new(key: [u8; 16]) -> Self {
        let mut rk = [[0u8; 16]; 11];
        rk[0] = key;
        for round in 1..=10 {
            let prev = rk[round - 1];
            let mut next = [0u8; 16];
            // RotWord + SubWord + Rcon on the last 4-byte word of the previous round key.
            next[0] = prev[0] ^ SBOX[usize::from(prev[13])] ^ RCON[round - 1];
            next[1] = prev[1] ^ SBOX[usize::from(prev[14])];
            next[2] = prev[2] ^ SBOX[usize::from(prev[15])];
            next[3] = prev[3] ^ SBOX[usize::from(prev[12])];
            for i in 4..16 {
                next[i] = prev[i] ^ next[i - 4];
            }
            rk[round] = next;
        }
        Self { rk }
    }

    fn encrypt_block(&self, block: [u8; 16]) -> [u8; 16] {
        let mut s = block;
        add_round_key(&mut s, &self.rk[0]);
        for rk in &self.rk[1..10] {
            sub_bytes(&mut s);
            shift_rows(&mut s);
            mix_columns(&mut s);
            add_round_key(&mut s, rk);
        }
        sub_bytes(&mut s);
        shift_rows(&mut s);
        add_round_key(&mut s, &self.rk[10]);
        s
    }

    /// XORs `buf` in place with the AES-128-CTR keystream starting at `byte_offset`.
    pub(crate) fn apply_at(&self, byte_offset: u64, mut buf: &mut [u8]) {
        let mut pos = byte_offset;
        while !buf.is_empty() {
            let block_idx = u128::from(pos >> 4);
            let skip = (pos & 15) as usize;
            let ks = self.encrypt_block(block_idx.to_be_bytes());
            let take = (16 - skip).min(buf.len());
            for (dst, &k) in buf[..take].iter_mut().zip(&ks[skip..skip + take]) {
                *dst ^= k;
            }
            pos = pos.saturating_add(take as u64);
            buf = &mut buf[take..];
        }
    }
}

/// Parses a 32-character hexadecimal AES-128 key string (from Yandex's `encraw` `downloadInfo.key`).
pub(crate) fn parse_hex_key(hex: &str) -> Option<[u8; 16]> {
    let hex = hex.trim();
    if hex.len() != 32 {
        return None;
    }
    let mut key = [0u8; 16];
    for (i, slot) in key.iter_mut().enumerate() {
        *slot = u8::from_str_radix(&hex[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(key)
}

#[inline]
fn add_round_key(s: &mut [u8; 16], rk: &[u8; 16]) {
    for (dst, &k) in s.iter_mut().zip(rk) {
        *dst ^= k;
    }
}

#[inline]
fn sub_bytes(s: &mut [u8; 16]) {
    for b in s.iter_mut() {
        *b = SBOX[usize::from(*b)];
    }
}

#[inline]
fn shift_rows(s: &mut [u8; 16]) {
    // Column-major 4x4 state: row r, col c is at index c * 4 + r.
    // Row 1: shift left by 1
    let t = s[1];
    s[1] = s[5];
    s[5] = s[9];
    s[9] = s[13];
    s[13] = t;
    // Row 2: shift left by 2
    s.swap(2, 10);
    s.swap(6, 14);
    // Row 3: shift left by 3 (right by 1)
    let t = s[15];
    s[15] = s[11];
    s[11] = s[7];
    s[7] = s[3];
    s[3] = t;
}

#[inline]
fn xtime(x: u8) -> u8 {
    (x << 1) ^ (((x >> 7) & 1).wrapping_mul(0x1b))
}

#[inline]
fn mix_columns(s: &mut [u8; 16]) {
    for col in 0..4 {
        let i = col * 4;
        let (a0, a1, a2, a3) = (s[i], s[i + 1], s[i + 2], s[i + 3]);
        let xor = a0 ^ a1 ^ a2 ^ a3;
        s[i] = a0 ^ xor ^ xtime(a0 ^ a1);
        s[i + 1] = a1 ^ xor ^ xtime(a1 ^ a2);
        s[i + 2] = a2 ^ xor ^ xtime(a2 ^ a3);
        s[i + 3] = a3 ^ xor ^ xtime(a3 ^ a0);
    }
}
