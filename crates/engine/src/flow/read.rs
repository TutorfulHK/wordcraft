//! Document → Flow JSON.

use std::collections::BTreeMap;
use std::iter::Peekable;
use std::vec::IntoIter;

use wordcraft_doc::numbering::Counters;
use wordcraft_doc::para::{COLUMN_BREAK, InlineObject, LINE_BREAK, OBJ, PAGE_BREAK};
use wordcraft_doc::props::{LineSpacing, ParaProps, Rgb, VMerge};
use wordcraft_doc::resolve::ResolvedPara;
use wordcraft_doc::styles::StyleSheet;
use wordcraft_doc::{Blocks, Document};
use wordcraft_formats::model::{self as fm, Kind};

use super::schema::*;
use super::{INDENT_STEP, MAX_DEPTH, MAX_LEVEL, TASK_DONE, TASK_OPEN};

/// The active document as Flow JSON v1.
pub fn to_flow(doc: &Document) -> Flow {
    let mut r = Reader { doc, refs: super::reference_styles(), counters: Counters::default(), media: BTreeMap::new() };
    let blocks = r.blocks(&doc.body, 0);
    let s = &doc.last_section;
    Flow {
        version: VERSION,
        page: Page {
            w: pt(s.page_w),
            h: pt(s.page_h),
            margins: Margins { t: pt(s.margin_top), r: pt(s.margin_right), b: pt(s.margin_bottom), l: pt(s.margin_left) },
        },
        blocks,
        media: r.media.into_values().collect(),
    }
}

/// Points, rounded to 1/100.
pub(super) fn pt(v: f32) -> f64 {
    if v.is_finite() { (f64::from(v) * 100.0).round() / 100.0 } else { 0.0 }
}

pub(super) fn hex(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c.0, c.1, c.2)
}

fn align(a: wordcraft_doc::Align) -> Align {
    match a {
        wordcraft_doc::Align::Left => Align::Left,
        wordcraft_doc::Align::Center => Align::Center,
        wordcraft_doc::Align::Right => Align::Right,
        wordcraft_doc::Align::Justify | wordcraft_doc::Align::Distribute => Align::Justify,
    }
}

/// A list paragraph's numbering.
#[derive(Clone, Copy)]
struct ListRef {
    num: u32,
    level: u8,
    ordered: bool,
}

/// One list paragraph (an item) or a block that continues an item (indented to its text).
enum Entry {
    Item { level: u8, ordered: bool, num: u32, start: u32, checked: Option<bool>, blocks: Vec<Block> },
    Extra { level: u8, blocks: Vec<Block> },
}

struct Reader<'a> {
    doc: &'a Document,
    /// The styles a document built from the flow gets (spacing is written when it differs).
    refs: StyleSheet,
    counters: Counters,
    media: BTreeMap<String, MediaInfo>,
}

