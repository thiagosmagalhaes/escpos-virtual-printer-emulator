//! Dot-accurate thermal printer model.
//!
//! Instead of drawing "text with a font" in the GUI, this module behaves like the
//! printer firmware: it keeps a line buffer of character cells measured in dots,
//! wraps when the print area is full, applies justification per line, and burns the
//! result into a 1-bit page bitmap at the printer resolution (203 dpi = 8 dots/mm).
//! The GUI only displays that bitmap, so what you see is what the paper would show.

use crate::escpos::codepage;
use crate::escpos::commands::{EscPosCommand, Font, Justification};
use crate::escpos::font::{self, DotBitmap};
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicU64, Ordering};

pub const DOTS_PER_MM: f32 = 8.0; // 203 dpi
const DEFAULT_LINE_SPACING: u32 = 30; // ~1/6 inch, ESC 2
const MAX_PAGE_HEIGHT: u32 = 200_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum PaperWidth {
    /// 58mm paper, 48mm printable = 384 dots (32 columns Font A)
    Width58mm,
    /// 80mm paper, 72mm printable = 576 dots (48 columns Font A) — Epson, Elgin, Bematech...
    Width80mm,
    /// 80mm paper configured with 512 dots (42 columns Font A) — common on generic printers
    Width80mm42,
}

impl PaperWidth {
    /// Printable width in dots
    pub fn get_width_dots(&self) -> u32 {
        match self {
            PaperWidth::Width58mm => 384,
            PaperWidth::Width80mm => 576,
            PaperWidth::Width80mm42 => 512,
        }
    }
    pub fn paper_mm(&self) -> u32 {
        match self {
            PaperWidth::Width58mm => 58,
            PaperWidth::Width80mm | PaperWidth::Width80mm42 => 80,
        }
    }
    /// Physical paper width in dots
    pub fn paper_dots(&self) -> u32 {
        (self.paper_mm() as f32 * DOTS_PER_MM) as u32
    }
    pub fn columns(&self, font: &Font) -> u32 {
        self.get_width_dots() / font::cell_size(font).0
    }
    pub fn label(&self) -> String {
        format!(
            "{}mm · {} dots · {} col",
            self.paper_mm(),
            self.get_width_dots(),
            self.columns(&Font::FontA)
        )
    }
}

/// A printed receipt (between two cuts), 1 byte per dot.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Page {
    pub id: u64,
    pub revision: u64,
    pub width: u32,
    pub height: u32,
    pub dots: Vec<u8>,
    /// true once the cutter has separated this page
    pub cut: bool,
}

static PAGE_IDS: AtomicU64 = AtomicU64::new(1);

impl Page {
    fn new(width: u32) -> Self {
        Self {
            id: PAGE_IDS.fetch_add(1, Ordering::Relaxed),
            revision: 0,
            width,
            height: 0,
            dots: Vec::new(),
            cut: false,
        }
    }
    fn ensure_height(&mut self, h: u32) {
        let h = h.min(MAX_PAGE_HEIGHT);
        if h > self.height {
            self.dots.resize((self.width * h) as usize, 0);
            self.height = h;
        }
    }
    fn blit(&mut self, bmp: &DotBitmap, x: i32, y: u32) {
        self.ensure_height(y + bmp.h);
        for by in 0..bmp.h {
            let py = y + by;
            if py >= self.height {
                break;
            }
            for bx in 0..bmp.w {
                let px = x + bx as i32;
                if px < 0 || px as u32 >= self.width {
                    continue;
                }
                if bmp.get(bx, by) != 0 {
                    self.dots[(py * self.width + px as u32) as usize] = 1;
                }
            }
        }
        self.revision += 1;
    }
}

/// Something placed in the current line buffer, positioned in dots.
#[derive(Debug, Clone)]
struct LineItem {
    x: u32,
    bmp: DotBitmap,
}

