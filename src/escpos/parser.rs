use crate::escpos::commands::{EscPosCommand, Font, Justification};
use anyhow::Result;

/// Streaming ESC/POS parser.
///
/// Every command is consumed with its exact byte length, so parameter bytes never
/// leak into the printed text (which is what produced the "□" boxes before).
pub struct EscPosParser {
    buffer: Vec<u8>,
}

/// Returns `Ok(None)` from the enclosing function when fewer than `$n` bytes are available.
macro_rules! need {
    ($data:expr, $n:expr) => {
        if $data.len() < ($n) as usize {
            return Ok(None);
        }
    };
}

fn u16le(lo: u8, hi: u8) -> u16 {
    lo as u16 | ((hi as u16) << 8)
}

impl EscPosParser {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn parse_stream(&mut self, data: &[u8]) -> Result<Vec<EscPosCommand>> {
        self.buffer.extend_from_slice(data);
        let mut commands = Vec::new();
        let mut i = 0;

        while i < self.buffer.len() {
            let b = self.buffer[i];
            let parsed = match b {
                0x0A => Some((EscPosCommand::NewLine, 1)),
                0x0D => Some((EscPosCommand::CarriageReturn, 1)),
                0x09 => Some((EscPosCommand::HorizontalTab, 1)),
                0x1B => match Self::parse_esc(&self.buffer[i..])? {
                    Some(r) => Some(r),
                    None => break, // incomplete, wait for more data
                },
                0x1D => match Self::parse_gs(&self.buffer[i..])? {
                    Some(r) => Some(r),
                    None => break,
                },
                0x1C => match Self::parse_fs(&self.buffer[i..])? {
                    Some(r) => Some(r),
                    None => break,
                },
                0x10 => match Self::parse_dle(&self.buffer[i..])? {
                    Some(r) => Some(r),
                    None => break,
                },
                0x00..=0x1F => {
                    // Other control codes (FF, NUL, BEL, ...) produce no output in standard mode
                    Some((EscPosCommand::Ignored(vec![b]), 1))
                }
                _ => {
                    let start = i;
                    let mut end = i;
                    while end < self.buffer.len() && self.buffer[end] >= 0x20 {
                        end += 1;
                    }
                    let bytes = self.buffer[start..end].to_vec();
                    Some((EscPosCommand::Text(bytes), end - start))
                }
            };

            if let Some((cmd, consumed)) = parsed {
                commands.push(cmd);
                i += consumed;
            }
        }

        if i > 0 {
            self.buffer.drain(0..i);
        }

        Ok(commands)
    }

