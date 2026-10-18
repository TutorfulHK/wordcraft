use std::sync::Arc;

use serde_json::{Value, json};

use super::schema::*;
use super::*;
use crate::Session;

fn png(w: u32, h: u32) -> Vec<u8> {
    wordcraft_render::Rendered { width: w, height: h, pixels: vec![200; (w * h * 4) as usize] }.to_png()
}

fn no_media(_: &str) -> Option<Arc<Vec<u8>>> {
    None
}

/// Every construct the schema has, as a host would write it.
fn rich_flow() -> Value {
    let t = |s: &str| json!({"type": "text", "text": s});
    let para = |runs: Value| json!({"type": "paragraph", "runs": runs});
    json!({
        "version": 1,
        "page": {"w": 595.3, "h": 841.9, "margins": {"t": 54, "r": 60, "b": 54, "l": 60}},
        "blocks": [
            {"type": "paragraph", "style": "title", "runs": [t("Report")]},
            {"type": "paragraph", "headingLevel": 2, "runs": [t("Intro")]},
            {"type": "paragraph", "align": "center", "indent": 2, "lineHeight": 1.5, "spaceBefore": 6, "spaceAfter": 18, "runs": [
                t("Plain "),
                {"type": "text", "text": "bold", "bold": true},
                {"type": "text", "text": " red", "italic": true, "color": "#c00000", "font": "Georgia", "size": 14},
                {"type": "break"},
                {"type": "text", "text": "mark", "highlight": "#ffff00"},
                {"type": "text", "text": "tint", "highlight": "#a0c4ff"},
                {"type": "text", "text": "x", "sup": true},
                {"type": "text", "text": "2", "sub": true},
                {"type": "text", "text": "gone", "strike": true, "underline": true},
                {"type": "text", "text": "link", "link": "https://example.com/"},
                {"type": "text", "text": "let x", "code": true},
                {"type": "image", "media": "dot", "w": 12, "h": 12, "alt": "dot"}
            ]},
            {"type": "paragraph", "style": "quote", "runs": [t("Quoted")]},
            {"type": "paragraph", "style": "code", "runs": [t("fn main() {}")]},
            {"type": "rule"},
            {"type": "list", "ordered": false, "items": [
                {"blocks": [para(json!([t("one")])), {"type": "list", "ordered": true, "items": [
                    {"blocks": [para(json!([t("one.a")]))]},
                    {"blocks": [para(json!([t("one.b")])), para(json!([t("more of one.b")]))]}
                ]}]},
                {"blocks": [para(json!([t("two")]))]}
            ]},
            {"type": "list", "ordered": true, "start": 3, "items": [
                {"blocks": [para(json!([t("three")]))]},
                {"blocks": [para(json!([t("four")]))]}
            ]},
            {"type": "list", "ordered": false, "items": [
                {"checked": true, "blocks": [para(json!([t("done")]))]},
                {"checked": false, "blocks": [para(json!([t("todo")]))]}
            ]},
            {"type": "pageBreak"},
            {"type": "table", "colWidths": [100, 120, 140], "rows": [
                {"header": true, "cells": [
                    {"blocks": [para(json!([t("A")]))], "background": "#d9e2f3"},
                    {"colspan": 2, "blocks": [para(json!([t("B+C")]))]}
                ]},
                {"cells": [
                    {"rowspan": 2, "blocks": [para(json!([t("tall")]))]},
                    {"blocks": [para(json!([t("b2")]))]},
                    {"blocks": [para(json!([t("c2")]))]}
                ]},
                {"cells": [
                    {"blocks": [para(json!([t("b3")]))]},
                    {"blocks": [{"type": "list", "ordered": false, "items": [{"blocks": [para(json!([t("in cell")]))]}]}]}
                ]}
            ]},
            {"type": "image", "media": "pic", "w": 150, "h": 75, "alt": "A picture", "align": "center"},
            para(json!([t("The end")]))
        ]
    })
}

fn media_for(id: &str) -> Option<Arc<Vec<u8>>> {
    match id {
        "dot" => Some(Arc::new(png(4, 4))),
        "pic" => Some(Arc::new(png(40, 20))),
        _ => None,
    }
}

fn build(v: &Value) -> wordcraft_doc::Document {
    let f: Flow = serde_json::from_value(v.clone()).unwrap();
    from_flow(&f, &media_for).unwrap()
}

fn docx_round_trip(doc: &wordcraft_doc::Document) -> wordcraft_doc::Document {
    let bytes = crate::io::save_bytes("t.docx", doc).unwrap();
    crate::io::open_bytes("t.docx", &bytes).unwrap()
}