impl Reader<'_> {
    fn blocks(&mut self, bl: &Blocks, depth: usize) -> Vec<Block> {
        let mut out = Vec::new();
        let mut i = 0;
        while let Some(b) = bl.get(i) {
            if let wordcraft_doc::Block::Para(p) = &**b
                && self.list_ref(p).is_some()
            {
                let mut entries = Vec::new();
                let mut last: Option<ListRef> = None;
                let mut j = i;
                while let Some(b) = bl.get(j) {
                    match &**b {
                        wordcraft_doc::Block::Para(p) => {
                            if let Some(li) = self.list_ref(p) {
                                entries.push(self.item(p, li));
                                last = Some(li);
                            } else if let Some(li) = last
                                && fm::para_kind(self.doc, p).0 == Kind::Normal
                                && let Some(level) = self.level_at(self.doc.styles.resolve_para(&p.props).indent_left, li)
                            {
                                entries.push(Entry::Extra { level, blocks: self.para_blocks(p, true) });
                            } else {
                                break;
                            }
                        }
                        wordcraft_doc::Block::Table(t) => match last {
                            Some(li) if depth < MAX_DEPTH => match self.level_at(t.props.indent.unwrap_or(0.0), li) {
                                Some(level) => entries.push(Entry::Extra { level, blocks: vec![self.table(t, depth)] }),
                                None => break,
                            },
                            _ => break,
                        },
                    }
                    j += 1;
                }
                out.extend(build_lists(entries));
                i = j.max(i + 1);
                continue;
            }
            match &**b {
                wordcraft_doc::Block::Para(p) => out.extend(self.para_blocks(p, false)),
                wordcraft_doc::Block::Table(t) if depth < MAX_DEPTH => out.push(self.table(t, depth)),
                wordcraft_doc::Block::Table(t) => {
                    // Too deep: the cells' content, in order.
                    for r in &t.rows {
                        for c in &r.cells {
                            out.extend(self.blocks(&c.blocks, depth));
                        }
                    }
                }
            }
            i += 1;
        }
        // The empty paragraph a story or cell needs after a table is not content.
        if let [.., Block::Table(_), Block::Paragraph(p)] = out.as_slice()
            && p.runs.is_empty()
            && p.heading_level.is_none()
            && p.style == ParaStyle::Normal
        {
            out.pop();
        }
        out
    }

    /// Numbering of a list paragraph (headings and titles keep their outline numbers off the list).
    fn list_ref(&self, p: &wordcraft_doc::Paragraph) -> Option<ListRef> {
        let n = self.doc.styles.resolve_para(&p.props).numbering?;
        let lv = self.doc.numbering.level(n.num, n.level)?;
        let (kind, _) = fm::para_kind(self.doc, p);
        if !matches!(kind, Kind::Normal | Kind::Quote | Kind::Code) {
            return None;
        }
        let ordered = !matches!(lv.format, wordcraft_doc::section::NumFormat::Bullet | wordcraft_doc::section::NumFormat::None);
        Some(ListRef { num: n.num, level: n.level.min(MAX_LEVEL), ordered })
    }

    /// The list level (up to the last item's) whose text indent is `indent`.
    fn level_at(&self, indent: f32, li: ListRef) -> Option<u8> {
        let abs = self.doc.numbering.abstract_of(li.num)?;
        (0..=li.level).rev().find(|l| abs.levels.get(*l as usize).is_some_and(|lv| (lv.indent - indent).abs() < 1.0))
    }

    fn item(&mut self, p: &wordcraft_doc::Paragraph, li: ListRef) -> Entry {
        let _ = self.counters.next_label(&self.doc.numbering, li.num, li.level);
        let start = self.counters.value(&self.doc.numbering, li.num, li.level).unwrap_or(1);
        let mut blocks = self.para_blocks(p, true);
        let mut checked = None;
        if let Some(Block::Paragraph(para)) = blocks.iter_mut().find(|b| matches!(b, Block::Paragraph(_))) {
            checked = take_checkbox(para);
        }
        Entry::Item { level: li.level, ordered: li.ordered, num: li.num, start, checked, blocks }
    }

    /// A paragraph as blocks: usually one paragraph; a picture alone is an image block, a rule a
    /// rule, and page breaks become `pageBreak` blocks between the pieces.
    fn para_blocks(&mut self, p: &wordcraft_doc::Paragraph, in_list: bool) -> Vec<Block> {
        let (kind, _) = fm::para_kind(self.doc, p);
        let rp = self.doc.styles.resolve_para(&p.props);
        let mut out = Vec::new();
        if rp.page_break_before {
            out.push(Block::PageBreak);
        }
        if kind == Kind::Rule {
            out.push(Block::Rule);
            return out;
        }
        let segments = self.inlines(p, kind == Kind::Code);
        if let [seg] = segments.as_slice()
            && let Some(img) = lone_image(seg)
        {
            out.push(Block::Image(ImageBlock { media: img.media.clone(), w: img.w, h: img.h, alt: img.alt.clone(), align: align(rp.align) }));
            return out;
        }
        let shell = self.shell(kind, &rp, in_list);
        let n = segments.len();
        for (k, runs) in segments.into_iter().enumerate() {
            if k > 0 {
                out.push(Block::PageBreak);
            }
            if !runs.is_empty() || n == 1 {
                out.push(Block::Paragraph(Paragraph { runs, ..shell.clone() }));
            }
        }
        out
    }

    /// Paragraph-level fields (no runs).
    fn shell(&self, kind: Kind, rp: &ResolvedPara, in_list: bool) -> Paragraph {
        let (style, heading_level) = match kind {
            Kind::Heading(n) => (ParaStyle::Normal, Some(n.clamp(1, 6))),
            Kind::Title => (ParaStyle::Title, None),
            Kind::Quote => (ParaStyle::Quote, None),
            Kind::Code => (ParaStyle::Code, None),
            Kind::Normal | Kind::Rule => (ParaStyle::Normal, None),
        };
        let rr = self.refs.resolve_para(&ParaProps { style: super::style_id(style, heading_level, in_list), ..Default::default() });
        let differs = |a: f32, b: f32| if (a - b).abs() > 0.05 { Some(pt(a)) } else { None };
        let indent = if in_list { 0 } else { ((rp.indent_left - rr.indent_left) / INDENT_STEP).round().clamp(0.0, f32::from(MAX_LEVEL)) as u8 };
        let line_height = match (rp.line_spacing, rr.line_spacing) {
            (LineSpacing::Multiple(a), LineSpacing::Multiple(b)) if (a - b).abs() < 0.005 => None,
            (LineSpacing::Multiple(a), _) if a.is_finite() => Some((f64::from(a) * 1000.0).round() / 1000.0),
            _ => None,
        };
        Paragraph {
            style,
            heading_level,
            align: align(rp.align),
            indent,
            line_height,
            space_before: differs(rp.space_before, rr.space_before),
            space_after: differs(rp.space_after, rr.space_after),
            runs: Vec::new(),
        }
    }

    /// The paragraph's inlines, split at page breaks.
    fn inlines(&mut self, p: &wordcraft_doc::Paragraph, in_code: bool) -> Vec<Vec<Inline>> {
        let mut segs: Vec<Vec<Inline>> = vec![Vec::new()];
        let mut k = 0usize;
        for (range, props) in p.run_ranges() {
            let Some(text) = p.text.get(range) else { continue };
            let hidden = props.hidden.unwrap_or(false) || props.del.is_some();
            let t = self.text_fmt(props, in_code);
            for c in text.chars() {
                let obj = if c == OBJ {
                    k += 1;
                    p.objects.get(k - 1)
                } else {
                    None
                };
                if hidden {
                    continue;
                }
                let Some(seg) = segs.last_mut() else { continue };
                match c {
                    OBJ => match obj {
                        Some(InlineObject::Image { media, w, h, alt, .. }) => {
                            if let Some(id) = self.register(media) {
                                seg.push(Inline::Image(InlineImage { media: id, w: pt(*w), h: pt(*h), alt: alt.clone() }));
                            } else if !alt.is_empty() {
                                push_text(seg, alt, &t);
                            }
                        }
                        Some(o) => push_text(seg, o.plain_text(), &t),
                        None => {}
                    },
                    LINE_BREAK | COLUMN_BREAK => seg.push(Inline::Break),
                    PAGE_BREAK => segs.push(Vec::new()),
                    c => {
                        let mut b = [0u8; 4];
                        push_text(seg, c.encode_utf8(&mut b), &t);
                    }
                }
            }
        }
        segs
    }

    fn text_fmt(&self, props: &wordcraft_doc::CharProps, in_code: bool) -> Text {
        let f = fm::fmt_of(self.doc, props, in_code);
        Text {
            text: String::new(),
            bold: f.bold,
            italic: f.italic,
            underline: f.underline,
            strike: f.strike,
            code: f.code,
            sup: f.sup,
            sub: f.sub,
            color: f.color.map(hex),
            highlight: f.background.map(hex),
            font: f.font,
            size: f.size.map(pt),
            link: f.link,
        }
    }

    /// Note a media key the flow references; `None` if the document lacks its bytes.
    fn register(&mut self, key: &str) -> Option<String> {
        if self.media.contains_key(key) {
            return Some(key.to_string());
        }
        let data = self.doc.media.get(key)?;
        let (w, h) = fm::image_px(data).unwrap_or((0, 0));
        let content_type = super::media_content_type(key, data);
        self.media.insert(key.to_string(), MediaInfo { id: key.to_string(), content_type: content_type.into(), name: key.to_string(), w, h });
        Some(key.to_string())
    }

    fn table(&mut self, t: &wordcraft_doc::Table, depth: usize) -> Block {
        let mut rows = Vec::new();
        for (ri, r) in t.rows.iter().enumerate() {
            let mut cells = Vec::new();
            for (ci, c) in r.cells.iter().enumerate() {
                if c.props.vmerge == VMerge::Continue {
                    continue;
                }
                let mut rowspan = 1u32;
                if c.props.vmerge == VMerge::Restart {
                    let g = t.grid_col(ri, ci);
                    for (k, below) in t.rows.iter().enumerate().skip(ri + 1) {
                        let cont = t.cell_at_grid(k, g).and_then(|i| below.cells.get(i)).is_some_and(|bc| bc.props.vmerge == VMerge::Continue);
                        if !cont {
                            break;
                        }
                        rowspan = rowspan.saturating_add(1);
                    }
                }
                cells.push(Cell {
                    colspan: c.span() as u32,
                    rowspan,
                    background: c.props.shading.map(hex),
                    blocks: self.blocks(&c.blocks, depth + 1),
                });
            }
            rows.push(Row { header: r.props.header, cells });
        }
        Block::Table(Table { col_widths: t.grid.iter().map(|w| pt(*w)).collect(), rows })
    }
}

