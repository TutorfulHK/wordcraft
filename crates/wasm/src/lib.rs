//! WordCraft as a headless WebAssembly library for web hosts (run it in a Web Worker): open and
//! save DOCX (and ODT, RTF, HTML, Markdown, plain text), export PDF with embedded fonts, convert
//! the document to and from Flow JSON (`docs/flow-json.md`), render page PNGs.
//!
//! A thin binding over engine commands: [`Doc`] holds the logic (plain Rust, tested natively)
//! and [`Engine`] exports it with wasm-bindgen. A panic aborts on wasm (there is no unwinding), so
//! nothing here may panic; the host restarts the worker if the module traps.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

use std::collections::BTreeMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use serde_json::{Value, json};
use wasm_bindgen::prelude::*;
use wordcraft_engine::cmd::exchange;
use wordcraft_engine::flow::Flow;
use wordcraft_engine::layout::DocLayout;
use wordcraft_engine::{Session, doc::Document};

/// Bumped by every font added, so cached layouts made before it are redone.
static FONT_EPOCH: AtomicU64 = AtomicU64::new(0);

/// The library's state and logic: one open document.
pub struct Doc {
    session: Session,
    /// Print layout of the document at (revision, font epoch), for page rendering.
    layout: Option<(u64, u64, Arc<DocLayout>)>,
}

impl Default for Doc {
    fn default() -> Self {
        Self::new()
    }
}

impl Doc {
    /// A blank document.
    pub fn new() -> Self {
        Doc { session: Session::new(Document::new()), layout: None }
    }

    pub fn session(&self) -> &Session {
        &self.session
    }

    /// Run any engine command; params and result are JSON text.
    pub fn execute(&mut self, id: &str, params_json: &str) -> Result<String, String> {
        let params: Value =
            if params_json.trim().is_empty() { json!({}) } else { serde_json::from_str(params_json).map_err(|e| format!("params: {e}"))? };
        let v = self.session.run(id, &params).map_err(|e| e.to_string())?;
        serde_json::to_string(&v).map_err(|e| e.to_string())
    }

    /// Open document bytes (`name`'s extension gives the format), replacing the open one.
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<(), String> {
        exchange::open_bytes(&mut self.session, name, bytes).map_err(|e| e.to_string())
    }

    /// The document as `docx`, `pdf`, `html`, `md`, `txt`, `odt` or `rtf` bytes.
    pub fn save_bytes(&self, format: &str) -> Result<Vec<u8>, String> {
        exchange::save_bytes(&self.session, format).map_err(|e| e.to_string())
    }

    /// The document as Flow JSON text.
    pub fn flow(&self) -> Result<String, String> {
        serde_json::to_string(&wordcraft_engine::flow::to_flow(&self.session.doc)).map_err(|e| e.to_string())
    }

    /// Replace the document with one built from Flow JSON text.
    pub fn from_flow(&mut self, flow_json: &str) -> Result<(), String> {
        let flow: Flow = serde_json::from_str(flow_json).map_err(|e| format!("flow: {e}"))?;
        exchange::from_flow(&mut self.session, &flow, &BTreeMap::new()).map_err(|e| e.to_string())
    }

    /// Stage media bytes for the next [`Doc::from_flow`] (avoids base64 for big blobs).
    pub fn put_media(&mut self, id: &str, bytes: &[u8]) {
        self.session.flow_media.insert(id.to_string(), Arc::new(bytes.to_vec()));
    }

    /// Bytes of a media id the flow references.
    pub fn media_bytes(&self, id: &str) -> Result<Vec<u8>, String> {
        self.session.doc.media.get(id).map(|b| b.to_vec()).ok_or_else(|| format!("no media `{id}`"))
    }

    fn print_layout(&mut self) -> Arc<DocLayout> {
        let key = (self.session.rev(), FONT_EPOCH.load(Ordering::Relaxed));
        if let Some((r, e, l)) = &self.layout
            && (*r, *e) == key
        {
            return l.clone();
        }
        let l = self.session.export_layout();
        self.layout = Some((key.0, key.1, l.clone()));
        l
    }