fn blocks_json(f: &Flow) -> Value {
    serde_json::to_value(&f.blocks).unwrap()
}

#[test]
fn rich_flow_maps_every_construct() {
    let doc = build(&rich_flow());
    let f = to_flow(&doc);
    let b = blocks_json(&f);
    assert_eq!(f.version, 1);
    assert_eq!(f.page.w, 595.3);
    assert_eq!(f.page.margins.l, 60.0);
    assert_eq!(b[0]["style"], "title");
    assert_eq!(b[1]["headingLevel"], 2);
    let p = &b[2];
    assert_eq!(p["align"], "center");
    assert_eq!(p["indent"], 2);
    assert_eq!(p["lineHeight"], 1.5);
    assert_eq!(p["spaceBefore"], 6.0);
    assert_eq!(p["spaceAfter"], 18.0);
    let runs = p["runs"].as_array().unwrap();
    let find = |s: &str| runs.iter().find(|r| r["text"] == s).cloned().unwrap_or(Value::Null);
    assert_eq!(find("bold")["bold"], true);
    assert_eq!(find(" red")["color"], "#c00000");
    assert_eq!(find(" red")["font"], "Georgia");
    assert_eq!(find(" red")["size"], 14.0);
    assert_eq!(find("mark")["highlight"], "#ffff00");
    assert_eq!(find("tint")["highlight"], "#a0c4ff");
    assert_eq!(find("x")["sup"], true);
    assert_eq!(find("2")["sub"], true);
    assert_eq!(find("gone")["strike"], true);
    assert_eq!(find("gone")["underline"], true);
    assert_eq!(find("link")["link"], "https://example.com/");
    assert_eq!(find("let x")["code"], true);
    assert!(runs.iter().any(|r| r["type"] == "break"));
    let img = runs.iter().find(|r| r["type"] == "image").unwrap();
    assert_eq!(img["w"], 12.0);
    assert_eq!(b[3]["style"], "quote");
    assert_eq!(b[4]["style"], "code");
    assert_eq!(b[4]["runs"][0]["code"], false, "a code paragraph's text isn't also inline code");
    assert_eq!(b[5]["type"], "rule");
    // Nested list with a two-paragraph item.
    let l = &b[6];
    assert_eq!(l["type"], "list");
    assert_eq!(l["ordered"], false);
    assert_eq!(l["items"].as_array().unwrap().len(), 2);
    let nested = &l["items"][0]["blocks"][1];
    assert_eq!(nested["type"], "list");
    assert_eq!(nested["ordered"], true);
    assert_eq!(nested["items"][1]["blocks"].as_array().unwrap().len(), 2);
    assert_eq!(nested["items"][1]["blocks"][1]["runs"][0]["text"], "more of one.b");
    assert_eq!(b[7]["start"], 3);
    assert_eq!(b[8]["items"][0]["checked"], true);
    assert_eq!(b[8]["items"][1]["checked"], false);
    assert_eq!(b[8]["items"][0]["blocks"][0]["runs"][0]["text"], "done");
    assert_eq!(b[9]["type"], "pageBreak");
    let t = &b[10];
    assert_eq!(t["type"], "table");
    assert_eq!(t["colWidths"], json!([100.0, 120.0, 140.0]));
    assert_eq!(t["rows"][0]["header"], true);
    assert_eq!(t["rows"][0]["cells"][0]["background"], "#d9e2f3");
    assert_eq!(t["rows"][0]["cells"][1]["colspan"], 2);
    assert_eq!(t["rows"][1]["cells"][0]["rowspan"], 2);
    assert_eq!(t["rows"][2]["cells"].as_array().unwrap().len(), 2, "covered cells are left out");
    assert_eq!(t["rows"][2]["cells"][1]["blocks"][0]["type"], "list");
    assert_eq!(b[11]["type"], "image");
    assert_eq!(b[11]["align"], "center");
    assert_eq!(b[11]["alt"], "A picture");
    assert_eq!(b[12]["runs"][0]["text"], "The end");
    assert_eq!(b.as_array().unwrap().len(), 13);
    // Media list: both pictures, with pixel sizes.
    assert_eq!(f.media.len(), 2);
    assert!(f.media.iter().all(|m| m.content_type == "image/png"));
    assert!(f.media.iter().any(|m| m.w == 40 && m.h == 20));
}

