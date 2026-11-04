use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EscPosCommand {
    /// Raw printable bytes (decoded later with the active code page, like a real printer does)
    Text(Vec<u8>),
    /// LF — print buffer and feed one line
    NewLine,
    /// CR — ignored in standard mode (auto line feed disabled)
    CarriageReturn,
    /// HT — move to next horizontal tab position
    HorizontalTab,

    // Character formatting
    /// ESC M n
    SetFont(Font),
    /// ESC ! n — print mode bit field (font B, bold, double height, double width, underline)
    SetPrintMode(u8),
    /// GS ! n — character size (high nibble = width-1, low nibble = height-1)
    SetCharSize(u8),
    /// ESC E n / ESC G n
    SetEmphasis(bool),
    /// ESC - n (0 = off, 1 = 1 dot, 2 = 2 dots)
    SetUnderline(u8),
    /// ESC 4 / ESC 5 (non-standard, kept for compatibility)
    SetItalic(bool),
    /// GS B n — white/black reverse
    SetReverse(bool),
    /// ESC { n — upside down
    SetUpsideDown(bool),
    /// ESC SP n — right-side character spacing in dots
    SetCharSpacing(u8),
    /// ESC t n
    SetCodepage(u8),

    // Layout
    /// ESC a n
    SetJustification(Justification),
    /// ESC 3 n (Some) / ESC 2 (None = default)
    SetLineSpacing(Option<u8>),
    /// GS L nL nH
    SetLeftMargin(u16),
    /// GS W nL nH
    SetPrintAreaWidth(u16),
    /// ESC $ nL nH
    AbsolutePosition(u16),
    /// ESC \ nL nH
    RelativePosition(i16),
    /// ESC D n1..nk NUL
    SetTabStops(Vec<u8>),

    // Paper movement
    /// ESC J n — print and feed n dots
    FeedDots(u8),
    /// ESC d n — print and feed n lines
    FeedLines(u8),
    /// GS V / ESC i / ESC m
    CutPaper,

    // Graphics
    /// ESC * m nL nH d1..dk (column format bit image, goes into the line buffer)
    BitImage { mode: u8, width: u16, data: Vec<u8> },
    /// GS v 0 m xL xH yL yH d1..dk
    PrintRasterImage { mode: u8, width_bytes: u16, height: u16, data: Vec<u8> },

    // Barcodes / 2D codes
    /// GS h n
    SetBarcodeHeight(u8),
    /// GS w n
    SetBarcodeWidth(u8),
    /// GS H n
    SetHriPosition(u8),
    /// GS k m ...
    Barcode { system: u8, data: Vec<u8> },
    /// GS ( k — QR: module size (fn 67)
    QrModuleSize(u8),
    /// GS ( k — QR: error correction level (fn 69) 48..51 = L,M,Q,H
    QrErrorCorrection(u8),
    /// GS ( k — QR: store data (fn 80)
    QrStoreData(Vec<u8>),
    /// GS ( k — QR: print stored symbol (fn 81)
    QrPrint,

    // Control
    /// ESC @
    InitializePrinter,

    /// Command recognised and consumed, but it has no visual effect (cash drawer, status, etc.)
    Ignored(Vec<u8>),
    /// Unknown command
    Unknown(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Font {
    FontA,
    FontB,
    FontC,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Justification {
    Left,
    Center,
    Right,
}
