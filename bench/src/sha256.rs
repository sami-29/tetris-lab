//! SHA-256, and nothing else: the artifact's identity, computed locally.
//!
//! §9.6 asks a Reference row to label itself with "the wasm hash". The hash has to
//! cover **the bytes the page executes**, and `bench` may not take a dependency
//! (`run/log.md`: `bench` is a bin that depends on `engine` only), so the choice is
//! a second implementation of a hash in this crate or a hash computed somewhere
//! else and trusted. Trusting a sidecar file is the worse of the two: the label
//! would then name whatever the last build script said, not the artifact sitting in
//! `web/pkg/`.
//!
//! FNV-1a-64 was rejected for this one field on purpose (§6.1 justifies it *only*
//! for the Dataset: "compared for equality, never defended against an adversary").
//! A shipped artifact's identity should be the conventional one, so a reader can
//! check it with `sha256sum` and get the same 64 characters.
//!
//! The implementation is the standard one — 32-bit words, 64 rounds, big-endian
//! length — and it is pinned by the published vectors below, so a transcription
//! error cannot reach a committed row.

/// The 64 constants: the first 32 bits of the fractional part of each prime's cube
/// root (§FIPS 180-4 §4.2.2), written out rather than computed.
const K: [u32; 64] = [
    0x428a_2f98,
    0x7137_4491,
    0xb5c0_fbcf,
    0xe9b5_dba5,
    0x3956_c25b,
    0x59f1_11f1,
    0x923f_82a4,
    0xab1c_5ed5,
    0xd807_aa98,
    0x1283_5b01,
    0x2431_85be,
    0x550c_7dc3,
    0x72be_5d74,
    0x80de_b1fe,
    0x9bdc_06a7,
    0xc19b_f174,
    0xe49b_69c1,
    0xefbe_4786,
    0x0fc1_9dc6,
    0x240c_a1cc,
    0x2de9_2c6f,
    0x4a74_84aa,
    0x5cb0_a9dc,
    0x76f9_88da,
    0x983e_5152,
    0xa831_c66d,
    0xb003_27c8,
    0xbf59_7fc7,
    0xc6e0_0bf3,
    0xd5a7_9147,
    0x06ca_6351,
    0x1429_2967,
    0x27b7_0a85,
    0x2e1b_2138,
    0x4d2c_6dfc,
    0x5338_0d13,
    0x650a_7354,
    0x766a_0abb,
    0x81c2_c92e,
    0x9272_2c85,
    0xa2bf_e8a1,
    0xa81a_664b,
    0xc24b_8b70,
    0xc76c_51a3,
    0xd192_e819,
    0xd699_0624,
    0xf40e_3585,
    0x106a_a070,
    0x19a4_c116,
    0x1e37_6c08,
    0x2748_774c,
    0x34b0_bcb5,
    0x391c_0cb3,
    0x4ed8_aa4a,
    0x5b9c_ca4f,
    0x682e_6ff3,
    0x748f_82ee,
    0x78a5_636f,
    0x84c8_7814,
    0x8cc7_0208,
    0x90be_fffa,
    0xa450_6ceb,
    0xbef9_a3f7,
    0xc671_78f2,
];

const INITIAL: [u32; 8] = [
    0x6a09_e667,
    0xbb67_ae85,
    0x3c6e_f372,
    0xa54f_f53a,
    0x510e_527f,
    0x9b05_688c,
    0x1f83_d9ab,
    0x5be0_cd19,
];

/// The digest of `bytes`, 32 bytes, in the order FIPS 180-4 prints them.
pub fn digest(bytes: &[u8]) -> [u8; 32] {
    let mut state = INITIAL;

    // Padding: 0x80, then zeros, then the message's bit length as a big-endian u64,
    // so the whole thing is a multiple of 64 bytes.
    let bit_len = (bytes.len() as u64).wrapping_mul(8);
    let mut message = bytes.to_vec();
    message.push(0x80);
    while message.len() % 64 != 56 {
        message.push(0);
    }
    message.extend_from_slice(&bit_len.to_be_bytes());

    for block in message.as_chunks::<64>().0 {
        let mut words = [0u32; 64];
        for (index, word) in words.iter_mut().take(16).enumerate() {
            let at = index * 4;
            *word = u32::from_be_bytes([block[at], block[at + 1], block[at + 2], block[at + 3]]);
        }
        for index in 16..64 {
            let s0 = words[index - 15].rotate_right(7)
                ^ words[index - 15].rotate_right(18)
                ^ (words[index - 15] >> 3);
            let s1 = words[index - 2].rotate_right(17)
                ^ words[index - 2].rotate_right(19)
                ^ (words[index - 2] >> 10);
            words[index] = words[index - 16]
                .wrapping_add(s0)
                .wrapping_add(words[index - 7])
                .wrapping_add(s1);
        }

        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for index in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choice = (e & f) ^ (!e & g);
            let temp1 = h
                .wrapping_add(s1)
                .wrapping_add(choice)
                .wrapping_add(K[index])
                .wrapping_add(words[index]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let temp2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(temp1);
            d = c;
            c = b;
            b = a;
            a = temp1.wrapping_add(temp2);
        }
        for (slot, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *slot = slot.wrapping_add(value);
        }
    }

    let mut out = [0u8; 32];
    for (index, word) in state.iter().enumerate() {
        out[index * 4..index * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

/// 64 lowercase hex digits: the form a row stores and a reader compares.
pub fn hex(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The digest of a file's bytes, and how many there were.
pub fn file(path: &std::path::Path) -> std::io::Result<(String, u64)> {
    let bytes = std::fs::read(path)?;
    Ok((hex(&digest(&bytes)), bytes.len() as u64))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_published_vectors_come_out_whole() {
        // FIPS 180-4 / every reference implementation's first three vectors.
        assert_eq!(
            hex(&digest(b"")),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            hex(&digest(b"abc")),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(
            hex(&digest(
                b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"
            )),
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1"
        );
    }

    #[test]
    fn a_length_that_straddles_the_padding_block_is_hashed_correctly() {
        // 55, 56 and 64 bytes are the three interesting cases: 56 forces a second
        // padding block, 64 needs no zero padding at all.
        assert_eq!(digest(&[b'a'; 55]).len(), 32);
        assert_eq!(digest(&[b'a'; 56]).len(), 32);
        assert_eq!(digest(&[b'a'; 64]).len(), 32);
        // 1,000,000 'a's is the classic long vector; it also proves the loop is not
        // silently truncating the message.
        assert_eq!(
            hex(&digest(&vec![b'a'; 1_000_000])),
            "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }

    #[test]
    fn one_flipped_bit_moves_the_digest() {
        let mut other = b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq".to_vec();
        other[10] ^= 1;
        assert_ne!(
            digest(&other),
            digest(b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")
        );
    }
}
