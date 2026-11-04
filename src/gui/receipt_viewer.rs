use crate::emulator::EmulatorState;
use crate::escpos::codepage;
use crate::escpos::printer::{Page, PaperWidth, PrinterState, DOTS_PER_MM};
use egui::{
    pos2, vec2, Color32, ColorImage, Frame, Margin, Rect, RichText, ScrollArea, Sense, Stroke,
    TextureHandle, TextureOptions, Ui,
};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

const TILE_ROWS: u32 = 2048;
const PAPER: Color32 = Color32::from_rgb(250, 249, 245);
const INK: Color32 = Color32::from_rgb(24, 24, 30);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ReceiptSubTab {
    #[default]
    RealPreview,
    RawText,
}

pub struct ReceiptViewer {
    selected_sub_tab: ReceiptSubTab,
    show_paper_shadow: bool,
    /// Screen pixels per printer dot
    zoom_factor: f32,
    /// (page id, tile index) -> (page revision, texture)
    tiles: HashMap<(u64, u32), (u64, TextureHandle)>,
}

impl Default for ReceiptViewer {
    fn default() -> Self {
        Self {
            selected_sub_tab: ReceiptSubTab::RealPreview,
            show_paper_shadow: true,
            zoom_factor: 1.0,
            tiles: HashMap::new(),
        }
    }
}

impl ReceiptViewer {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn show(&mut self, ui: &mut Ui, emulator_state: &Arc<Mutex<EmulatorState>>) {
        ui.vertical(|ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(&mut self.selected_sub_tab, ReceiptSubTab::RealPreview, "📄 Real Preview");
                ui.selectable_value(&mut self.selected_sub_tab, ReceiptSubTab::RawText, "📝 Raw Text");
                ui.separator();

                if let Ok(mut state) = emulator_state.try_lock() {
                    let ps = &state.printer_state;
                    let current_pw = ps.paper_width.clone();
                    let current_cp = ps.default_codepage;

                    ui.label(RichText::new("📏 Papel:").strong());
                    for (pw, label) in [
                        (PaperWidth::Width58mm, "58mm (32 col)"),
                        (PaperWidth::Width80mm, "80mm (48 col)"),
                        (PaperWidth::Width80mm42, "80mm (42 col)"),
                    ] {
                        if ui.selectable_label(current_pw == pw, label).clicked() {
                            state.printer_state.set_paper_width(pw);
                        }
                    }

                    ui.separator();
                    ui.label(RichText::new("🌐 Code page padrão:").strong())
                        .on_hover_text("Code page configurada na impressora (usada enquanto o sistema não envia ESC t)");
                    egui::ComboBox::from_id_source("default_cp")
                        .selected_text(codepage::name(current_cp))
                        .show_ui(ui, |ui| {
                            for (cp, label) in codepage::SUPPORTED {
                                if ui.selectable_label(current_cp == *cp, *label).clicked() {
                                    state.printer_state.set_default_codepage(*cp);
                                }
                            }
                        });
                }

                ui.separator();
                if self.selected_sub_tab == ReceiptSubTab::RealPreview {
                    ui.checkbox(&mut self.show_paper_shadow, "Sombra");
                    ui.add(egui::Slider::new(&mut self.zoom_factor, 0.5..=3.0).text("Zoom"));
                    if ui.button("1:1 mm").on_hover_text("Tamanho físico aproximado (96 dpi)").clicked() {
                        // 203 dpi printer dots shown on a ~96 dpi screen
                        self.zoom_factor = 96.0 / 203.0 / ui.ctx().pixels_per_point();
                    }
                }
                if ui.button("🗑️ Clear").on_hover_text("Limpar o papel").clicked() {
                    if let Ok(mut state) = emulator_state.try_lock() {
                        state.clear_printer_buffer();
                    }
                    self.tiles.clear();
                }
            });

            ui.separator();