#[derive(Debug, Clone, Default)]
struct LineBuffer {
    items: Vec<LineItem>,
    x: u32,
    justification: Option<Justification>,
    text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrinterState {
    pub paper_width: PaperWidth,
    /// Code page selected by the printer's DIP switches / memory switch (used until ESC t)
    pub default_codepage: u8,

    // Current modes (public for status display)
    pub current_font: Font,
    pub justification: Justification,
    pub emphasis: bool,
    pub underline: u8,
    pub italic: bool,
    pub reverse: bool,
    pub upside_down: bool,
    pub width_mul: u32,
    pub height_mul: u32,
    pub char_spacing: u32,
    pub line_spacing: u32,
    pub left_margin: u32,
    pub area_width: Option<u32>,
    pub codepage: u8,
    pub dpi: u32,
    tab_stops: Vec<u8>,
    barcode_height: u32,
    barcode_width: u32,
    hri_position: u8,
    qr_module: u32,
    qr_ec: u8,
    qr_data: Vec<u8>,

    #[serde(skip)]
    line: LineBuffer,
    cursor_y: u32,
    pub pages: Vec<Page>,
    /// Plain-text transcript (for the Raw Text tab)
    pub text_lines: Vec<String>,
    /// Every command since the last clear, so the receipt can be re-rendered when the
    /// paper width or default code page changes.
    journal: Vec<EscPosCommand>,
}

impl PrinterState {
    pub fn new() -> Self {
        let paper_width = PaperWidth::Width80mm;
        let w = paper_width.get_width_dots();
        Self {
            paper_width,
            default_codepage: 2,
            current_font: Font::FontA,
            justification: Justification::Left,
            emphasis: false,
            underline: 0,
            italic: false,
            reverse: false,
            upside_down: false,
            width_mul: 1,
            height_mul: 1,
            char_spacing: 0,
            line_spacing: DEFAULT_LINE_SPACING,
            left_margin: 0,
            area_width: None,
            codepage: 2,
            dpi: 203,
            tab_stops: default_tabs(),
            barcode_height: 162,
            barcode_width: 3,
            hri_position: 0,
            qr_module: 3,
            qr_ec: 48,
            qr_data: Vec::new(),
            line: LineBuffer::default(),
            cursor_y: 0,
            pages: vec![Page::new(w)],
            text_lines: Vec::new(),
            journal: Vec::new(),
        }
    }

    fn reset_modes(&mut self) {
        self.current_font = Font::FontA;
        self.justification = Justification::Left;
        self.emphasis = false;
        self.underline = 0;
        self.italic = false;
        self.reverse = false;
        self.upside_down = false;
        self.width_mul = 1;
        self.height_mul = 1;
        self.char_spacing = 0;
        self.line_spacing = DEFAULT_LINE_SPACING;
        self.left_margin = 0;
        self.area_width = None;
        self.codepage = self.default_codepage;
        self.tab_stops = default_tabs();
        self.barcode_height = 162;
        self.barcode_width = 3;
        self.hri_position = 0;
        self.qr_module = 3;
        self.qr_ec = 48;
        self.qr_data.clear();
    }

    pub fn process_command(&mut self, command: &EscPosCommand) {
        self.journal.push(command.clone());
        self.apply(command);
    }

