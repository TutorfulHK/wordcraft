//! Flow JSON → Document.

use std::collections::HashMap;
use std::sync::Arc;

use wordcraft_doc::numbering::{AbstractNum, Level, LevelSuffix, ListKind, Num, levels_for};
use wordcraft_doc::para::{InlineObject, LINE_BREAK, PAGE_BREAK};
use wordcraft_doc::props::{Border, BorderStyle, Borders, CellProps, CharProps, Highlight, LineSpacing, NumRef, ParaProps, RowProps, VMerge};
use wordcraft_doc::section::NumFormat;
use wordcraft_doc::table::{MAX_COLS, MAX_ROWS};
use wordcraft_doc::{Blocks, Document, Rgb, para_block};
use wordcraft_formats::model::{self as fm, Fmt};

use super::schema::*;
use super::{INDENT_STEP, MAX_DEPTH, MAX_LEVEL, TASK_DONE, TASK_OPEN};

/// Largest page side and picture size, points (22").
const MAX_PT: f64 = 1584.0;
/// Rows a cell may span.
const MAX_ROWSPAN: u32 = 1000;

/// Where a flow's media bytes come from, by media id.
pub type MediaSource<'a> = &'a dyn Fn(&str) -> Option<Arc<Vec<u8>>>;

/// Build a document from a flow. Fails on an unknown version or a media id `media` can't
/// resolve; out-of-range numbers are clamped.
pub fn from_flow(flow: &Flow, media: MediaSource<'_>) -> Result<Document, String> {
    if flow.version != VERSION {
        return Err(format!("flow version {} (this engine reads version {VERSION})", flow.version));
    }
    let mut doc = Document::new();
    doc.numbering = Default::default();
    doc.styles = super::reference_styles();
    let s = &mut doc.last_section;
    let side = |v: f64, d: f32| if v.is_finite() && v >= 72.0 { v.min(MAX_PT) as f32 } else { d };
    s.page_w = side(flow.page.w, s.page_w);
    s.page_h = side(flow.page.h, s.page_h);
    s.landscape = s.page_w > s.page_h;
    let m = &flow.page.margins;
    let margin = |v: f64, side: f32| if v.is_finite() { (v.max(0.0) as f32).min(side * 0.4) } else { 72.0f32.min(side * 0.4) };
    s.margin_top = margin(m.t, s.page_h);
    s.margin_bottom = margin(m.b, s.page_h);
    s.margin_left = margin(m.l, s.page_w);
    s.margin_right = margin(m.r, s.page_w);
    let mut w = Writer { doc: &mut doc, media, keys: HashMap::new() };
    let body = w.blocks(&flow.blocks, 0)?;
    doc.body = body;
    doc.ensure_nonempty();
    Ok(doc)
}

/// The list a block sequence sits in: its numbering instance, level and the levels' text indents.
#[derive(Clone, Copy)]
struct InList {
    num: u32,
    level: u8,
    indents: [f32; 9],
}

impl InList {
    fn indent(&self) -> f32 {
        self.indents.get(usize::from(self.level)).copied().unwrap_or(0.0)
    }
}

struct Writer<'a, 'm> {
    doc: &'a mut Document,
    media: MediaSource<'m>,
    /// Flow media id → document media key.
    keys: HashMap<String, String>,
}

impl Writer<'_, '_> {
    fn blocks(&mut self, blocks: &[Block], depth: usize) -> Result<Blocks, String> {
        let mut out = Blocks::new();
        let mut pending = false;
        for b in blocks {
            self.block(b, depth, None, &mut out, &mut pending)?;
        }
        if pending {
            out.push(para_block(break_para()));
        }
        Ok(out)
    }