#[test]
fn flow_docx_flow_is_stable() {
    let doc = build(&rich_flow());
    let f1 = to_flow(&doc);
    let f2 = to_flow(&docx_round_trip(&doc));
    assert_eq!(serde_json::to_value(&f1).unwrap(), serde_json::to_value(&f2).unwrap());
    // And flow → doc → flow (media from the document itself) is stable too.
    let f3 = to_flow(&from_flow(&f2, &|id: &str| doc.media.get(id).cloned()).unwrap());
    assert_eq!(serde_json::to_value(&f2).unwrap(), serde_json::to_value(&f3).unwrap());
}

#[test]
fn docx_flow_docx_flow_is_stable_for_documents_built_in_code() {
    for doc in [crate::sample::sample_document(), crate::sample::report(), crate::sample::resume(), crate::sample::letter()] {
        let d1 = docx_round_trip(&doc);
        let f1 = to_flow(&d1);
        let d2 = docx_round_trip(&from_flow(&f1, &|id: &str| d1.media.get(id).cloned()).unwrap());
        let f2 = to_flow(&d2);
        assert_eq!(serde_json::to_value(&f1).unwrap(), serde_json::to_value(&f2).unwrap());
    }
}

#[test]
fn word_numbering_maps_to_lists() {
    // A list as Word writes it: one numbering instance, levels by paragraph, an interruption, and
    // the same list continuing (so the second run starts at 3).
    use wordcraft_doc::props::NumRef;
    use wordcraft_doc::{Paragraph, para_block};
    let mut doc = wordcraft_doc::Document::new();
    let num = doc.numbering.add_list(wordcraft_doc::ListKind::Numbered);
    let item = |t: &str, level: u8| {
        let mut p = Paragraph::with_text(t, Default::default()).styled("ListParagraph");
        p.props.numbering = Some(NumRef { num, level });
        para_block(p)
    };
    doc.body = vec![
        item("a", 0),
        item("a.i", 1),
        item("b", 0),
        para_block(Paragraph::with_text("between", Default::default())),
        item("c", 0),
        item("d.deep", 2),
    ];
    let b = blocks_json(&to_flow(&doc));
    assert_eq!(b[0]["type"], "list");
    assert_eq!(b[0]["items"].as_array().unwrap().len(), 2);
    assert_eq!(b[0]["items"][0]["blocks"][1]["type"], "list");
    assert_eq!(b[1]["runs"][0]["text"], "between");
    assert_eq!(b[2]["start"], 3);
    assert_eq!(b[2]["items"][0]["blocks"][1]["items"][0]["blocks"][0]["runs"][0]["text"], "d.deep");
}

#[test]
fn hostile_flows_error_or_clamp() {
    let bad = [
        json!({"version": 2, "blocks": []}),
        json!({"blocks": []}),
        json!({"version": 1, "blocks": [{"type": "nope"}]}),
        json!({"version": 1, "blocks": [{"type": "image", "media": "missing"}]}),
    ];
    for v in bad {
        let r = serde_json::from_value::<Flow>(v.clone()).map_err(|e| e.to_string()).and_then(|f| from_flow(&f, &no_media).map(|_| ()));
        assert!(r.is_err(), "{v}");
    }
    let mut deep = json!({"type": "paragraph", "runs": [{"type": "text", "text": "x"}]});
    for _ in 0..30 {
        deep = json!({"type": "list", "ordered": true, "start": u32::MAX, "items": [{"blocks": [deep], "checked": true}]});
    }
    let mut table = json!({"type": "paragraph"});
    for _ in 0..20 {
        table = json!({"type": "table", "colWidths": [-1, f64::MAX], "rows": [{"cells": [
            {"colspan": 4_000_000_000u32, "rowspan": 4_000_000_000u32, "blocks": [table], "background": "nonsense"}, {}, {"rowspan": 0}
        ]}, {"cells": []}, {}]});
    }
    let weird = json!({"version": 1, "page": {"w": -5, "h": 1e300, "margins": {"t": 1e9, "l": -3}}, "blocks": [
        deep, table,
        {"type": "paragraph", "headingLevel": 99, "indent": 255, "lineHeight": -1, "spaceBefore": 1e30, "runs": [
            {"type": "text", "text": "\u{0}\u{c}\u{fffc}ok", "size": -4, "color": "#zzz", "highlight": "rgb(1,2)", "font": "  ", "sup": true, "sub": true},
            {"type": "image", "media": "dot", "w": -1, "h": 1e12}
        ]},
        {"type": "pageBreak"}, {"type": "pageBreak"}, {"type": "list", "items": []}, {"type": "table", "rows": []}
    ]});
    let f: Flow = serde_json::from_value(weird).unwrap();
    let doc = from_flow(&f, &media_for).unwrap();
    let back = to_flow(&doc);
    assert!(!back.blocks.is_empty());
    let mut s = Session::new(doc);
    let _ = s.layout();
    assert!(crate::io::save_bytes("x.docx", &s.doc).is_ok());
}

