//! Byte and Flow JSON exchange for headless hosts (a web app running the engine in a Worker):
//! open and save documents as bytes, and convert to and from Flow JSON (`docs/flow-json.md`).
//! None of these touch the file system or open a dialog.

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::{Value, json};

use super::insert::{base64_decode, base64_encode};
use crate::flow::{self, Flow};
use crate::{CmdError, CmdResult, CommandSpec, Session, p};

/// Formats `file.saveBytes` writes.
pub const SAVE_FORMATS: &[&str] = &["docx", "pdf", "html", "md", "txt", "odt", "rtf"];

pub fn specs() -> Vec<CommandSpec> {
    vec![
        CommandSpec::new("file.openBytes", "Open from Bytes", "Agents", open_bytes_cmd)
            .params(r#"{"name": string (the extension picks the format), "data": base64}"#)
            .pure(),
        CommandSpec::new("file.saveBytes", "Save to Bytes", "Agents", save_bytes_cmd)
            .params(r#"{"format": "docx|pdf|html|md|txt|odt|rtf"} → {"data": base64, "contentType", "bytes"}"#)
            .pure(),
        CommandSpec::new("file.flow", "Document as Flow", "Agents", |s, _| {
            serde_json::to_value(flow::to_flow(&s.doc)).map_err(|e| CmdError::Failed(e.to_string()))
        })
        .params(r#"{} → Flow JSON v1 (docs/flow-json.md)"#)
        .pure(),
        CommandSpec::new("file.fromFlow", "Document from Flow", "Agents", from_flow_cmd)
            .params(r#"{"flow": Flow JSON v1, "media"?: {id: base64}}"#)
            .pure(),
        CommandSpec::new("media.bytes", "Media Bytes", "Agents", media_bytes_cmd)
            .params(r#"{"id": string} → {"data": base64, "contentType", "bytes"}"#)
            .pure(),
    ]
}

/// MIME type of a saved format.
pub fn content_type(format: &str) -> &'static str {
    match format {
        "docx" => "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        "pdf" => "application/pdf",
        "html" => "text/html",
        "md" => "text/markdown",
        "txt" => "text/plain",
        "odt" => "application/vnd.oasis.opendocument.text",
        "rtf" => "application/rtf",
        _ => "application/octet-stream",
    }
}

/// Open document bytes (`name`'s extension gives the format), replacing the session's document.
pub fn open_bytes(s: &mut Session, name: &str, bytes: &[u8]) -> Result<(), CmdError> {
    let doc = crate::io::open_bytes(name, bytes).map_err(CmdError::Failed)?;
    s.set_document(doc);
    s.path = None;
    s.flow_media.clear();
    Ok(())
}

/// The session's document as `format` bytes (one of [`SAVE_FORMATS`]).
pub fn save_bytes(s: &Session, format: &str) -> Result<Vec<u8>, CmdError> {
    let f = format.trim().trim_start_matches('.').to_ascii_lowercase();
    if !SAVE_FORMATS.contains(&f.as_str()) {
        return Err(CmdError::Params(format!("unknown format `{format}` ({})", SAVE_FORMATS.join(", "))));
    }
    crate::io::save_bytes(&format!("document.{f}"), &s.doc).map_err(CmdError::Failed)
}

/// Replace the session's document with one built from `flow`. Media ids resolve from `extra`
/// (decoded `media` param), then bytes staged in [`Session::flow_media`], then the current
/// document's own media (the ids `file.flow` gave out).
pub fn from_flow(s: &mut Session, flow: &Flow, extra: &BTreeMap<String, Arc<Vec<u8>>>) -> Result<(), CmdError> {
    let doc = {
        let lookup = |id: &str| extra.get(id).or_else(|| s.flow_media.get(id)).or_else(|| s.doc.media.get(id)).cloned();
        flow::from_flow(flow, &lookup).map_err(CmdError::Failed)?
    };
    s.set_document(doc);
    s.path = None;
    Ok(())
}

fn open_bytes_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let name = p::str(v, "name").unwrap_or("document.docx");
    let bytes = base64_decode(p::req_str(v, "data")?).ok_or_else(|| CmdError::Params("`data` is not base64".into()))?;
    open_bytes(s, name, &bytes)?;
    Ok(json!({"name": name, "paragraphs": s.doc.paragraph_count(), "words": s.doc.word_count()}))
}

fn save_bytes_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let format = p::req_str(v, "format")?;
    let bytes = save_bytes(s, format)?;
    let f = format.trim().trim_start_matches('.').to_ascii_lowercase();
    Ok(json!({"data": base64_encode(&bytes), "contentType": content_type(&f), "bytes": bytes.len()}))
}

fn from_flow_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let raw = v.get("flow").ok_or_else(|| CmdError::Params("`flow` (Flow JSON) is required".into()))?;
    let flow: Flow = match raw {
        Value::String(text) => serde_json::from_str(text),
        other => serde_json::from_value(other.clone()),
    }
    .map_err(|e| CmdError::Params(format!("flow: {e}")))?;
    let mut extra = BTreeMap::new();
    if let Some(m) = v.get("media") {
        let Some(m) = m.as_object() else { return Err(CmdError::Params("`media` must be an object of id → base64".into())) };
        for (id, data) in m {
            let b64 = data.as_str().or_else(|| data.get("data").and_then(Value::as_str)).unwrap_or("");
            let bytes = base64_decode(b64).ok_or_else(|| CmdError::Params(format!("media `{id}` is not base64")))?;
            extra.insert(id.clone(), Arc::new(bytes));
        }
    }
    from_flow(s, &flow, &extra)?;
    Ok(json!({"blocks": s.doc.body.len(), "paragraphs": s.doc.paragraph_count(), "media": s.doc.media.len()}))
}

fn media_bytes_cmd(s: &mut Session, v: &Value) -> CmdResult {
    let id = p::req_str(v, "id")?;
    let data = s.doc.media.get(id).ok_or_else(|| CmdError::Params(format!("no media `{id}`")))?;
    Ok(json!({"data": base64_encode(data), "contentType": flow::media_content_type(id, data), "bytes": data.len()}))
}
