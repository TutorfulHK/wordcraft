# Flow JSON (version 1)

Flow JSON is a WordCraft document as a block tree, for hosts that keep their own rich-text model,
such as a web app with a ProseMirror / TipTap editor driving the
[`wordcraft-wasm`](../packaging/wasm/README.md) library from a Web Worker. It covers paragraphs
and headings, inline formatting, nested and task lists, tables with merges and shading, pictures,
rules and page breaks.

| Command | Does |
|---|---|
| `file.openBytes {name, data: base64}` | opens document bytes (the extension of `name` picks the format: `docx`, `odt`, `rtf`, `html`, `md`, `txt`…), replacing the document |
| `file.saveBytes {format}` | → `{data: base64, contentType, bytes}`; `format` is `docx`, `pdf`, `html`, `md`, `txt`, `odt` or `rtf` |
| `file.flow {}` | → the document as Flow JSON |
| `file.fromFlow {flow, media?: {id: base64}}` | replaces the document with one built from a flow (`flow` may be an object or JSON text) |
| `media.bytes {id}` | → `{data: base64, contentType, bytes}` of a media id the flow references |

None of them touch the file system or open a dialog. The Rust types are in
`crates/engine/src/flow/schema.rs`; the mapping is `flow/read.rs` (document → flow) and
`flow/write.rs` (flow → document).

## Conventions

- Units are **points** (1/72 in). Colours are `"#rrggbb"`.
- On input every field except `version` is optional and defaults as listed; unknown fields are
  ignored, an unknown `type` is an error. Out-of-range numbers are clamped (page sides 72–1584 pt,
  pictures up to 1584 pt, spans up to 63 columns / 1000 rows, nine list levels).
- On output, `null` (or an absent field) means "what the style gives". Spacing and line height are
  written only where they differ from the built-in style the paragraph maps to, so a document that
  came from a flow gives the same flow back.

## Document

```
Flow      { version: 1, page: Page, blocks: [Block], media: [Media] }
Page      { w, h, margins: { t, r, b, l } }          // default: US Letter, 72 pt margins
Media     { id, contentType, name, w, h }            // w, h: pixel size of the image (0 = unknown)
```

