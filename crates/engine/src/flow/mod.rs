//! Flow JSON v1: the document as a block tree for web hosts that keep their own rich-text model
//! (a ProseMirror / TipTap document): paragraphs, nested lists (task lists too), tables with
//! merges and shading, pictures, rules and page breaks. `docs/flow-json.md` documents it.
//!
//! [`to_flow`] reads a [`wordcraft_doc::Document`] with the richest fidelity the model has
//! (numbering → lists, merges, styles → headings, indents and spacing); [`from_flow`] builds a
//! document back. Both are total: hostile input gives an error or a clamped value, never a panic.

pub mod read;
pub mod schema;
pub mod write;

#[cfg(test)]
mod tests;

pub use read::to_flow;
pub use schema::Flow;
use wordcraft_doc::styles::StyleSheet;
pub use write::from_flow;

use self::schema::ParaStyle;

/// One indent level, points (Word's Increase Indent step).
pub const INDENT_STEP: f32 = 36.0;
/// Deepest list level (0-based; Word lists have nine).
pub const MAX_LEVEL: u8 = 8;
/// Tables nest at most this deep; deeper tables are flattened into their cells' blocks.
pub const MAX_DEPTH: usize = wordcraft_formats::model::MAX_DEPTH;
/// Task-list checkboxes, written at the start of the item's text.
pub const TASK_OPEN: char = '☐';
pub const TASK_DONE: char = '☒';

/// The paragraph style id a flow paragraph maps to (`None` = Normal).
pub fn style_id(style: ParaStyle, heading: Option<u8>, in_list: bool) -> Option<String> {
    if let Some(n) = heading.filter(|n| (1..=6).contains(n)) {
        return Some(format!("Heading{n}"));
    }
    match style {
        ParaStyle::Title => Some("Title".into()),
        ParaStyle::Quote => Some("Quote".into()),
        ParaStyle::Code => Some(wordcraft_formats::model::CODE_STYLE.into()),
        ParaStyle::Normal if in_list => Some("ListParagraph".into()),
        ParaStyle::Normal => None,
    }
}

/// The styles of a document built from a flow: the built-in set plus the code style.
pub fn reference_styles() -> StyleSheet {
    let mut s = StyleSheet::builtin();
    s.upsert(wordcraft_formats::model::code_style());
    s
}

/// MIME type of embedded media, sniffed from the bytes, else from the key's extension.
pub fn media_content_type(key: &str, data: &[u8]) -> &'static str {
    match wordcraft_formats::model::media_ext(key, data).as_str() {
        "svg" => "image/svg+xml",
        "tif" | "tiff" => "image/tiff",
        "emf" => "image/emf",
        "wmf" => "image/wmf",
        e => wordcraft_formats::model::mime_of(e),
    }
}