    fn apply(&mut self, command: &EscPosCommand) {
        match command {
            EscPosCommand::Text(bytes) => {
                for &b in bytes {
                    let c = codepage::decode(self.codepage, b);
                    self.put_char(c);
                }
            }
            EscPosCommand::NewLine => self.print_and_feed(self.line_spacing),
            EscPosCommand::CarriageReturn => {}
            EscPosCommand::HorizontalTab => self.tab(),

            EscPosCommand::SetFont(f) => self.current_font = f.clone(),
            EscPosCommand::SetPrintMode(n) => {
                self.current_font = if n & 0x01 != 0 { Font::FontB } else { Font::FontA };
                self.emphasis = n & 0x08 != 0;
                self.height_mul = if n & 0x10 != 0 { 2 } else { 1 };
                self.width_mul = if n & 0x20 != 0 { 2 } else { 1 };
                self.underline = if n & 0x80 != 0 { 1 } else { 0 };
            }
            EscPosCommand::SetCharSize(n) => {
                self.width_mul = ((n >> 4) & 0x07) as u32 + 1;
                self.height_mul = (n & 0x07) as u32 + 1;
            }
            EscPosCommand::SetEmphasis(b) => self.emphasis = *b,
            EscPosCommand::SetUnderline(n) => self.underline = *n,
            EscPosCommand::SetItalic(b) => self.italic = *b,
            EscPosCommand::SetReverse(b) => self.reverse = *b,
            EscPosCommand::SetUpsideDown(b) => self.upside_down = *b,
            EscPosCommand::SetCharSpacing(n) => self.char_spacing = *n as u32,
            EscPosCommand::SetCodepage(cp) => self.codepage = *cp,

            EscPosCommand::SetJustification(j) => {
                self.justification = j.clone();
                if self.line.items.is_empty() {
                    self.line.justification = None;
                }
            }
            EscPosCommand::SetLineSpacing(n) => {
                self.line_spacing = n.map(|v| v as u32).unwrap_or(DEFAULT_LINE_SPACING)
            }
            EscPosCommand::SetLeftMargin(n) => {
                if self.line.items.is_empty() {
                    self.left_margin = (*n as u32).min(self.print_width());
                }
            }
            EscPosCommand::SetPrintAreaWidth(n) => {
                if self.line.items.is_empty() {
                    self.area_width = Some(*n as u32);
                }
            }
            EscPosCommand::AbsolutePosition(n) => {
                if (*n as u32) < self.area() {
                    self.line.x = *n as u32;
                }
            }
            EscPosCommand::RelativePosition(n) => {
                let nx = self.line.x as i32 + *n as i32;
                if nx >= 0 && (nx as u32) < self.area() {
                    self.line.x = nx as u32;
                }
            }
            EscPosCommand::SetTabStops(stops) => self.tab_stops = stops.clone(),

            EscPosCommand::FeedDots(n) => self.print_and_feed(*n as u32),
            EscPosCommand::FeedLines(n) => self.print_and_feed(*n as u32 * self.line_spacing),
            EscPosCommand::CutPaper => self.cut(),

            EscPosCommand::BitImage { mode, width, data } => self.bit_image(*mode, *width, data),
            EscPosCommand::PrintRasterImage { mode, width_bytes, height, data } => {
                self.raster_image(*mode, *width_bytes, *height, data)
            }

            EscPosCommand::SetBarcodeHeight(n) => self.barcode_height = (*n as u32).max(1),
            EscPosCommand::SetBarcodeWidth(n) => self.barcode_width = (*n as u32).clamp(1, 6),
            EscPosCommand::SetHriPosition(n) => self.hri_position = *n,
            EscPosCommand::Barcode { system, data } => self.barcode(*system, data),
            EscPosCommand::QrModuleSize(n) => self.qr_module = (*n as u32).clamp(1, 16),
            EscPosCommand::QrErrorCorrection(n) => self.qr_ec = *n,
            EscPosCommand::QrStoreData(d) => self.qr_data = d.clone(),
            EscPosCommand::QrPrint => self.qr_print(),

            EscPosCommand::InitializePrinter => {
                self.line = LineBuffer::default();
                self.reset_modes();
            }
            EscPosCommand::Ignored(_) | EscPosCommand::Unknown(_) => {}
        }
    }

    // ------------------------------------------------------------------ geometry

    fn print_width(&self) -> u32 {
        self.paper_width.get_width_dots()
    }

    /// Width available for the line, after the left margin
    fn area(&self) -> u32 {
        let max = self.print_width().saturating_sub(self.left_margin);
        self.area_width.map(|w| w.min(max)).unwrap_or(max).max(1)
    }

    fn page(&mut self) -> &mut Page {
        self.pages.last_mut().expect("there is always a current page")
    }

    // ------------------------------------------------------------------ text

