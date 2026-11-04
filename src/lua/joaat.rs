pub fn joaat(text: &str) -> i32 {
    joaat_bytes(text.as_bytes())
}

pub fn joaat_bytes(bytes: &[u8]) -> i32 {
    let mut h: u32 = 0;
    for &b in bytes {
        let c = if b.is_ascii_uppercase() { b + 32 } else { b };
        // The runtime adds the bytes as signed chars (sign-extended above 0x7F).
        h = h.wrapping_add(c as i8 as i32 as u32);
        h = h.wrapping_add(h << 10);
        h ^= h >> 6;
    }
    h = h.wrapping_add(h << 3);
    h ^= h >> 11;
    h = h.wrapping_add(h << 15);
    h as i32
}

pub fn hex(hash: i32) -> String {
    format!("0x{:08X}", hash as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lua::lexer::decode_escapes;

    #[test]
    fn known_hashes() {
        assert_eq!(joaat("adder"), -1216765807);
        assert_eq!(joaat("ADDER"), -1216765807);
        assert_eq!(hex(joaat("adder")), "0xB779A091");
        assert_eq!(joaat("WEAPON_PISTOL"), 453432689);
        assert_eq!(joaat("prop_bench_01a"), 1805980844);
        assert_eq!(joaat(""), 0);
    }

    #[test]
    fn hash_literals_as_the_runtime_computes_them() {
        let lit = |raw: &str| joaat_bytes(&decode_escapes(raw));
        assert_eq!(lit("é"), 26298542);
        assert_eq!(lit("a\\tb\\x41\\65"), -756207222);
        assert_eq!(lit("\\z\n   adder"), -1216765807);
        assert_eq!(lit("a\\u{48}b"), 1267065503);
        assert_eq!(lit("\\u{7FFFFFFF}"), -619256725);
        assert_eq!(lit("a\\\nb"), -622335608);
        assert_eq!(lit("it's"), 1943295684);
        assert_eq!(lit("say \\\"hi\\\""), 1876969269);
        assert_eq!(lit("\\0\\255"), -1369070444);
    }
}
