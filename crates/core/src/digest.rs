// SPDX-FileCopyrightText: 2026 Blackcat Informatics Inc.
// SPDX-License-Identifier: MIT OR Apache-2.0 OR MulanPSL-2.0

//! Dependency-free SHA-256 content identities, following FIPS 180-4 section 6.2.
//! These identify bytes; they do not authenticate their author.

use core::fmt;

/// A SHA-256 identity of exact source bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; 32]);

impl Digest {
    /// Hash a complete buffer without copying it.
    ///
    /// Equal to feeding the same bytes, split anywhere, through a
    /// [`DigestBuilder`].
    #[must_use]
    pub fn of(bytes: &[u8]) -> Self {
        let mut builder = DigestBuilder::new();
        builder.update(bytes);
        builder.finish()
    }

    /// The digest bytes in network order.
    #[must_use]
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("sha256:")?;
        for byte in self.0 {
            write!(f, "{byte:02x}")?;
        }
        Ok(())
    }
}

/// Incremental SHA-256 over bytes supplied in any number of pieces.
///
/// The finished identity equals [`Digest::of`] over the concatenation of every
/// [`Self::update`] call, wherever the input was split. At most one partial
/// 64-byte block is buffered; supplied bytes are never retained otherwise.
///
/// ```
/// use purrassist_core::digest::{Digest, DigestBuilder};
/// let mut builder = DigestBuilder::new();
/// builder.update(b"ab");
/// builder.update(b"c");
/// assert_eq!(builder.finish(), Digest::of(b"abc"));
/// ```
///
/// Hashing is infallible: there are no error cases. Inputs longer than
/// `2^61 - 1` bytes, which FIPS 180-4 excludes, encode a wrapped bit length
/// exactly as [`Digest::of`] does.
#[derive(Clone, Debug)]
pub struct DigestBuilder {
    state: [u32; 8],
    block: [u8; 64],
    buffered: usize,
    length: u64,
}

impl Default for DigestBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl DigestBuilder {
    /// Start hashing an empty message.
    ///
    /// ```
    /// use purrassist_core::digest::{Digest, DigestBuilder};
    /// assert_eq!(DigestBuilder::new().finish(), Digest::of(b""));
    /// ```
    #[must_use]
    pub const fn new() -> Self {
        Self {
            state: [
                0x6a09_e667,
                0xbb67_ae85,
                0x3c6e_f372,
                0xa54f_f53a,
                0x510e_527f,
                0x9b05_688c,
                0x1f83_d9ab,
                0x5be0_cd19,
            ],
            block: [0; 64],
            buffered: 0,
            length: 0,
        }
    }

