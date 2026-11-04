//! Jenkins one-at-a-time hash, the GTA / RDR `GetHashKey` (and CfxLua's
//! `` `name` `` literals): lower-cased input, signed 32-bit result.

pub fn joaat(text: &str) -> i32 {
    let mut h: u32 = 0;
    for b in text.bytes() {
        h = h.wrapping_add(u32::from(b.to_ascii_lowercase()));
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h = h.wrapping_add(h << 15);
    h as i32
}

/// `0x` + 8 upper-case hex digits of the unsigned hash.
pub fn hex(hash: i32) -> String {
    format!("0x{:08X}", hash as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_hashes() {
        assert_eq!(joaat("adder"), -1216765807);
        assert_eq!(joaat("ADDER"), -1216765807);
        assert_eq!(hex(joaat("adder")), "0xB779A091");
        assert_eq!(joaat("WEAPON_PISTOL"), 453432689);
    }
}