    /// One block into `out`. `pending`: a page break waits for the next paragraph.
    fn block(&mut self, b: &Block, depth: usize, list: Option<InList>, out: &mut Blocks, pending: &mut bool) -> Result<(), String> {
        let continuation = |mut p: wordcraft_doc::Paragraph| {
            if let Some(l) = list {
                p.props.indent_left = Some(l.indent());
                if p.props.style.is_none() {
                    p.props.style = Some("ListParagraph".into());
                }
            }
            p
        };
        match b {
            Block::PageBreak => {
                if std::mem::replace(pending, true) {
                    out.push(para_block(break_para()));
                }
            }
            Block::Paragraph(p) => {
                let para = continuation(self.paragraph(p, list.is_some())?);
                out.push(para_block(with_break(para, pending)));
            }
            Block::Image(img) => {
                let para = continuation(self.image_para(img)?);
                out.push(para_block(with_break(para, pending)));
            }
            Block::Rule => out.push(para_block(with_break(rule_para(), pending))),
            Block::List(l) => self.list(l, list, depth, out, pending)?,
            Block::Table(t) => {
                if std::mem::take(pending) {
                    out.push(para_block(break_para()));
                }
                if depth >= MAX_DEPTH {
                    // Too deep: the cells' content, in order.
                    for c in t.rows.iter().flat_map(|r| &r.cells) {
                        out.extend(self.blocks(&c.blocks, depth)?);
                    }
                } else if let Some(mut tb) = self.table(t, depth)? {
                    if let Some(l) = list {
                        tb.props.indent = Some(l.indent());
                    }
                    out.push(Arc::new(wordcraft_doc::Block::Table(tb)));
                }
            }
        }
        Ok(())
    }

    fn list(&mut self, l: &List, parent: Option<InList>, depth: usize, out: &mut Blocks, pending: &mut bool) -> Result<(), String> {
        let here = match parent {
            Some(p) => InList { level: (p.level + 1).min(MAX_LEVEL), ..p },
            None => self.new_list(l),
        };
        for item in &l.items {
            let mut numbered = false;
            for b in &item.blocks {
                match b {
                    Block::Paragraph(p) if !numbered => {
                        let mut para = self.paragraph(p, true)?;
                        number(&mut para, here, item.checked);
                        out.push(para_block(with_break(para, pending)));
                        numbered = true;
                    }
                    Block::Image(img) if !numbered => {
                        let mut para = self.image_para(img)?;
                        number(&mut para, here, item.checked);
                        out.push(para_block(with_break(para, pending)));
                        numbered = true;
                    }
                    Block::PageBreak => self.block(b, depth, Some(here), out, pending)?,
                    _ => {
                        if !numbered {
                            // An item that starts with a nested list or a table: an empty item paragraph.
                            let mut para = wordcraft_doc::Paragraph::new();
                            number(&mut para, here, item.checked);
                            out.push(para_block(with_break(para, pending)));
                            numbered = true;
                        }
                        self.block(b, depth, Some(here), out, pending)?;
                    }
                }
            }
            if !numbered {
                let mut para = wordcraft_doc::Paragraph::new();
                number(&mut para, here, item.checked);
                out.push(para_block(with_break(para, pending)));
            }
        }
        Ok(())
    }

