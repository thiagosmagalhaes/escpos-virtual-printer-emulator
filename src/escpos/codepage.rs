//! Single-byte code pages used by ESC/POS printers (ESC t n).
//!
//! A real printer never sees "characters", only bytes. The byte is looked up in the
//! active code page table, so the emulator must do the same to show accents exactly
//! as the printer would (including the garbage a real printer prints when the
//! application sends UTF-8 or the wrong code page).

const CP437_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜ¢£¥₧ƒáíóúñÑªº¿⌐¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{A0}";
const CP850_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈıÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{AD}±‗¾¶§÷¸°¨·¹³²■\u{A0}";
const CP858_HIGH: &str = "ÇüéâäàåçêëèïîìÄÅÉæÆôöòûùÿÖÜø£Ø×ƒáíóúñÑªº¿®¬½¼¡«»░▒▓│┤ÁÂÀ©╣║╗╝¢¥┐└┴┬├─┼ãÃ╚╔╩╦╠═╬¤ðÐÊËÈ€ÍÎÏ┘┌█▄¦Ì▀ÓßÔÒõÕµþÞÚÛÙýÝ¯´\u{AD}±‗¾¶§÷¸°¨·¹³²■\u{A0}";
const CP860_HIGH: &str = "ÇüéâãàÁçêÊèÍÔìÃÂÉÀÈôõòÚùÌÕÜ¢£Ù₧ÓáíóúñÑªº¿Ò¬½¼¡«»░▒▓│┤╡╢╖╕╣║╗╝╜╛┐└┴┬├─┼╞╟╚╔╩╦╠═╬╧╨╤╥╙╘╒╓╫╪┘┌█▄▌▐▀αßΓπΣσµτΦΘΩδ∞φε∩≡±≥≤⌠⌡÷≈°∙·√ⁿ²■\u{A0}";
/// 0x80..0x9F of Windows-1252 (0xA0..0xFF are identical to Latin-1)
const CP1252_80_9F: &str = "€\u{FFFD}‚ƒ„…†‡ˆ‰Š‹Œ\u{FFFD}Ž\u{FFFD}\u{FFFD}‘’“”•–—˜™š›œ\u{FFFD}žŸ";

/// Code pages offered in the UI: (ESC t value, label)
pub const SUPPORTED: &[(u8, &str)] = &[
    (0, "PC437 (EUA)"),
    (2, "PC850 (Multilíngue)"),
    (3, "PC860 (Português)"),
    (19, "PC858 (Euro)"),
    (16, "WPC1252 (Windows)"),
];

pub fn name(cp: u8) -> &'static str {
    SUPPORTED
        .iter()
        .find(|(n, _)| *n == cp)
        .map(|(_, l)| *l)
        .unwrap_or("PC437 (fallback)")
}

/// Decode one byte with the given ESC t code page.
pub fn decode(cp: u8, b: u8) -> char {
    if b < 0x80 {
        return b as char;
    }
    let idx = (b - 0x80) as usize;
    let table = match cp {
        2 => CP850_HIGH,
        3 => CP860_HIGH,
        19 => CP858_HIGH,
        16 => {
            return if b >= 0xA0 {
                b as char
            } else {
                CP1252_80_9F.chars().nth(idx).unwrap_or('?')
            };
        }
        _ => CP437_HIGH,
    };
    table.chars().nth(idx).unwrap_or('?')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_have_128_entries() {
        for t in [CP437_HIGH, CP850_HIGH, CP858_HIGH, CP860_HIGH] {
            assert_eq!(t.chars().count(), 128);
        }
        assert_eq!(CP1252_80_9F.chars().count(), 32);
    }

    #[test]
    fn portuguese_accents() {
        assert_eq!(decode(2, 0xC7), 'Ã');
        assert_eq!(decode(2, 0x80), 'Ç');
        assert_eq!(decode(3, 0x84), 'ã');
        assert_eq!(decode(16, 0xC7), 'Ç');
        assert_eq!(decode(16, 0xEA), 'ê');
    }
}
