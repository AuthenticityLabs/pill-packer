//! Portable CRC32C (Castagnoli) for integrity checks on already encoded bytes.
const fn tables() -> [[u32; 256]; 8] {
    let mut table = [[0; 256]; 8];
    let mut i = 0;
    while i < 256 {
        let mut crc = i as u32;
        let mut bit = 0;
        while bit < 8 {
            crc = (crc >> 1) ^ if crc & 1 != 0 { 0x82f63b78 } else { 0 };
            bit += 1;
        }
        table[0][i] = crc;
        i += 1;
    }
    let mut n = 1;
    while n < 8 {
        let mut i = 0;
        while i < 256 {
            let prior = table[n - 1][i];
            table[n][i] = (prior >> 8) ^ table[0][(prior & 255) as usize];
            i += 1;
        }
        n += 1;
    }
    table
}
static TABLE: [[u32; 256]; 8] = tables();
pub fn crc32c(mut bytes: &[u8]) -> u32 {
    let mut crc = !0u32;
    while bytes.len() >= 8 {
        let a = crc ^ u32::from_le_bytes(bytes[..4].try_into().unwrap());
        crc = TABLE[7][(a & 255) as usize]
            ^ TABLE[6][((a >> 8) & 255) as usize]
            ^ TABLE[5][((a >> 16) & 255) as usize]
            ^ TABLE[4][(a >> 24) as usize]
            ^ TABLE[3][bytes[4] as usize]
            ^ TABLE[2][bytes[5] as usize]
            ^ TABLE[1][bytes[6] as usize]
            ^ TABLE[0][bytes[7] as usize];
        bytes = &bytes[8..];
    }
    for &b in bytes {
        crc = (crc >> 8) ^ TABLE[0][((crc as u8) ^ b) as usize];
    }
    !crc
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn vectors() {
        assert_eq!(crc32c(b""), 0);
        assert_eq!(crc32c(b"123456789"), 0xe3069283);
        for n in 0..200 {
            let data: Vec<u8> = (0..n).map(|i| i as u8).collect();
            let mut crc = !0u32;
            for &b in &data {
                crc ^= b as u32;
                for _ in 0..8 {
                    crc = (crc >> 1) ^ if crc & 1 != 0 { 0x82f63b78 } else { 0 };
                }
            }
            assert_eq!(crc32c(&data), !crc);
        }
    }
}