    fn char_bitmap(&self, c: char) -> DotBitmap {
        let (cw, ch) = font::cell_size(&self.current_font);
        let g = font::glyph(c, cw, ch);
        let mut cell = DotBitmap::new(cw + self.char_spacing, ch);

        for y in 0..ch {
            for x in 0..cw {
                if g.get(x, y) != 0 {
                    // Italic (non-standard): shear the glyph
                    let shift = if self.italic { (ch - 1 - y) / 6 } else { 0 };
                    cell.set(x + shift, y);
                    if self.emphasis {
                        // Emphasised = each dot printed twice, shifted one dot right
                        cell.set(x + shift + 1, y);
                    }
                }
            }
        }
        if self.underline > 0 {
            for t in 0..self.underline as u32 {
                for x in 0..cell.w {
                    cell.set(x, ch - 1 - t);
                }
            }
        }
        if self.reverse {
            for d in cell.dots.iter_mut() {
                *d ^= 1;
            }
        }
        if self.upside_down {
            cell.dots.reverse();
        }
        cell.scaled(self.width_mul, self.height_mul)
    }

    fn put_char(&mut self, c: char) {
        let bmp = self.char_bitmap(c);
        if self.line.x + bmp.w > self.area() {
            // Line full: the printer prints it and continues on the next line
            self.print_and_feed(self.line_spacing);
        }
        self.push_item(bmp);
        self.line.text.push(c);
    }

    fn push_item(&mut self, bmp: DotBitmap) {
        if self.line.justification.is_none() {
            self.line.justification = Some(self.justification.clone());
        }
        let w = bmp.w;
        let x = self.line.x;
        self.line.items.push(LineItem { x, bmp });
        self.line.x += w;
    }

    fn tab(&mut self) {
        let (cw, _) = font::cell_size(&self.current_font);
        let col_w = (cw + self.char_spacing) * self.width_mul;
        let col = self.line.x / col_w.max(1);
        if let Some(next) = self.tab_stops.iter().map(|&t| t as u32).find(|&t| t > col) {
            let nx = next * col_w;
            if nx < self.area() {
                let pad = ((nx - self.line.x) / col_w.max(1)) as usize;
                self.line.text.push_str(&" ".repeat(pad));
                self.line.x = nx;
                if self.line.justification.is_none() {
                    self.line.justification = Some(self.justification.clone());
                }
            }
        }
    }

    /// Print the line buffer, then advance the paper `feed` dots
    /// (or the height of the tallest element if it is taller).
    fn print_and_feed(&mut self, feed: u32) {
        let items = std::mem::take(&mut self.line.items);
        let content_w = items.iter().map(|i| i.x + i.bmp.w).max().unwrap_or(0);
        let content_h = items.iter().map(|i| i.bmp.h).max().unwrap_or(0);
        let just = self.line.justification.take().unwrap_or(self.justification.clone());
        let text = std::mem::take(&mut self.line.text);
        self.line.x = 0;

        if !items.is_empty() {
            let free = self.area().saturating_sub(content_w);
            let offset = self.left_margin
                + match just {
                    Justification::Left => 0,
                    Justification::Center => free / 2,
                    Justification::Right => free,
                };
            let y0 = self.cursor_y;
            let page = self.page();
            for item in &items {
                // Characters of different heights are aligned on the bottom line
                let y = y0 + (content_h - item.bmp.h);
                page.blit(&item.bmp, (offset + item.x) as i32, y);
            }
        }

        self.text_lines.push(match just {
            Justification::Center if !text.trim().is_empty() => format!("{:^w$}", text.trim(), w = self.paper_width.columns(&Font::FontA) as usize),
            Justification::Right if !text.trim().is_empty() => format!("{:>w$}", text.trim_end(), w = self.paper_width.columns(&Font::FontA) as usize),
            _ => text,
        });

        self.cursor_y += feed.max(content_h);
        let y = self.cursor_y;
        let page = self.page();
        page.ensure_height(y);
        page.revision += 1;
    }

    /// Print a block (raster image, QR, barcode) on its own, honouring justification.
    fn print_block(&mut self, bmp: &DotBitmap) {
        if !self.line.items.is_empty() {
            self.print_and_feed(self.line_spacing);
        }
        let free = self.area().saturating_sub(bmp.w);
        let offset = self.left_margin
            + match self.justification {
                Justification::Left => 0,
                Justification::Center => free / 2,
                Justification::Right => free,
            };
        let y = self.cursor_y;
        self.page().blit(bmp, offset as i32, y);
        self.cursor_y += bmp.h;
    }