    /// Append bytes to the message. This cannot fail and does not allocate.
    ///
    /// ```
    /// use purrassist_core::digest::{Digest, DigestBuilder};
    /// let mut builder = DigestBuilder::new();
    /// builder.update(&[]);
    /// builder.update(b"tea");
    /// assert_eq!(builder.finish(), Digest::of(b"tea"));
    /// ```
    pub fn update(&mut self, mut bytes: &[u8]) {
        self.length = self
            .length
            .wrapping_add(u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        if self.buffered > 0 {
            let take = bytes.len().min(64 - self.buffered);
            self.block[self.buffered..self.buffered + take].copy_from_slice(&bytes[..take]);
            self.buffered += take;
            bytes = &bytes[take..];
            if self.buffered < 64 {
                return;
            }
            compress(&mut self.state, &self.block);
            self.buffered = 0;
        }
        let (chunks, remainder) = bytes.as_chunks::<64>();
        for chunk in chunks {
            compress(&mut self.state, chunk);
        }
        self.block[..remainder.len()].copy_from_slice(remainder);
        self.buffered = remainder.len();
    }

    /// Pad the message and produce its identity.
    ///
    /// ```
    /// use purrassist_core::digest::DigestBuilder;
    /// let mut builder = DigestBuilder::new();
    /// builder.update(b"abc");
    /// assert_eq!(
    ///     builder.finish().to_string(),
    ///     "sha256:ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    /// );
    /// ```
    #[must_use]
    pub fn finish(mut self) -> Digest {
        let mut tail = [0_u8; 128];
        tail[..self.buffered].copy_from_slice(&self.block[..self.buffered]);
        tail[self.buffered] = 0x80;
        let end = if self.buffered < 56 { 64 } else { 128 };
        let bit_len = self.length.wrapping_mul(8);
        tail[end - 8..end].copy_from_slice(&bit_len.to_be_bytes());
        for chunk in tail[..end].as_chunks::<64>().0 {
            compress(&mut self.state, chunk);
        }
        let mut output = [0; 32];
        for (chunk, word) in output.as_chunks_mut::<4>().0.iter_mut().zip(self.state) {
            chunk.copy_from_slice(&word.to_be_bytes());
        }
        Digest(output)
    }
}

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
#[expect(
    clippy::many_single_char_names,
    reason = "FIPS 180-4 names its eight working registers a through h"
)]
fn compress(state: &mut [u32; 8], block: &[u8]) {
    let mut schedule = [0_u32; 64];
    for (word, bytes) in schedule[..16].iter_mut().zip(block.as_chunks::<4>().0) {
        *word = u32::from_be_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]);
    }
    for index in 16..64 {
        let left = schedule[index - 15];
        let right = schedule[index - 2];
        let small_zero = left.rotate_right(7) ^ left.rotate_right(18) ^ (left >> 3);
        let small_one = right.rotate_right(17) ^ right.rotate_right(19) ^ (right >> 10);
        schedule[index] = schedule[index - 16]
            .wrapping_add(small_zero)
            .wrapping_add(schedule[index - 7])
            .wrapping_add(small_one);
    }
    let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = *state;
    for (constant, word) in K.into_iter().zip(schedule) {
        let big_one = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
        let choice = (e & f) ^ (!e & g);
        let first = h
            .wrapping_add(big_one)
            .wrapping_add(choice)
            .wrapping_add(constant)
            .wrapping_add(word);
        let big_zero = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
        let majority = (a & b) ^ (a & c) ^ (b & c);
        let second = big_zero.wrapping_add(majority);
        h = g;
        g = f;
        f = e;
        e = d.wrapping_add(first);
        d = c;
        c = b;
        b = a;
        a = first.wrapping_add(second);
    }
    for (word, addition) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
        *word = word.wrapping_add(addition);
    }
}

#[cfg(test)]
mod tests {
    use super::{Digest, DigestBuilder};

    #[test]
    fn digest_builder_equals_of_for_every_split() {
        let long: Vec<u8> = (0..200_u8).map(|byte| byte.wrapping_mul(37)).collect();
        for input in [&b""[..], b"abc", &long] {
            let expected = Digest::of(input);
            for split in 0..=input.len() {
                let mut builder = DigestBuilder::new();
                builder.update(&input[..split]);
                builder.update(&input[split..]);
                assert_eq!(
                    builder.finish(),
                    expected,
                    "split at {split} of {}",
                    input.len()
                );
            }
        }
    }

    #[test]
    fn digest_builder_equals_of_for_three_way_splits() {
        let input: Vec<u8> = (0..200_u8).map(|byte| byte ^ 0x5a).collect();
        let expected = Digest::of(&input);
        for first in (0..=input.len()).step_by(7) {
            for second in first..=input.len() {
                let mut builder = DigestBuilder::new();
                builder.update(&input[..first]);
                builder.update(&input[first..second]);
                builder.update(&input[second..]);
                assert_eq!(builder.finish(), expected, "splits at {first} and {second}");
            }
        }
        let mut bytewise = DigestBuilder::new();
        for byte in &input {
            bytewise.update(core::slice::from_ref(byte));
        }
        assert_eq!(bytewise.finish(), expected);
    }

    #[test]
    fn standard_vectors() {
        for (input, expected) in [
            (
                "",
                "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
            ),
            (
                "abc",
                "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
            ),
            (
                "abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
                "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
            ),
        ] {
            assert_eq!(
                Digest::of(input.as_bytes()).to_string(),
                format!("sha256:{expected}")
            );
        }
        assert_eq!(
            Digest::of(&vec![b'a'; 1_000_000]).to_string(),
            "sha256:cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
        );
    }
}