    /// A numbering instance for a top-level list: each level takes the kind (and start) of the
    /// first list met at that depth.
    fn new_list(&mut self, l: &List) -> InList {
        #[derive(Clone, Copy, PartialEq)]
        enum K {
            Ordered,
            Bullet,
            Task,
        }
        fn walk(l: &List, d: usize, seen: &mut [Option<(K, u32)>; 9]) {
            let Some(slot) = seen.get_mut(d) else { return };
            if slot.is_none() {
                let k = if l.items.iter().any(|i| i.checked.is_some()) {
                    K::Task
                } else if l.ordered {
                    K::Ordered
                } else {
                    K::Bullet
                };
                *slot = Some((k, l.start.unwrap_or(1)));
            }
            for b in l.items.iter().flat_map(|i| &i.blocks) {
                if let Block::List(sub) = b {
                    walk(sub, d + 1, seen);
                }
            }
        }
        let mut seen = [None; 9];
        walk(l, 0, &mut seen);
        let first = seen.first().copied().flatten().map(|s| s.0).unwrap_or(K::Bullet);
        let numbered = levels_for(ListKind::Numbered);
        let bullets = levels_for(ListKind::Bullet);
        let mut indents = [0.0f32; 9];
        let levels: Vec<Level> = (0..9)
            .map(|i| {
                let (k, start) = seen.get(i).copied().flatten().unwrap_or((first, 1));
                let mut lv = match k {
                    K::Ordered => numbered.get(i).cloned().unwrap_or_default(),
                    K::Bullet => bullets.get(i).cloned().unwrap_or_default(),
                    K::Task => {
                        let base = bullets.get(i).cloned().unwrap_or_default();
                        Level { format: NumFormat::None, text: String::new(), suffix: LevelSuffix::Nothing, chr: CharProps::default(), ..base }
                    }
                };
                if k == K::Ordered {
                    lv.start = start.min(32_767);
                }
                if let Some(x) = indents.get_mut(i) {
                    *x = lv.indent;
                }
                lv
            })
            .collect();
        let numbering = &mut self.doc.numbering;
        let aid = numbering.abstracts.iter().map(|a| a.id.saturating_add(1)).max().unwrap_or(0);
        numbering.abstracts.push(AbstractNum { id: aid, name: None, levels });
        let nid = numbering.nums.iter().map(|n| n.id.saturating_add(1)).max().unwrap_or(1).max(1);
        numbering.nums.push(Num { id: nid, abstract_id: aid, start_overrides: Vec::new() });
        InList { num: nid, level: 0, indents }
    }

    fn paragraph(&mut self, p: &Paragraph, in_list: bool) -> Result<wordcraft_doc::Paragraph, String> {
        let mut out = wordcraft_doc::Paragraph::new();
        let style = super::style_id(p.style, p.heading_level, in_list);
        let rr = self.doc.styles.resolve_para(&ParaProps { style: style.clone(), ..Default::default() });
        out.props.style = style;
        let a = match p.align {
            Align::Left => wordcraft_doc::Align::Left,
            Align::Center => wordcraft_doc::Align::Center,
            Align::Right => wordcraft_doc::Align::Right,
            Align::Justify => wordcraft_doc::Align::Justify,
        };
        if a != rr.align {
            out.props.align = Some(a);
        }
        if !in_list && p.indent > 0 {
            out.props.indent_left = Some(rr.indent_left + INDENT_STEP * f32::from(p.indent.min(MAX_LEVEL)));
        }
        let spacing = |v: Option<f64>| v.filter(|x| x.is_finite()).map(|x| x.clamp(0.0, MAX_PT) as f32);
        out.props.space_before = spacing(p.space_before);
        out.props.space_after = spacing(p.space_after);
        out.props.line_spacing = p.line_height.filter(|x| x.is_finite() && *x > 0.0).map(|x| LineSpacing::Multiple(x.clamp(0.06, 132.0) as f32));
        let in_code = p.style == ParaStyle::Code && p.heading_level.is_none();
        for i in &p.runs {
            let end = out.len();
            match i {
                Inline::Text(t) => {
                    let text: String = fm::clean_text(&t.text).chars().filter(|c| *c != PAGE_BREAK && *c != '\u{000E}').collect();
                    let mut cp = char_props(t);
                    if in_code && cp.font.as_deref() == Some(fm::MONO_FONT) {
                        cp.font = None;
                    }
                    out.insert_text(end, &text, &cp).map_err(|e| e.to_string())?;
                }
                Inline::Break => {
                    let mut b = [0u8; 4];
                    out.insert_text(end, LINE_BREAK.encode_utf8(&mut b), &CharProps::default()).map_err(|e| e.to_string())?;
                }
                Inline::Image(img) => {
                    let obj = self.image(&img.media, img.w, img.h, &img.alt)?;
                    out.insert_object(end, obj, &CharProps::default()).map_err(|e| e.to_string())?;
                }
            }
        }
        Ok(out)
    }

    fn image_para(&mut self, img: &ImageBlock) -> Result<wordcraft_doc::Paragraph, String> {
        let mut out = wordcraft_doc::Paragraph::new();
        out.props.align = match img.align {
            Align::Left => None,
            Align::Center => Some(wordcraft_doc::Align::Center),
            Align::Right => Some(wordcraft_doc::Align::Right),
            Align::Justify => Some(wordcraft_doc::Align::Justify),
        };
        let obj = self.image(&img.media, img.w, img.h, &img.alt)?;
        out.insert_object(0, obj, &CharProps::default()).map_err(|e| e.to_string())?;
        Ok(out)
    }