fn push_text(seg: &mut Vec<Inline>, s: &str, f: &Text) {
    if s.is_empty() {
        return;
    }
    if let Some(Inline::Text(last)) = seg.last_mut()
        && last.same_format(f)
    {
        last.text.push_str(s);
        return;
    }
    seg.push(Inline::Text(Text { text: s.to_string(), ..f.clone() }));
}

/// The picture of a paragraph that holds nothing else (whitespace aside).
fn lone_image(seg: &[Inline]) -> Option<&InlineImage> {
    let mut img = None;
    for i in seg {
        match i {
            Inline::Image(m) if img.is_none() => img = Some(m),
            Inline::Text(t) if t.text.trim().is_empty() => {}
            _ => return None,
        }
    }
    img
}

/// A leading ☐ / ☒ (task-list checkbox) taken off the paragraph: its state.
fn take_checkbox(p: &mut Paragraph) -> Option<bool> {
    let Some(Inline::Text(t)) = p.runs.first_mut() else { return None };
    let c = t.text.chars().next()?;
    let checked = match c {
        TASK_OPEN => false,
        TASK_DONE | '☑' => true,
        _ => return None,
    };
    let rest = t.text.get(c.len_utf8()..).unwrap_or("");
    let rest = rest.strip_prefix([' ', '\u{00A0}', '\t']).unwrap_or(rest).to_string();
    t.text = rest;
    if t.text.is_empty() {
        p.runs.remove(0);
    }
    Some(checked)
}

