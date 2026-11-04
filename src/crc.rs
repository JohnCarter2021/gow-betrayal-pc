//! CRC-32 (IEEE, polynomial 0xEDB88320), as used by PNG chunks and by the
//! game's palette keys (key = new PLTE + CRC32("PLTE" + new PLTE)).

pub fn crc32(data: &[u8]) -> u32 {
    // Table generated on first use; cheap enough for the small chunks here.
    let mut table = [0u32; 256];
    for i in 0..256u32 {
        let mut c = i;
        for _ in 0..8 {
            c = if c & 1 != 0 { 0xEDB88320 ^ (c >> 1) } else { c >> 1 };
        }
        table[i as usize] = c;
    }
    let mut crc = 0xFFFF_FFFFu32;
    for &b in data {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn known_vector() {
        // CRC-32("123456789") = 0xCBF43926, the standard check value.
        assert_eq!(crc32(b"123456789"), 0xCBF43926);
    }
}