            match self.selected_sub_tab {
                ReceiptSubTab::RealPreview => {
                    Frame::none()
                        .fill(Color32::from_rgb(26, 28, 38))
                        .inner_margin(Margin::same(16.0))
                        .show(ui, |ui| {
                            ScrollArea::both().auto_shrink([false, false]).stick_to_bottom(true).show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    if let Ok(state) = emulator_state.try_lock() {
                                        self.render_paper(ui, &state.printer_state);
                                    } else {
                                        ui.colored_label(Color32::RED, "Loading printer state...");
                                        ui.ctx().request_repaint();
                                    }
                                });
                            });
                        });
                }
                ReceiptSubTab::RawText => {
                    ScrollArea::both().show(ui, |ui| {
                        if let Ok(state) = emulator_state.try_lock() {
                            Self::render_raw_text(ui, &state.printer_state);
                        }
                    });
                }
            }
        });
    }

    fn render_paper(&mut self, ui: &mut Ui, ps: &PrinterState) {
        let scale = self.zoom_factor;
        let paper_dots = ps.paper_width.paper_dots();
        let print_dots = ps.paper_width.get_width_dots();
        let paper_w = paper_dots as f32 * scale;
        let side_margin = (paper_dots - print_dots) as f32 / 2.0 * scale;
        let top_pad = 3.0 * DOTS_PER_MM * scale;

        // Header strip (printer mouth)
        Frame::none()
            .fill(Color32::from_rgb(45, 48, 62))
            .stroke(Stroke::new(1.0_f32, Color32::from_rgb(65, 70, 90)))
            .rounding(egui::Rounding { nw: 8.0, ne: 8.0, sw: 2.0, se: 2.0 })
            .inner_margin(Margin::symmetric(12.0, 6.0))
            .show(ui, |ui| {
                ui.set_width(paper_w.max(260.0));
                ui.label(
                    RichText::new(format!(
                        "● {} · 203 dpi · Font A 12x24 · cp {}",
                        ps.paper_width.label(),
                        codepage::name(ps.codepage)
                    ))
                    .color(Color32::from_rgb(200, 205, 220))
                    .small()
                    .monospace(),
                );
            });

        if !ps.has_output() {
            ui.add_space(30.0);
            ui.label(RichText::new("📄 Papel pronto — aguardando dados ESC/POS (TCP 9100 ou serial)").color(Color32::GRAY));
            return;
        }

        let pages = ps.pages();
        // Skip the empty page that follows the last cut
        let visible: Vec<&Page> = pages
            .iter()
            .enumerate()
            .filter(|(i, p)| p.height > 0 || *i + 1 < pages.len())
            .map(|(_, p)| p)
            .collect();

        for (idx, page) in visible.iter().enumerate() {
            let body_h = page.height as f32 * scale;
            let total_h = body_h + top_pad * 2.0;
            let (rect, _) = ui.allocate_exact_size(vec2(paper_w, total_h), Sense::hover());
            let painter = ui.painter_at(rect.expand(12.0));

            if self.show_paper_shadow {
                painter.rect_filled(rect.translate(vec2(4.0, 4.0)), 2.0, Color32::from_black_alpha(90));
            }
            painter.rect_filled(rect, 0.0, PAPER);

            // Tiles of the page bitmap
            let tiles = (page.height + TILE_ROWS - 1) / TILE_ROWS;
            for t in 0..tiles {
                let tex = self.tile_texture(ui, page, t);
                let y0 = t * TILE_ROWS;
                let rows = (page.height - y0).min(TILE_ROWS);
                let min = pos2(rect.min.x + side_margin, rect.min.y + top_pad + y0 as f32 * scale);
                let r = Rect::from_min_size(min, vec2(page.width as f32 * scale, rows as f32 * scale));
                painter.image(tex, r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }

            if page.cut {
                // Zig-zag cut edge
                let y = rect.max.y;
                let mut pts = Vec::new();
                let step = 6.0;
                let mut x = rect.min.x;
                let mut up = true;
                while x <= rect.max.x {
                    pts.push(pos2(x, if up { y } else { y - 3.0 }));
                    x += step;
                    up = !up;
                }
                painter.add(egui::Shape::line(pts, Stroke::new(1.0_f32, Color32::from_rgb(120, 120, 130))));
                if idx + 1 < visible.len() {
                    ui.add_space(4.0);
                    ui.label(RichText::new("✂ corte").small().color(Color32::from_rgb(150, 150, 160)));
                    ui.add_space(4.0);
                }
            }
        }

        // Drop textures of pages that no longer exist
        let alive: std::collections::HashSet<u64> = pages.iter().map(|p| p.id).collect();
        self.tiles.retain(|(id, _), _| alive.contains(id));
    }

    fn tile_texture(&mut self, ui: &Ui, page: &Page, tile: u32) -> egui::TextureId {
        let key = (page.id, tile);
        if let Some((rev, tex)) = self.tiles.get(&key) {
            if *rev == page.revision {
                return tex.id();
            }
        }
        let y0 = tile * TILE_ROWS;
        let rows = (page.height - y0).min(TILE_ROWS);
        let start = (y0 * page.width) as usize;
        let end = start + (rows * page.width) as usize;
        let pixels: Vec<Color32> = page.dots[start..end]
            .iter()
            .map(|&d| if d != 0 { INK } else { PAPER })
            .collect();
        let img = ColorImage { size: [page.width as usize, rows as usize], pixels };
        let tex = ui
            .ctx()
            .load_texture(format!("page_{}_{}", page.id, tile), img, TextureOptions::LINEAR);
        let id = tex.id();
        self.tiles.insert(key, (page.revision, tex));
        id
    }

    fn render_raw_text(ui: &mut Ui, ps: &PrinterState) {
        if ps.text_lines.is_empty() {
            ui.label("No receipt data available");
            return;
        }
        let cols = ps.paper_width.columns(&crate::escpos::commands::Font::FontA) as usize;
        ui.label(format!("📄 {} · ruler = {} colunas Font A", ps.paper_width.label(), cols));
        ui.label(RichText::new(format!("    │{}│", "─".repeat(cols))).monospace().weak());
        for (n, line) in ps.text_lines.iter().enumerate() {
            ui.label(RichText::new(format!("{:03} │{}", n + 1, line)).monospace());
        }
    }
}