    fn cut(&mut self) {
        if !self.line.items.is_empty() {
            self.print_and_feed(self.line_spacing);
        }
        let w = self.print_width();
        let page = self.page();
        page.cut = true;
        page.revision += 1;
        self.pages.push(Page::new(w));
        self.cursor_y = 0;
        self.text_lines.push("✂ ------------- CORTE ------------- ✂".to_string());
    }

    // ------------------------------------------------------------------ graphics

    fn bit_image(&mut self, mode: u8, width: u16, data: &[u8]) {
        let (bytes_per_col, sx, sy) = match mode {
            0 => (1, 2, 3),
            1 => (1, 1, 3),
            32 => (3, 2, 1),
            33 => (3, 1, 1),
            _ => (1, 1, 1),
        };
        let h = bytes_per_col * 8;
        let mut bmp = DotBitmap::new(width as u32, h);
        for col in 0..width as u32 {
            for b in 0..bytes_per_col {
                let byte = data.get((col * bytes_per_col + b) as usize).copied().unwrap_or(0);
                for bit in 0..8 {
                    if byte & (0x80 >> bit) != 0 {
                        bmp.set(col, b * 8 + bit);
                    }
                }
            }
        }
        let bmp = bmp.scaled(sx, sy);
        if self.line.x + bmp.w > self.area() {
            return; // printer discards the part outside the print area
        }
        self.push_item(bmp);
        self.line.text.push_str("[img]");
    }

    fn raster_image(&mut self, mode: u8, width_bytes: u16, height: u16, data: &[u8]) {
        let w = width_bytes as u32 * 8;
        let h = height as u32;
        let mut bmp = DotBitmap::new(w, h);
        for y in 0..h {
            for x in 0..w {
                let idx = (y * width_bytes as u32 + x / 8) as usize;
                if data.get(idx).map(|b| b & (0x80 >> (x % 8)) != 0).unwrap_or(false) {
                    bmp.set(x, y);
                }
            }
        }
        let (sx, sy) = match mode {
            1 | 49 => (2, 1),
            2 | 50 => (1, 2),
            3 | 51 => (2, 2),
            _ => (1, 1),
        };
        let bmp = bmp.scaled(sx, sy);
        self.print_block(&bmp);
        self.text_lines.push(format!("[ imagem raster {}x{} dots ]", bmp.w, bmp.h));
    }

    fn qr_print(&mut self) {
        use qrcode::{EcLevel, QrCode};
        let ec = match self.qr_ec {
            49 => EcLevel::M,
            50 => EcLevel::Q,
            51 => EcLevel::H,
            _ => EcLevel::L,
        };
        let Ok(code) = QrCode::with_error_correction_level(&self.qr_data, ec) else {
            return;
        };
        let n = code.width() as u32;
        let m = self.qr_module;
        let colors = code.to_colors();
        let mut bmp = DotBitmap::new(n * m, n * m);
        for y in 0..n {
            for x in 0..n {
                if colors[(y * n + x) as usize] == qrcode::Color::Dark {
                    for dy in 0..m {
                        for dx in 0..m {
                            bmp.set(x * m + dx, y * m + dy);
                        }
                    }
                }
            }
        }
        self.print_block(&bmp);
        self.text_lines
            .push(format!("[ QR: {} ]", String::from_utf8_lossy(&self.qr_data)));
    }

