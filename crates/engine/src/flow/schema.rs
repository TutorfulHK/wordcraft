//! Flow JSON v1: the serde types (`docs/flow-json.md` documents them).
//!
//! Units are points; colours are `#rrggbb`. Fields that may be absent default on input, so a host
//! only has to write what it uses.

use serde::{Deserialize, Serialize};

/// The schema version this engine reads and writes.
pub const VERSION: u32 = 1;

/// A document as a block tree.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Flow {
    pub version: u32,
    #[serde(default)]
    pub page: Page,
    #[serde(default)]
    pub blocks: Vec<Block>,
    /// Every media id the blocks reference (input: optional, ignored).
    #[serde(default)]
    pub media: Vec<MediaInfo>,
}

/// Page size and margins, points.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Page {
    pub w: f64,
    pub h: f64,
    pub margins: Margins,
}

impl Default for Page {
    /// US Letter, 1" margins (a new document's page).
    fn default() -> Self {
        Page { w: 612.0, h: 792.0, margins: Margins { t: 72.0, r: 72.0, b: 72.0, l: 72.0 } }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default)]
pub struct Margins {
    pub t: f64,
    pub r: f64,
    pub b: f64,
    pub l: f64,
}

/// A media item (picture) the blocks reference by `id`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct MediaInfo {
    pub id: String,
    pub content_type: String,
    pub name: String,
    /// Pixel size of the encoded image (0 when it can't be read).
    pub w: u32,
    pub h: u32,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Block {
    Paragraph(Paragraph),
    List(List),
    Table(Table),
    Image(ImageBlock),
    Rule,
    PageBreak,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum ParaStyle {
    #[default]
    Normal,
    Title,
    Quote,
    Code,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub enum Align {
    #[default]
    Left,
    Center,
    Right,
    Justify,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Paragraph {
    pub style: ParaStyle,
    /// 1..=6: a heading (then `style` is `normal`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub heading_level: Option<u8>,
    pub align: Align,
    /// Indent level (0..=8), 36 pt each.
    pub indent: u8,
    /// Line height as a multiple of single spacing; `null` = the style's.
    pub line_height: Option<f64>,
    /// Points; `null` = the style's.
    pub space_before: Option<f64>,
    pub space_after: Option<f64>,
    pub runs: Vec<Inline>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct List {
    pub ordered: bool,
    /// First number of an ordered list (omitted when 1).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub start: Option<u32>,
    pub items: Vec<ListItem>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ListItem {
    pub blocks: Vec<Block>,
    /// A task-list item: ticked or not; `null` = an ordinary item.
    pub checked: Option<bool>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Table {
    /// Grid column widths, points.
    pub col_widths: Vec<f64>,
    pub rows: Vec<Row>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Row {
    pub header: bool,
    /// Cells covered by a row span from above are left out (as in HTML).
    pub cells: Vec<Cell>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Cell {
    pub colspan: u32,
    pub rowspan: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub background: Option<String>,
    pub blocks: Vec<Block>,
}

impl Default for Cell {
    fn default() -> Self {
        Cell { colspan: 1, rowspan: 1, background: None, blocks: Vec::new() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct ImageBlock {
    pub media: String,
    /// Display size, points.
    pub w: f64,
    pub h: f64,
    pub alt: String,
    pub align: Align,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "camelCase")]
pub enum Inline {
    Text(Text),
    /// A line break inside the paragraph.
    Break,
    Image(InlineImage),
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct Text {
    pub text: String,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub strike: bool,
    pub code: bool,
    pub sup: bool,
    pub sub: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub highlight: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font: Option<String>,
    /// Points.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub size: Option<f64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
}

impl Text {
    /// Same formatting (everything but the text).
    pub fn same_format(&self, o: &Text) -> bool {
        Text { text: String::new(), ..self.clone() } == Text { text: String::new(), ..o.clone() }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
#[serde(default, rename_all = "camelCase")]
pub struct InlineImage {
    pub media: String,
    pub w: f64,
    pub h: f64,
    pub alt: String,
}
