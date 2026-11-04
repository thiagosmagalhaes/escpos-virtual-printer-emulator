//! Dot-matrix character generator.
//!
//! Thermal printers draw every character inside a fixed cell of dots
//! (Font A = 12x24, Font B = 9x17 on Epson-compatible printers). We rasterise a
//! monospace TrueType font into exactly that cell, 1 bit per dot, so the result has
//! the same width, height, column count and wrapping as the real printout.

use crate::escpos::commands::Font;
use ab_glyph::{Font as _, FontVec, PxScale, ScaleFont};
use std::collections::HashMap;
use std::sync::{Arc, Mutex, OnceLock};

/// A monochrome bitmap, row-major, 1 byte per dot (0 = blank, 1 = burned).
#[derive(Debug, Clone)]
pub struct DotBitmap {
    pub w: u32,
    pub h: u32,
    pub dots: Vec<u8>,
}

impl DotBitmap {
    pub fn new(w: u32, h: u32) -> Self {
        Self { w, h, dots: vec![0; (w * h) as usize] }
    }
    #[inline]
    pub fn get(&self, x: u32, y: u32) -> u8 {
        if x < self.w && y < self.h {
            self.dots[(y * self.w + x) as usize]
        } else {
            0
        }
    }
    #[inline]
    pub fn set(&mut self, x: u32, y: u32) {
        if x < self.w && y < self.h {
            self.dots[(y * self.w + x) as usize] = 1;
        }
    }
    /// Nearest-neighbour integer scaling (exactly what the printer does for GS ! / ESC !)
    pub fn scaled(&self, sx: u32, sy: u32) -> DotBitmap {
        if sx == 1 && sy == 1 {
            return self.clone();
        }
        let mut out = DotBitmap::new(self.w * sx, self.h * sy);
        for y in 0..out.h {
            for x in 0..out.w {
                out.dots[(y * out.w + x) as usize] = self.get(x / sx, y / sy);
            }
        }
        out
    }
}

/// Character cell size in dots (including the built-in inter-character space).
pub fn cell_size(font: &Font) -> (u32, u32) {
    match font {
        Font::FontA => (12, 24),
        Font::FontB | Font::FontC => (9, 17),
    }
}

struct Generator {
    font: FontVec,
    cache: Mutex<HashMap<(char, u32, u32), Arc<DotBitmap>>>,
}

fn generator() -> &'static Generator {
    static GEN: OnceLock<Generator> = OnceLock::new();
    GEN.get_or_init(|| {
        // Hack (monospace) ships inside egui — no extra font file required.
        let defs = egui::FontDefinitions::default();
        let bytes = defs
            .font_data
            .get("Hack")
            .map(|d| d.font.to_vec())
            .expect("egui default fonts must include Hack");
        let font = FontVec::try_from_vec(bytes).expect("valid Hack font");
        Generator { font, cache: Mutex::new(HashMap::new()) }
    })
}

/// Return the dot pattern for `c` inside a `cw` x `ch` cell.
pub fn glyph(c: char, cw: u32, ch: u32) -> Arc<DotBitmap> {
    let gen = generator();
    if let Some(g) = gen.cache.lock().unwrap().get(&(c, cw, ch)) {
        return g.clone();
    }
    let bmp = Arc::new(rasterize(&gen.font, c, cw, ch));
    gen.cache.lock().unwrap().insert((c, cw, ch), bmp.clone());
    bmp
}

fn rasterize(font: &FontVec, c: char, cw: u32, ch: u32) -> DotBitmap {
    let mut out = DotBitmap::new(cw, ch);
    if c == ' ' || c == '\u{A0}' || c.is_control() {
        return out;
    }

    // Leave one empty column on the right and fit the font body to the cell height,
    // like printer ROM fonts (tall, narrow glyphs).
    let units = font.units_per_em().unwrap_or(2048.0);
    let probe = font.as_scaled(PxScale::from(units));
    let adv = probe.h_advance(font.glyph_id('M')) / units; // in em
    let body = (probe.ascent() - probe.descent()) / units; // in em
    let sx = (cw as f32) / adv;
    let sy = (ch as f32) / body;
    let scale = PxScale { x: sx, y: sy };
    let scaled = font.as_scaled(scale);

    let gid = font.glyph_id(c);
    if gid.0 == 0 {
        // Character not available: print a small hollow box like many printers do for undefined codes
        for y in ch / 4..ch * 3 / 4 {
            out.set(1, y);
            out.set(cw - 3, y);
        }
        for x in 1..cw - 2 {
            out.set(x, ch / 4);
            out.set(x, ch * 3 / 4 - 1);
        }
        return out;
    }

    let glyph = gid.with_scale_and_position(scale, ab_glyph::point(0.0, scaled.ascent()));
    if let Some(outlined) = font.outline_glyph(glyph) {
        let b = outlined.px_bounds();
        outlined.draw(|x, y, cov| {
            if cov >= 0.45 {
                let px = b.min.x as i32 + x as i32;
                let py = b.min.y as i32 + y as i32;
                if px >= 0 && py >= 0 {
                    out.set(px as u32, py as u32);
                }
            }
        });
    }
    out
}