    /// Pages in the print layout.
    pub fn page_count(&mut self) -> u32 {
        u32::try_from(self.print_layout().pages.len()).unwrap_or(u32::MAX)
    }

    /// Page `page` (0-based) as PNG at `scale` pixels per point.
    pub fn render_page_png(&mut self, page: u32, scale: f64) -> Result<Vec<u8>, String> {
        let l = self.print_layout();
        let i = usize::try_from(page).map_err(|e| e.to_string())?;
        let pg = l.pages.get(i).ok_or_else(|| format!("no page {i} (the document has {})", l.pages.len()))?;
        // Capped so a page stays a sane allocation (4 px/pt: 2448 × 3168 for Letter).
        let scale = if scale.is_finite() && scale > 0.0 { scale.clamp(0.01, 4.0) } else { 1.0 };
        let png = wordcraft_engine::render::render_page(&self.session.doc, pg, scale as f32, &Default::default()).to_png();
        if png.is_empty() { Err("the page could not be encoded".into()) } else { Ok(png) }
    }
}

/// Add a font (TTF/OTF/TTC bytes) for layout, rendering and PDF; returns the faces added.
pub fn add_font_bytes(bytes: &[u8]) -> u32 {
    let n = wordcraft_fonts::FontDb::global().add_font(bytes.to_vec());
    if n > 0 {
        FONT_EPOCH.fetch_add(1, Ordering::Relaxed);
    }
    u32::try_from(n).unwrap_or(u32::MAX)
}

fn js(e: String) -> JsError {
    JsError::new(&e)
}

/// The JS-facing engine: one open document (a blank one to start).
#[wasm_bindgen]
pub struct Engine {
    doc: Doc,
}

#[wasm_bindgen]
impl Engine {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Engine {
        Engine { doc: Doc::new() }
    }

    /// Run any engine command by id with JSON params; returns the JSON result.
    pub fn execute(&mut self, id: &str, params_json: &str) -> Result<String, JsError> {
        self.doc.execute(id, params_json).map_err(js)
    }

    #[wasm_bindgen(js_name = openBytes)]
    pub fn open_bytes(&mut self, name: &str, bytes: &[u8]) -> Result<(), JsError> {
        self.doc.open_bytes(name, bytes).map_err(js)
    }

    /// `docx`, `pdf`, `html`, `md`, `txt`, `odt` or `rtf`.
    #[wasm_bindgen(js_name = saveBytes)]
    pub fn save_bytes(&self, format: &str) -> Result<Vec<u8>, JsError> {
        self.doc.save_bytes(format).map_err(js)
    }

    /// The document as Flow JSON text.
    pub fn flow(&self) -> Result<String, JsError> {
        self.doc.flow().map_err(js)
    }

    #[wasm_bindgen(js_name = fromFlow)]
    pub fn from_flow(&mut self, flow_json: &str) -> Result<(), JsError> {
        self.doc.from_flow(flow_json).map_err(js)
    }

    #[wasm_bindgen(js_name = putMedia)]
    pub fn put_media(&mut self, id: &str, bytes: &[u8]) {
        self.doc.put_media(id, bytes)
    }

    #[wasm_bindgen(js_name = mediaBytes)]
    pub fn media_bytes(&self, id: &str) -> Result<Vec<u8>, JsError> {
        self.doc.media_bytes(id).map_err(js)
    }

    #[wasm_bindgen(js_name = pageCount)]
    pub fn page_count(&mut self) -> u32 {
        self.doc.page_count()
    }

    #[wasm_bindgen(js_name = renderPagePng)]
    pub fn render_page_png(&mut self, page: u32, scale: f64) -> Result<Vec<u8>, JsError> {
        self.doc.render_page_png(page, scale).map_err(js)
    }

    /// The library version (also the free function `version()`).
    #[wasm_bindgen(js_name = version)]
    pub fn engine_version() -> String {
        version()
    }
}

impl Default for Engine {
    fn default() -> Self {
        Self::new()
    }
}

/// Add a font (TTF/OTF/TTC bytes); returns the number of faces added.
#[wasm_bindgen(js_name = addFont)]
pub fn add_font(bytes: &[u8]) -> u32 {
    add_font_bytes(bytes)
}

/// The library version.
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests;