/// Nest a run of list entries into list blocks.
fn build_lists(entries: Vec<Entry>) -> Vec<Block> {
    let mut out = Vec::new();
    let mut it = entries.into_iter().peekable();
    while let Some(e) = it.peek() {
        match e {
            Entry::Extra { .. } => {
                if let Some(Entry::Extra { blocks, .. }) = it.next() {
                    out.extend(blocks);
                }
            }
            Entry::Item { .. } => out.push(Block::List(build_list(&mut it, 0))),
        }
    }
    out
}

fn build_list(it: &mut Peekable<IntoIter<Entry>>, depth: usize) -> List {
    let Some(Entry::Item { level: base, ordered, num, start, checked, blocks }) = it.next() else { return List::default() };
    let mut list = List { ordered, start: (ordered && start != 1).then_some(start), items: vec![ListItem { blocks, checked }] };
    while let Some(e) = it.peek() {
        match e {
            Entry::Item { level, ordered: o, num: n, .. } => {
                if *level < base {
                    break;
                }
                if *level > base && depth < usize::from(MAX_LEVEL) {
                    let nested = build_list(it, depth + 1);
                    if let Some(last) = list.items.last_mut() {
                        last.blocks.push(Block::List(nested));
                    }
                    continue;
                }
                if *o != ordered || *n != num {
                    break;
                }
                if let Some(Entry::Item { blocks, checked, .. }) = it.next() {
                    list.items.push(ListItem { blocks, checked });
                }
            }
            Entry::Extra { level, .. } => {
                if *level < base {
                    break;
                }
                if let Some(Entry::Extra { blocks, .. }) = it.next()
                    && let Some(last) = list.items.last_mut()
                {
                    last.blocks.extend(blocks);
                }
            }
        }
    }
    list
}