    /// An inline picture of media `id`, `w` × `h` points (natural size at 96 dpi when unset).
    fn image(&mut self, id: &str, w: f64, h: f64, alt: &str) -> Result<InlineObject, String> {
        let key = match self.keys.get(id) {
            Some(k) => k.clone(),
            None => {
                let bytes = (self.media)(id).ok_or_else(|| format!("no media `{id}` (pass it in `media` or stage it first)"))?;
                let ext = fm::media_ext(id, &bytes);
                let key = self.doc.add_media(bytes.to_vec(), &ext);
                self.keys.insert(id.to_string(), key.clone());
                key
            }
        };
        let (nw, nh) =
            self.doc.media.get(&key).and_then(|d| fm::image_px(d)).map(|(a, b)| (a as f32 * 0.75, b as f32 * 0.75)).unwrap_or((72.0, 72.0));
        let size = |v: f64, natural: f32| if v.is_finite() && v > 0.0 { v.min(MAX_PT) as f32 } else { natural.clamp(1.0, MAX_PT as f32) };
        let (w, h) = (size(w, nw).max(1.0), size(h, nh).max(1.0));
        Ok(InlineObject::Image { media: key, w, h, alt: alt.to_string(), float: Default::default(), crop: [0.0; 4] })
    }

    fn table(&mut self, t: &Table, depth: usize) -> Result<Option<wordcraft_doc::Table>, String> {
        // Place cells on the grid: cells covered by a row span from above become `Continue` cells.
        enum Slot<'c> {
            Real(&'c Cell, usize, bool),
            Covered(usize),
            Gap(usize),
        }
        let mut carry: Vec<(u32, usize)> = Vec::new();
        let mut rows: Vec<(bool, Vec<Slot<'_>>)> = Vec::new();
        let mut cols = 0usize;
        for r in t.rows.iter().take(MAX_ROWS) {
            let mut slots = Vec::new();
            let mut cells = r.cells.iter();
            let mut g = 0usize;
            while g < MAX_COLS {
                if let Some((rem, span)) = carry.get_mut(g)
                    && *rem > 0
                {
                    *rem -= 1;
                    let span = (*span).clamp(1, MAX_COLS - g);
                    slots.push(Slot::Covered(span));
                    g += span;
                    continue;
                }
                match cells.next() {
                    Some(c) => {
                        let span = (c.colspan.clamp(1, 63) as usize).min(MAX_COLS - g);
                        let rs = c.rowspan.clamp(1, MAX_ROWSPAN);
                        if rs > 1 {
                            if carry.len() < g + 1 {
                                carry.resize(g + 1, (0, 1));
                            }
                            if let Some(slot) = carry.get_mut(g) {
                                *slot = (rs - 1, span);
                            }
                        }
                        slots.push(Slot::Real(c, span, rs > 1));
                        g += span;
                    }
                    None => {
                        // Row spans further right still need their covered cells.
                        match carry.iter().skip(g).position(|c| c.0 > 0) {
                            Some(0) | None => break,
                            Some(gap) => {
                                slots.push(Slot::Gap(gap));
                                g += gap;
                            }
                        }
                    }
                }
            }
            cols = cols.max(g);
            rows.push((r.header, slots));
        }
        if rows.is_empty() {
            return Ok(None);
        }
        let cols = cols.clamp(1, MAX_COLS);
        let width = self.doc.last_section.text_width().max(72.0);
        let mut tb = wordcraft_doc::Table::new(rows.len(), cols, width);
        if t.col_widths.len() == cols && t.col_widths.iter().all(|w| w.is_finite() && *w > 1.0) {
            tb.grid = t.col_widths.iter().map(|w| w.clamp(6.0, MAX_PT) as f32).collect();
        }
        tb.rows.clear();
        for (header, slots) in rows {
            let mut row = wordcraft_doc::Row { props: RowProps { header, ..Default::default() }, cells: Vec::new() };
            let mut g = 0usize;
            for s in slots {
                let span = match &s {
                    Slot::Real(_, n, _) | Slot::Covered(n) | Slot::Gap(n) => *n,
                };
                let w: f32 = tb.grid.get(g..(g + span).min(tb.grid.len())).map(|s| s.iter().sum()).unwrap_or(72.0);
                g += span;
                let mut cell = wordcraft_doc::Cell::empty();
                cell.props = CellProps { width: Some(w), span: span as u32, ..Default::default() };
                match s {
                    Slot::Real(c, _, restart) => {
                        cell.props.vmerge = if restart { VMerge::Restart } else { VMerge::None };
                        cell.props.shading = c.background.as_deref().and_then(fm::parse_color);
                        let mut blocks = self.blocks(&c.blocks, depth + 1)?;
                        if blocks.is_empty() || matches!(blocks.last().map(|b| &**b), Some(wordcraft_doc::Block::Table(_))) {
                            blocks.push(para_block(wordcraft_doc::Paragraph::new()));
                        }
                        cell.blocks = blocks;
                    }
                    Slot::Covered(_) => cell.props.vmerge = VMerge::Continue,
                    Slot::Gap(_) => {}
                }
                row.cells.push(cell);
            }
            if row.cells.is_empty() {
                row.cells.push(wordcraft_doc::Cell::empty());
            }
            tb.rows.push(row);
        }
        Ok(Some(tb))
    }
}

/// Make `p` item text of list `l` (with a task checkbox when `checked` is set).
fn number(p: &mut wordcraft_doc::Paragraph, l: InList, checked: Option<bool>) {
    p.props.numbering = Some(NumRef { num: l.num, level: l.level });
    if p.props.style.is_none() {
        p.props.style = Some("ListParagraph".into());
    }
    p.props.indent_left = None;
    if let Some(c) = checked {
        let mark = format!("{} ", if c { TASK_DONE } else { TASK_OPEN });
        let props = p.runs.first().map(|r| r.props.clone()).unwrap_or_default();
        let _ = p.insert_text(0, &mark, &props);
    }
}

fn with_break(mut p: wordcraft_doc::Paragraph, pending: &mut bool) -> wordcraft_doc::Paragraph {
    if std::mem::take(pending) {
        p.props.page_break_before = Some(true);
    }
    p
}

/// A paragraph holding only a page break.
fn break_para() -> wordcraft_doc::Paragraph {
    let mut b = [0u8; 4];
    wordcraft_doc::Paragraph::with_text(PAGE_BREAK.encode_utf8(&mut b), CharProps::default())
}

/// A horizontal rule: an empty paragraph with a bottom border.
fn rule_para() -> wordcraft_doc::Paragraph {
    let mut p = wordcraft_doc::Paragraph::new();
    p.props.borders = Some(Borders {
        bottom: Some(Border { style: BorderStyle::Single, width: 0.75, color: Some(Rgb(0xA0, 0xA0, 0xA0)), space: 1.0 }),
        ..Default::default()
    });
    p
}

fn char_props(t: &Text) -> CharProps {
    let f = Fmt {
        bold: t.bold,
        italic: t.italic,
        underline: t.underline,
        strike: t.strike,
        code: t.code,
        sup: t.sup,
        sub: t.sub && !t.sup,
        link: t.link.clone().filter(|l| !l.is_empty()),
        color: t.color.as_deref().and_then(fm::parse_color),
        size: t.size.filter(|s| s.is_finite()).map(|s| s as f32),
        font: t.font.clone().filter(|f| !f.trim().is_empty()),
        background: None,
    };
    let mut cp = fm::char_props(&f);
    if let Some(c) = t.highlight.as_deref().and_then(fm::parse_color) {
        match Highlight::ALL.iter().find(|h| h.rgb() == Some(c)) {
            Some(h) => cp.highlight = Some(*h),
            None => cp.shading = Some(c),
        }
    }
    cp
}