    /// Barcodes: drawn with the configured height/width so the layout is right.
    /// The bar pattern itself is a visual approximation (not scannable).
    fn barcode(&mut self, system: u8, data: &[u8]) {
        let module = self.barcode_width;
        let h = self.barcode_height;
        let modules = 11 * (data.len() as u32 + 3) + 2;
        let w = (modules * module).min(self.area());
        let mut bmp = DotBitmap::new(w, h);
        let mut x = 0u32;
        let mut seed: u32 = 0x9E37_79B9 ^ system as u32;
        let mut bar = true;
        while x < w {
            for &b in data.iter().chain([0u8, 1, 2].iter()) {
                seed = seed.rotate_left(5) ^ b as u32;
                let run = 1 + (seed % 3);
                for _ in 0..run * module {
                    if bar && x < w {
                        for y in 0..h {
                            bmp.set(x, y);
                        }
                    }
                    x += 1;
                }
                bar = !bar;
                if x >= w {
                    break;
                }
            }
        }
        let hri: String = data.iter().map(|&b| codepage::decode(self.codepage, b)).collect();
        if self.hri_position == 1 || self.hri_position == 3 {
            self.hri_line(&hri);
        }
        self.print_block(&bmp);
        if self.hri_position == 2 || self.hri_position == 3 {
            self.hri_line(&hri);
        }
        self.text_lines.push(format!("[ código de barras: {} ]", hri));
    }

    fn hri_line(&mut self, text: &str) {
        let saved = (self.width_mul, self.height_mul, self.emphasis, self.underline, self.reverse);
        self.width_mul = 1;
        self.height_mul = 1;
        self.emphasis = false;
        self.underline = 0;
        self.reverse = false;
        for c in text.chars() {
            self.put_char(c);
        }
        self.print_and_feed(0);
        (self.width_mul, self.height_mul, self.emphasis, self.underline, self.reverse) = saved;
    }

    // ------------------------------------------------------------------ public API

    pub fn clear_buffer(&mut self) {
        self.journal.clear();
        self.restart();
    }

    /// Throw away rendered output and modes (keeps journal)
    fn restart(&mut self) {
        self.line = LineBuffer::default();
        self.reset_modes();
        self.cursor_y = 0;
        self.pages = vec![Page::new(self.print_width())];
        self.text_lines.clear();
    }

    /// Re-render everything received since the last clear (after a config change).
    fn rerender(&mut self) {
        let journal = std::mem::take(&mut self.journal);
        self.restart();
        for cmd in &journal {
            self.apply(cmd);
        }
        self.journal = journal;
    }

    pub fn pages(&self) -> &[Page] {
        &self.pages
    }

    pub fn has_output(&self) -> bool {
        self.pages.iter().any(|p| p.height > 0) || !self.line.items.is_empty()
    }

    pub fn get_paper_width_dots(&self) -> u32 {
        self.paper_width.get_width_dots()
    }

    pub fn set_paper_width(&mut self, width: PaperWidth) {
        if self.paper_width != width {
            self.paper_width = width;
            self.rerender();
        }
    }

    pub fn set_default_codepage(&mut self, cp: u8) {
        if self.default_codepage != cp {
            self.default_codepage = cp;
            self.rerender();
        }
    }
}

impl Default for PrinterState {
    fn default() -> Self {
        Self::new()
    }
}

fn default_tabs() -> Vec<u8> {
    (1..=31u8).map(|i| i * 8).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::escpos::parser::EscPosParser;

    fn run(bytes: &[u8]) -> PrinterState {
        let mut p = EscPosParser::new();
        let mut s = PrinterState::new();
        for c in p.parse_stream(bytes).unwrap() {
            s.process_command(&c);
        }
        s
    }

    #[test]
    fn line_of_48_chars_fits_80mm() {
        let s = run(&[b"-".repeat(48), b"\n".to_vec()].concat());
        assert_eq!(s.text_lines.len(), 1);
    }

    #[test]
    fn line_of_49_chars_wraps() {
        let s = run(&[b"-".repeat(49), b"\n".to_vec()].concat());
        assert_eq!(s.text_lines.len(), 2);
    }

    #[test]
    fn double_size_halves_columns() {
        let s = run(&[b"\x1d\x21\x11".to_vec(), b"X".repeat(25), b"\n".to_vec()].concat());
        assert_eq!(s.text_lines.len(), 2); // 24 fit, 25th wraps
        assert_eq!(s.pages[0].height, 48 + 48);
    }

    #[test]
    fn codepage_850_accents() {
        let s = run(b"\x1bt\x02OBSERVA\x80\xc7O\n");
        assert_eq!(s.text_lines[0], "OBSERVAÇÃO");
    }
}