    /// ESC (0x1B) commands. Returns (command, bytes consumed) or None if incomplete.
    fn parse_esc(data: &[u8]) -> Result<Option<(EscPosCommand, usize)>> {
        need!(data, 2);
        let ign = |n: usize| Ok(Some((EscPosCommand::Ignored(data[..n].to_vec()), n)));

        match data[1] {
            b'@' => Ok(Some((EscPosCommand::InitializePrinter, 2))),
            b'!' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetPrintMode(data[2]), 3)))
            }
            b' ' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetCharSpacing(data[2]), 3)))
            }
            b'$' => {
                need!(data, 4);
                Ok(Some((EscPosCommand::AbsolutePosition(u16le(data[2], data[3])), 4)))
            }
            b'\\' => {
                need!(data, 4);
                Ok(Some((EscPosCommand::RelativePosition(u16le(data[2], data[3]) as i16), 4)))
            }
            b'-' => {
                need!(data, 3);
                let n = match data[2] {
                    0 | b'0' => 0,
                    1 | b'1' => 1,
                    2 | b'2' => 2,
                    _ => 0,
                };
                Ok(Some((EscPosCommand::SetUnderline(n), 3)))
            }
            b'2' => Ok(Some((EscPosCommand::SetLineSpacing(None), 2))),
            b'3' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetLineSpacing(Some(data[2])), 3)))
            }
            b'4' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetItalic(data[2] != 0), 3)))
            }
            b'5' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetItalic(data[2] == 0), 3)))
            }
            b'E' | b'G' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetEmphasis(data[2] & 1 == 1), 3)))
            }
            b'F' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetEmphasis(data[2] == 0), 3)))
            }
            b'M' => {
                need!(data, 3);
                let font = match data[2] {
                    1 | b'1' => Font::FontB,
                    2 | b'2' => Font::FontC,
                    _ => Font::FontA,
                };
                Ok(Some((EscPosCommand::SetFont(font), 3)))
            }
            b'a' => {
                need!(data, 3);
                let j = match data[2] {
                    1 | b'1' => Justification::Center,
                    2 | b'2' => Justification::Right,
                    _ => Justification::Left,
                };
                Ok(Some((EscPosCommand::SetJustification(j), 3)))
            }
            b'{' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetUpsideDown(data[2] & 1 == 1), 3)))
            }
            b't' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetCodepage(data[2]), 3)))
            }
            b'J' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::FeedDots(data[2]), 3)))
            }
            b'd' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::FeedLines(data[2]), 3)))
            }
            b'D' => {
                // ESC D n1 ... nk NUL
                match data[2..].iter().position(|&b| b == 0) {
                    Some(pos) => {
                        let stops = data[2..2 + pos].to_vec();
                        Ok(Some((EscPosCommand::SetTabStops(stops), 2 + pos + 1)))
                    }
                    None => {
                        if data.len() > 2 + 33 {
                            ign(2) // malformed, give up
                        } else {
                            Ok(None)
                        }
                    }
                }
            }
            b'*' => {
                need!(data, 5);
                let m = data[2];
                let n = u16le(data[3], data[4]);
                let bytes_per_col: usize = if m == 32 || m == 33 { 3 } else { 1 };
                let total = 5 + bytes_per_col * n as usize;
                need!(data, total);
                Ok(Some((
                    EscPosCommand::BitImage { mode: m, width: n, data: data[5..total].to_vec() },
                    total,
                )))
            }
            b'i' | b'm' => Ok(Some((EscPosCommand::CutPaper, 2))),
            b'p' => {
                need!(data, 5);
                ign(5) // cash drawer pulse
            }
            b'c' => {
                need!(data, 4);
                ign(4)
            }
            b'&' => {
                // ESC & y c1 c2 [x d1..d(y*x)]... — user-defined characters
                need!(data, 5);
                let y = data[2] as usize;
                let (c1, c2) = (data[3], data[4]);
                let mut pos = 5;
                if c2 >= c1 {
                    for _ in c1..=c2 {
                        need!(data, pos + 1);
                        let x = data[pos] as usize;
                        pos += 1 + x * y;
                    }
                }
                need!(data, pos);
                ign(pos)
            }
            b'W' => {
                need!(data, 10);
                ign(10)
            }
            b'%' | b'=' | b'?' | b'R' | b'T' | b'U' | b'V' | b'e' | b'r' | b'u' => {
                need!(data, 3);
                ign(3)
            }
            b'<' | b'L' | b'S' | b'v' | 0x0C | 0x0E | 0x14 => ign(2),
            _ => Ok(Some((EscPosCommand::Unknown(data[..2].to_vec()), 2))),
        }
    }

    /// GS (0x1D) commands.
    fn parse_gs(data: &[u8]) -> Result<Option<(EscPosCommand, usize)>> {
        need!(data, 2);
        let ign = |n: usize| Ok(Some((EscPosCommand::Ignored(data[..n].to_vec()), n)));

        match data[1] {
            b'!' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetCharSize(data[2]), 3)))
            }
            b'B' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetReverse(data[2] & 1 == 1), 3)))
            }
            b'L' => {
                need!(data, 4);
                Ok(Some((EscPosCommand::SetLeftMargin(u16le(data[2], data[3])), 4)))
            }
            b'W' => {
                need!(data, 4);
                Ok(Some((EscPosCommand::SetPrintAreaWidth(u16le(data[2], data[3])), 4)))
            }
            b'h' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetBarcodeHeight(data[2]), 3)))
            }
            b'w' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetBarcodeWidth(data[2]), 3)))
            }
            b'H' => {
                need!(data, 3);
                Ok(Some((EscPosCommand::SetHriPosition(data[2] & 0x0F), 3)))
            }
            b'V' => {
                need!(data, 3);
                match data[2] {
                    65 | 66 | 97 | 98 | 103 | 104 => {
                        need!(data, 4);
                        Ok(Some((EscPosCommand::CutPaper, 4)))
                    }
                    _ => Ok(Some((EscPosCommand::CutPaper, 3))),
                }
            }
            b'v' => {
                // GS v 0 m xL xH yL yH d1...dk
                need!(data, 8);
                let mode = data[3];
                let width_bytes = u16le(data[4], data[5]);
                let height = u16le(data[6], data[7]);
                let total = 8 + width_bytes as usize * height as usize;
                need!(data, total);
                Ok(Some((
                    EscPosCommand::PrintRasterImage {
                        mode,
                        width_bytes,
                        height,
                        data: data[8..total].to_vec(),
                    },
                    total,
                )))
            }
            b'k' => {
                need!(data, 4);
                let m = data[2];
                if m <= 6 {
                    // data terminated by NUL
                    match data[3..].iter().position(|&b| b == 0) {
                        Some(pos) => Ok(Some((
                            EscPosCommand::Barcode { system: m, data: data[3..3 + pos].to_vec() },
                            3 + pos + 1,
                        ))),
                        None => {
                            if data.len() > 3 + 255 {
                                ign(3)
                            } else {
                                Ok(None)
                            }
                        }
                    }
                } else {
                    let n = data[3] as usize;
                    let total = 4 + n;
                    need!(data, total);
                    Ok(Some((
                        EscPosCommand::Barcode { system: m, data: data[4..total].to_vec() },
                        total,
                    )))
                }
            }
            b'(' => {
                // GS ( fn pL pH ...
                need!(data, 5);
                let len = u16le(data[3], data[4]) as usize;
                let total = 5 + len;
                need!(data, total);
                if data[2] == b'k' && len >= 2 && data[5] == 49 {
                    // QR Code (cn = 49)
                    let func = data[6];
                    let cmd = match func {
                        67 if len >= 3 => EscPosCommand::QrModuleSize(data[7]),
                        69 if len >= 3 => EscPosCommand::QrErrorCorrection(data[7]),
                        80 if len >= 3 => EscPosCommand::QrStoreData(data[8..total].to_vec()),
                        81 => EscPosCommand::QrPrint,
                        _ => EscPosCommand::Ignored(data[..total].to_vec()),
                    };
                    return Ok(Some((cmd, total)));
                }
                ign(total)
            }
            b'8' => {
                // GS 8 L p1 p2 p3 p4 ...
                need!(data, 7);
                let len = data[3] as usize
                    | (data[4] as usize) << 8
                    | (data[5] as usize) << 16
                    | (data[6] as usize) << 24;
                need!(data, 7 + len);
                ign(7 + len)
            }
            b'*' => {
                need!(data, 4);
                let total = 4 + data[2] as usize * data[3] as usize * 8;
                need!(data, total);
                ign(total)
            }
            b'P' | b'$' | b'\\' => {
                need!(data, 4);
                ign(4)
            }
            b'^' => {
                need!(data, 5);
                ign(5)
            }
            b'/' | b'a' | b'b' | b'f' | b'r' | b'I' | b'T' | b'E' | b'j' => {
                need!(data, 3);
                ign(3)
            }
            b':' | b'Z' => ign(2),
            _ => Ok(Some((EscPosCommand::Unknown(data[..2].to_vec()), 2))),
        }
    }

    /// FS (0x1C) commands — mostly Kanji / NV images, no effect on Latin text.
    fn parse_fs(data: &[u8]) -> Result<Option<(EscPosCommand, usize)>> {
        need!(data, 2);
        let ign = |n: usize| Ok(Some((EscPosCommand::Ignored(data[..n].to_vec()), n)));
        match data[1] {
            b'&' | b'.' => ign(2),
            b'!' | b'-' | b'C' | b'W' => {
                need!(data, 3);
                ign(3)
            }
            b'S' | b'p' => {
                need!(data, 4);
                ign(4)
            }
            b'(' => {
                need!(data, 5);
                let total = 5 + u16le(data[3], data[4]) as usize;
                need!(data, total);
                ign(total)
            }
            _ => ign(2),
        }
    }

    /// DLE (0x10) real-time commands — no visual effect.
    fn parse_dle(data: &[u8]) -> Result<Option<(EscPosCommand, usize)>> {
        need!(data, 2);
        let ign = |n: usize| Ok(Some((EscPosCommand::Ignored(data[..n].to_vec()), n)));
        match data[1] {
            0x04 | 0x05 => {
                need!(data, 3);
                ign(3)
            }
            0x14 => {
                need!(data, 5);
                ign(5)
            }
            _ => ign(1),
        }
    }
}

impl Default for EscPosParser {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for EscPosParser {
    fn clone(&self) -> Self {
        Self { buffer: self.buffer.clone() }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gs_char_size_does_not_leak_into_text() {
        let mut p = EscPosParser::new();
        let cmds = p.parse_stream(b"\x1d\x21\x11DELIVERY\n").unwrap();
        assert!(matches!(cmds[0], EscPosCommand::SetCharSize(0x11)));
        assert!(matches!(&cmds[1], EscPosCommand::Text(t) if t == b"DELIVERY"));
    }

    #[test]
    fn split_command_waits_for_more_data() {
        let mut p = EscPosParser::new();
        assert!(p.parse_stream(b"\x1d").unwrap().is_empty());
        let cmds = p.parse_stream(b"\x21\x00").unwrap();
        assert!(matches!(cmds[0], EscPosCommand::SetCharSize(0)));
    }

    #[test]
    fn qr_store_data() {
        let mut p = EscPosParser::new();
        // GS ( k pL pH 49 80 48 "abc"
        let cmds = p.parse_stream(b"\x1d(k\x06\x00\x31\x50\x30abc").unwrap();
        assert!(matches!(&cmds[0], EscPosCommand::QrStoreData(d) if d == b"abc"));
    }
}
