//! Compatibility with instagram-private-api's seeded Chance.js device identity.
//! Chance.js copyright (c) 2015 Victor Quinn, MIT: https://github.com/chancejs/chancejs
use crate::DeviceProfile;
use std::io;

struct Mt {
    state: [u32; 624],
    index: usize,
}
impl Mt {
    fn new(seed: u32) -> Self {
        let mut state = [0u32; 624];
        state[0] = seed;
        for i in 1..624 {
            state[i] = 1_812_433_253u32
                .wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        Self { state, index: 624 }
    }
    fn next(&mut self) -> u32 {
        if self.index == 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
                self.state[i] = self.state[(i + 397) % 624]
                    ^ (y >> 1)
                    ^ if y & 1 != 0 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }
    fn character(&mut self, pool: &[u8]) -> char {
        let index = ((u64::from(self.next()) * pool.len() as u64) >> 32) as usize;
        char::from(pool[index])
    }
    fn string(&mut self, pool: &[u8], len: usize) -> String {
        (0..len).map(|_| self.character(pool)).collect()
    }
}

// MD5 is used solely to reproduce the old device selection, not for security.
fn md5_first_word(bytes: &[u8]) -> u32 {
    const S: [u32; 64] = [
        7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 7, 12, 17, 22, 5, 9, 14, 20, 5, 9, 14, 20, 5,
        9, 14, 20, 5, 9, 14, 20, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 4, 11, 16, 23, 6, 10,
        15, 21, 6, 10, 15, 21, 6, 10, 15, 21, 6, 10, 15, 21,
    ];
    const K: [u32; 64] = [
        0xd76aa478, 0xe8c7b756, 0x242070db, 0xc1bdceee, 0xf57c0faf, 0x4787c62a, 0xa8304613,
        0xfd469501, 0x698098d8, 0x8b44f7af, 0xffff5bb1, 0x895cd7be, 0x6b901122, 0xfd987193,
        0xa679438e, 0x49b40821, 0xf61e2562, 0xc040b340, 0x265e5a51, 0xe9b6c7aa, 0xd62f105d,
        0x02441453, 0xd8a1e681, 0xe7d3fbc8, 0x21e1cde6, 0xc33707d6, 0xf4d50d87, 0x455a14ed,
        0xa9e3e905, 0xfcefa3f8, 0x676f02d9, 0x8d2a4c8a, 0xfffa3942, 0x8771f681, 0x6d9d6122,
        0xfde5380c, 0xa4beea44, 0x4bdecfa9, 0xf6bb4b60, 0xbebfbc70, 0x289b7ec6, 0xeaa127fa,
        0xd4ef3085, 0x04881d05, 0xd9d4d039, 0xe6db99e5, 0x1fa27cf8, 0xc4ac5665, 0xf4292244,
        0x432aff97, 0xab9423a7, 0xfc93a039, 0x655b59c3, 0x8f0ccc92, 0xffeff47d, 0x85845dd1,
        0x6fa87e4f, 0xfe2ce6e0, 0xa3014314, 0x4e0811a1, 0xf7537e82, 0xbd3af235, 0x2ad7d2bb,
        0xeb86d391,
    ];
    let mut block = [0u8; 64];
    block[..bytes.len()].copy_from_slice(bytes);
    block[bytes.len()] = 0x80;
    block[56..].copy_from_slice(&((bytes.len() as u64) * 8).to_le_bytes());
    let mut words = [0u32; 16];
    for (i, chunk) in block.as_chunks::<4>().0.iter().enumerate() {
        words[i] = u32::from_le_bytes(*chunk);
    }
    let (mut a, mut b, mut c, mut d) = (0x67452301u32, 0xefcdab89u32, 0x98badcfeu32, 0x10325476u32);
    for i in 0..64 {
        let (f, g) = if i < 16 {
            ((b & c) | (!b & d), i)
        } else if i < 32 {
            ((d & b) | (!d & c), (5 * i + 1) % 16)
        } else if i < 48 {
            (b ^ c ^ d, (3 * i + 5) % 16)
        } else {
            (c ^ (b | !d), (7 * i) % 16)
        };
        let next = b.wrapping_add(
            a.wrapping_add(f)
                .wrapping_add(K[i])
                .wrapping_add(words[g])
                .rotate_left(S[i]),
        );
        a = d;
        d = c;
        c = b;
        b = next;
    }
    u32::from_be_bytes(0x67452301u32.wrapping_add(a).to_le_bytes())
}

/// Reproduce the old adapter's device identity without invoking Node.
pub fn legacy_device(account_id: &str) -> io::Result<DeviceProfile> {
    if account_id.is_empty()
        || account_id.len() > 30
        || !account_id.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "invalid account",
        ));
    }
    // JS bitwise operations coerce to signed 32 bits; arithmetic remains exact in f64 here.
    let mut hash = 0i64;
    for byte in account_id.bytes() {
        let value = hash as i32;
        hash =
            i64::from(byte) + i64::from(value.wrapping_shl(6)) + i64::from(value.wrapping_shl(16))
                - hash;
    }
    let seed = (hash.wrapping_mul(account_id.len() as i64)) as u32;
    let mut mt = Mt::new(seed);
    mt.next(); // The previous adapter selected a legacy sample device first.
    let device_id = format!("android-{}", mt.string(b"abcdef0123456789", 16));
    let pool = b"abcdef1234567890";
    let uuid = format!(
        "{}-{}-5{}-{}{}-{}",
        mt.string(pool, 8),
        mt.string(pool, 4),
        mt.string(pool, 3),
        mt.string(b"ab89", 1),
        mt.string(pool, 3),
        mt.string(pool, 12)
    );
    let devices = [
        "34/14; 480dpi; 1344x2992; Google/google; Pixel 8 Pro; husky; husky",
        "34/14; 420dpi; 1080x2400; Google/google; Pixel 8; shiba; shiba",
        "34/14; 420dpi; 1080x2340; samsung; SM-S921B; e1s; s5e9945",
    ];
    Ok(DeviceProfile {
        device_id,
        uuid,
        device_string: devices[(md5_first_word(account_id.as_bytes()) % 3) as usize].into(),
        language: "en_US".into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn matches_chance_js_for_synthetic_accounts() {
        for (id, device, uuid, index) in [
            (
                "1",
                "android-1d68d864f3a25dc6",
                "899662f2-a7f8-51dd-9fac-e4e962650240",
                1,
            ),
            (
                "123456",
                "android-fd3177648aed3dd4",
                "d95b0fd8-223d-5b96-a300-c54dca01e620",
                2,
            ),
            (
                "12345678901234567890",
                "android-af93e9b0bd5a603c",
                "188cedae-1b8f-5139-ac52-d4adf946cc66",
                1,
            ),
            (
                "999999999999999999999999999999",
                "android-444c47dab73e9018",
                "bac3dad4-9293-57ba-929f-0e7a88b08712",
                0,
            ),
        ] {
            let p = legacy_device(id).unwrap();
            assert_eq!(p.device_id, device);
            assert_eq!(p.uuid, uuid);
            assert_eq!(md5_first_word(id.as_bytes()) % 3, index);
        }
        assert!(legacy_device("../x").is_err());
    }
}