#[test]
fn commands_open_save_flow_and_media() {
    let mut s = Session::new(wordcraft_doc::Document::new());
    let r = |s: &mut Session, id: &str, v: Value| s.run(id, &v);
    // fromFlow with base64 media in the params.
    let mut flow = rich_flow();
    let pic = crate::cmd::insert::base64_encode(&png(40, 20));
    let dot = crate::cmd::insert::base64_encode(&png(4, 4));
    assert!(r(&mut s, "file.fromFlow", json!({"flow": flow.clone()})).is_err(), "media missing");
    r(&mut s, "file.fromFlow", json!({"flow": flow.clone(), "media": {"pic": pic, "dot": {"data": dot}}})).unwrap();
    let f = r(&mut s, "file.flow", json!({})).unwrap();
    assert_eq!(f["version"], 1);
    // Media ids from file.flow resolve against the open document, so the same flow rebuilds.
    r(&mut s, "file.fromFlow", json!({"flow": f.to_string()})).unwrap();
    let id = f["media"][0]["id"].as_str().unwrap();
    let m = r(&mut s, "media.bytes", json!({"id": id})).unwrap();
    assert_eq!(m["contentType"], "image/png");
    assert!(r(&mut s, "media.bytes", json!({"id": "nope"})).is_err());
    // Staged bytes (the wasm putMedia path) also resolve.
    s.flow_media.insert("pic".into(), Arc::new(png(40, 20)));
    s.flow_media.insert("dot".into(), Arc::new(png(4, 4)));
    flow["blocks"].as_array_mut().unwrap().truncate(13);
    r(&mut s, "file.fromFlow", json!({"flow": flow})).unwrap();
    // Save in every format, reopen the docx.
    for fmt in crate::cmd::exchange::SAVE_FORMATS {
        let out = r(&mut s, "file.saveBytes", json!({"format": fmt})).unwrap();
        assert!(out["bytes"].as_u64().unwrap() > 0, "{fmt}");
    }
    let pdf = r(&mut s, "file.saveBytes", json!({"format": "pdf"})).unwrap();
    assert!(crate::cmd::insert::base64_decode(pdf["data"].as_str().unwrap()).unwrap().starts_with(b"%PDF"));
    let docx = r(&mut s, "file.saveBytes", json!({"format": "docx"})).unwrap();
    assert_eq!(docx["contentType"], "application/vnd.openxmlformats-officedocument.wordprocessingml.document");
    r(&mut s, "file.openBytes", json!({"name": "again.docx", "data": docx["data"]})).unwrap();
    assert!(s.flow_media.is_empty(), "opening clears staged media");
    assert_eq!(r(&mut s, "file.flow", json!({})).unwrap()["blocks"], f["blocks"]);
    // Plain text opens too; junk errors.
    r(&mut s, "file.openBytes", json!({"name": "a.txt", "data": crate::cmd::insert::base64_encode(b"hello\nworld")})).unwrap();
    assert_eq!(s.doc.paragraph_count(), 2);
    assert!(s.ui_requests.is_empty(), "programmatic calls never open dialogs");
}

#[test]
fn exchange_commands_survive_junk() {
    let ids = ["file.openBytes", "file.saveBytes", "file.flow", "file.fromFlow", "media.bytes"];
    let junk = [
        json!(null),
        json!({}),
        json!({"name": 5, "data": "!!!", "format": "exe", "flow": [], "media": "x", "id": []}),
        json!({"name": "x.docx", "data": "UEsDBA==", "format": "PDF", "flow": "{", "media": {"a": 5}, "id": "image1.png"}),
        json!({"name": "x.rtf", "data": "e1xydGY=", "flow": {"version": 1, "blocks": [{"type": "list", "items": [{}]}]}, "media": {}}),
        json!({"flow": {"version": 1, "blocks": [{"type": "table", "rows": [{"cells": [{"rowspan": 9}]}]}]}}),
    ];
    for id in ids {
        for j in &junk {
            let mut s = Session::new(crate::sample::sample_document());
            let _ = s.run(id, j);
            s.clamp_selection();
            let _ = s.layout();
        }
    }
}
