use serde_json::Value;

use super::*;

/// DOCX bytes of the engine's sample document (built in code).
fn sample_docx() -> Vec<u8> {
    wordcraft_engine::io::save_bytes("sample.docx", &wordcraft_engine::sample::sample_document()).unwrap()
}

fn png() -> Vec<u8> {
    wordcraft_engine::render::Rendered { width: 8, height: 4, pixels: vec![255; 8 * 4 * 4] }.to_png()
}

#[test]
fn docx_flow_docx_pdf_png() {
    let mut d = Doc::new();
    d.open_bytes("sample.docx", &sample_docx()).unwrap();
    let flow: Value = serde_json::from_str(&d.flow().unwrap()).unwrap();
    assert_eq!(flow["version"], 1);
    assert!(flow["blocks"].as_array().unwrap().iter().any(|b| b["type"] == "table"));
    d.from_flow(&flow.to_string()).unwrap();
    let docx = d.save_bytes("docx").unwrap();
    assert!(docx.starts_with(b"PK"));
    assert!(d.save_bytes("pdf").unwrap().starts_with(b"%PDF"));
    assert!(d.save_bytes("html").unwrap().starts_with(b"<"));
    assert!(d.page_count() >= 1);
    assert!(d.render_page_png(0, 0.25).unwrap().starts_with(b"\x89PNG"));
    // Reopening replaces the document and gives the same flow.
    d.open_bytes("again.docx", &docx).unwrap();
    let again: Value = serde_json::from_str(&d.flow().unwrap()).unwrap();
    assert_eq!(again["blocks"], flow["blocks"]);
}

#[test]
fn staged_media_feeds_from_flow() {
    let mut d = Doc::new();
    let png = png();
    d.put_media("my-image", &png);
    let flow = r#"{"version":1,"blocks":[{"type":"image","media":"my-image","w":80,"h":40,"alt":"x","align":"center"},
        {"type":"paragraph","runs":[{"type":"text","text":"after"}]}]}"#;
    d.from_flow(flow).unwrap();
    let out: Value = serde_json::from_str(&d.flow().unwrap()).unwrap();
    let id = out["blocks"][0]["media"].as_str().unwrap().to_string();
    assert_eq!(d.media_bytes(&id).unwrap(), png);
    assert_eq!(out["media"][0]["contentType"], "image/png");
    // A flow from flow() rebuilds without staging again.
    d.from_flow(&out.to_string()).unwrap();
    assert_eq!(d.media_bytes(&id).unwrap(), png);
}

#[test]
fn execute_runs_commands() {
    let mut d = Doc::new();
    d.execute("text.insert", r#"{"text":"Hello"}"#).unwrap();
    let r: Value = serde_json::from_str(&d.execute("document.text", "").unwrap()).unwrap();
    assert_eq!(r["text"], "Hello");
    let f: Value = serde_json::from_str(&d.execute("file.flow", "{}").unwrap()).unwrap();
    assert_eq!(f["blocks"][0]["runs"][0]["text"], "Hello");
}

#[test]
fn junk_is_an_error_not_a_crash() {
    let mut d = Doc::new();
    assert!(d.open_bytes("x.docx", b"not a zip").is_err());
    assert!(d.open_bytes("x.exe", b"MZ").is_err());
    assert!(d.from_flow("{").is_err());
    assert!(d.from_flow("[]").is_err());
    assert!(d.from_flow(r#"{"version":7}"#).is_err());
    assert!(d.from_flow(r#"{"version":1,"blocks":[{"type":"image","media":"missing"}]}"#).is_err());
    assert!(d.execute("no.such.command", "{}").is_err());
    assert!(d.execute("file.new", "{oops").is_err());
    d.execute("file.new", "").unwrap();
    assert!(d.save_bytes("exe").is_err());
    assert!(d.render_page_png(99, f64::NAN).is_err());
    assert!(d.render_page_png(0, f64::INFINITY).is_ok());
    assert!(d.render_page_png(0, -3.0).is_ok());
    assert!(d.media_bytes("nope").is_err());
    assert_eq!(add_font_bytes(b"not a font"), 0);
    assert!(!version().is_empty());
}