`page` is the document's last section. `media` lists every picture the blocks reference; fetch the
bytes with `media.bytes` (or `mediaBytes` in the wasm library). On input `media` is ignored: ids
are resolved when the document is built (see [Media](#media)).

## Blocks

```
Block =
  { type: "paragraph", style, headingLevel?, align, indent, lineHeight, spaceBefore, spaceAfter, runs: [Inline] }
| { type: "list", ordered, start?, items: [{ blocks: [Block], checked: bool | null }] }
| { type: "table", colWidths: [pt], rows: [{ header, cells: [{ colspan, rowspan, background?, blocks: [Block] }] }] }
| { type: "image", media, w, h, alt, align }
| { type: "rule" }
| { type: "pageBreak" }
```

### paragraph

| Field | Type | Default | Meaning |
|---|---|---|---|
| `style` | `"normal"` \| `"title"` \| `"quote"` \| `"code"` | `"normal"` | the paragraph style (Normal, Title, Quote, Code) |
| `headingLevel` | 1–6 | absent | a heading (`Heading1`…`Heading6`); `style` is then `"normal"` |
| `align` | `"left"` \| `"center"` \| `"right"` \| `"justify"` | `"left"` | |
| `indent` | 0–8 | 0 | left indent in levels of 36 pt, on top of the style's |
| `lineHeight` | number \| null | null | line spacing as a multiple of single (1.15, 1.5, 2…) |
| `spaceBefore`, `spaceAfter` | pt \| null | null | space above / below |
| `runs` | [Inline] | [] | the content |

A code paragraph is one line of a code block; its text is monospace through the style, so its runs
don't also carry `code`.

### list

`ordered` picks numbers or bullets; `start` is the first number of an ordered list (absent when 1).
Each item holds blocks: usually a paragraph, then more paragraphs, pictures, tables or a nested
`list` (which is one level deeper). Lists nest up to nine levels.

An item whose `checked` is `true` or `false` is a **task item**. A list whose items carry `checked`
is a task list (TipTap `taskList` / `taskItem`).

### table

`colWidths` are the grid columns. Each row lists its cells left to right; a cell spanning rows
(`rowspan` > 1) covers the cells below it, which are **left out** of the rows below, as in HTML and
ProseMirror tables. `header` marks a header row (repeated at the top of each page in Word, `th`
cells in TipTap). `background` is the cell shading. Cells hold blocks, including nested tables
(up to eight deep; deeper tables are flattened into their cells' blocks).

### image

A picture on its own line: `media` id, display size `w` × `h` in points (0 or absent = the
picture's natural size at 96 dpi), `alt` text and paragraph alignment.

### rule, pageBreak

A horizontal line (an empty paragraph with a bottom border) and a page break.

## Inlines

```
Inline =
  { type: "text", text, bold, italic, underline, strike, code, sup, sub,
    color?, highlight?, font?, size?, link? }
| { type: "break" }                       // line break within the paragraph
| { type: "image", media, w, h, alt }     // a picture in the line of text
```

- `code` is inline code (a monospace font). `sup` wins over `sub` when both are set.
- `color` is the text colour; absent = automatic.
- `highlight` is the background colour. Word's 16 highlighter colours map to Word highlights;
  any other colour becomes character shading.
- `font` is a font family name and `size` is in points; both are absent when the paragraph style
  decides them.
- `link` is a URL, or `#name` for a bookmark in the document. Linked text isn't also flagged
  `underline` (the link style underlines it).
- Tabs stay in `text` as `\t`.

## Media

`file.flow` gives each picture the id it has in the document (`image1.png`…). `file.fromFlow`
resolves an id from, in order:

1. the command's `media` param: `{ id: base64 }` (or `{ id: { data: base64 } }`);
2. bytes staged under that id (`putMedia` in the wasm library);
3. the media of the document open when `file.fromFlow` runs, so a flow from `file.flow` can be
   built back without sending pictures again.

An id none of these knows is an error that names it.

## Fidelity

Reading a document keeps everything above. It leaves out what the schema has no place for:

- **Sections and page furniture**: headers, footers, footnotes and endnotes (note references are
  dropped), comments, columns, page borders and every section but the last (its page size and
  margins are `page`).
- **Styles beyond the six kinds**: a custom paragraph style maps to `normal` (or to a heading when
  it has an outline level); style-defined fonts, sizes and colours are not repeated on runs. A
  heading deeper than 6 becomes `headingLevel: 6`. Outline numbering on headings is dropped.
- **Paragraph details**: right and first-line / hanging indents, tab stops, borders other than a
  rule, paragraph shading, keep-with-next and widow control. Indents round to 36 pt levels. Exact
  and at-least line spacing read as `lineHeight: null`.
- **Character details**: double strike reads as `strike`; underline styles, caps, small caps,
  character spacing, scale and position, outline / shadow / emboss / engrave and language.
  Hidden text and tracked deletions are left out; tracked insertions read as plain text.
- **Lists**: number formats (decimal vs letters vs roman), custom bullets and label text; only
  ordered / bullet / task and the start number survive. Levels are normalised so nesting goes one
  level at a time. A paragraph or table between items stays in the item when it is indented to
  that item's text, otherwise it ends the list.
- **Tables**: table style, borders, alignment, cell margins, vertical alignment, row heights and
  text direction. A header row is a row marked as a repeating header.
- **Objects**: shapes and text boxes are dropped; floating pictures read as inline pictures (their
  position is lost); picture cropping is dropped; fields read as their current result text;
  equations as their linear text; bookmarks are dropped (links to them are kept).

Building a document from a flow uses the built-in styles and Word's default list levels, so the
first Flow → DOCX → Flow round trip is stable: the tests check that `docx → flow → docx → flow`
gives the same flow twice.

Task-list items are written to DOCX as a list level without a number or bullet, and the item text
starts with ☐ (open) or ☒ (done); reading treats any list item that starts with ☐, ☑ or ☒ as a
task item. (PDF and PNG output draw ☐ / ☒ with whatever loaded font has them.)
