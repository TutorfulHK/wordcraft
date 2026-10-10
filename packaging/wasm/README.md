# @tutorfulhk/wordcraft-wasm

WordCraft as a headless WebAssembly library, for web apps that keep their own rich-text model
(a ProseMirror / TipTap editor, say): open and save `.docx`, convert to and from **Flow JSON** (a
block tree of paragraphs, lists, tables and pictures, see
[`docs/flow-json.md`](../../docs/flow-json.md)), export PDF with the fonts embedded, and render
pages to PNG. It is the WordCraft engine (`crates/engine`) behind a thin wasm-bindgen binding
(`crates/wasm`); it is not the egui app.

Run it in a **Web Worker**: calls are synchronous and a long document takes a while. The module
never panics on purpose, but a WebAssembly trap can't be caught inside the module: if a call throws
a `WebAssembly.RuntimeError`, restart the worker.

## API

```ts
import init, { Engine, addFont, version } from "@tutorfulhk/wordcraft-wasm";

await init();                                   // fetches wordcraft_wasm_bg.wasm next to the module
addFont(new Uint8Array(await (await fetch("/fonts/NotoSansTC-Regular.otf")).arrayBuffer())); // see Fonts

const engine = new Engine();                    // starts with a blank document
engine.openBytes("notes.docx", docxBytes);      // .docx (or .odt, .rtf, .html, .md, .txt); replaces the document
const flow = JSON.parse(engine.flow());         // Flow JSON
for (const m of flow.media) {
  const bytes = engine.mediaBytes(m.id);        // Uint8Array of each picture
}

// Back again: stage new pictures (raw bytes, no base64), then build from the flow.
engine.putMedia("my-image-1", imageBytes);
engine.fromFlow(JSON.stringify(flow));          // media ids from the last flow() need no re-upload
const docx = engine.saveBytes("docx");          // Uint8Array; also "pdf", "html", "md", "txt", "odt", "rtf"
const pdf = engine.saveBytes("pdf");            // fonts embedded (subset)

const pages = engine.pageCount();
const thumb = engine.renderPagePng(0, 0.25);    // page 0 as PNG at 0.25 px per point

engine.execute("file.info", "{}");              // any engine command (JSON in/out)
version();                                      // "0.4.0"
```

| Call | Does |
|---|---|
| `new Engine()` | an engine with a blank document open |
| `execute(id, paramsJson) → string` | runs any engine command (`wordcraft-cli commands` lists them) |
| `openBytes(name, bytes)` | opens document bytes, replacing the document; the extension of `name` picks the format |
| `saveBytes(format) → Uint8Array` | `"docx"`, `"pdf"`, `"html"`, `"md"`, `"txt"`, `"odt"` or `"rtf"` |
| `flow() → string` | the document as Flow JSON |
| `putMedia(id, bytes)` | stages a picture for the next `fromFlow` |
| `fromFlow(flowJson)` | replaces the document with one built from Flow JSON |
| `mediaBytes(id) → Uint8Array` | bytes of a media id the flow references |
| `pageCount() → number` | pages in the print layout |
| `renderPagePng(page, scale) → Uint8Array` | page (0-based) as PNG; `scale` is pixels per point, capped at 4 |
| `addFont(bytes) → number` | adds a TTF/OTF/TTC font for layout, rendering and PDF; returns the faces added |
| `version() → string` | library version (also `Engine.version()`) |

Errors are thrown as `Error` with the engine's message (unknown command, bad parameters, a file
that isn't a document, a flow that doesn't parse, missing media…).

`fromFlow` finds a media id in this order: bytes staged with `putMedia`, then the pictures of the
document open at the time (by the ids `flow()` gave them). A missing id is an error naming it.
`openBytes` clears staged media.

The same operations are engine commands (`file.openBytes`, `file.saveBytes`, `file.flow`,
`file.fromFlow`, `media.bytes`) that take and return base64, for `execute` or the CLI / MCP.

## Fonts

The library carries WordCraft's bundled open fonts (Source Sans 3, Source Serif 4, Inter,
JetBrains Mono; SIL OFL), and a document font that isn't loaded is replaced by one of the same kind
(sans, serif, mono), so Latin text lays out and exports without help. For line breaks closer to
Word's, add the metric-compatible open fonts with `addFont` (Carlito for Calibri, Caladea for
Cambria, Liberation Sans / Serif / Mono for Arial / Times New Roman / Courier New): they are tried
first. The library has no CJK font: for Chinese, Japanese or Korean text,
fetch an OTF/TTF/TTC font (for example Noto Sans TC / SC; WOFF2 is not read) and pass it to
`addFont` before rendering or saving PDF. Characters a document's font lacks fall back to any added
font that has them, and the PDF embeds the subset used. Task-list checkboxes are the characters
☐ / ☒ (U+2610 / U+2612): to draw them in PDF and PNG output, add a font that has them (DejaVu Sans,
Noto Sans Symbols 2).

## Building

```sh
cargo install wasm-bindgen-cli --locked --version 0.2.129   # the version in Cargo.lock
npm install -g binaryen                                     # wasm-opt (optional, shrinks the module)
packaging/wasm/build.sh target/wasm-pkg
```

`build.sh` builds with the `wasm-release` profile (`opt-level = "z"`, fat LTO, `panic = "abort"`),
runs `wasm-bindgen --target web` and `wasm-opt -Oz`, writes `package.json` from the template here,
and prints the module size.

The GitHub workflow `.github/workflows/wasm-package.yml` does the same on a tag `wasm-v<version>`
and publishes the package to GitHub Packages (`https://npm.pkg.github.com`). To install it, point
the scope at that registry in `.npmrc`:

```
@tutorfulhk:registry=https://npm.pkg.github.com
```

## Size

`wordcraft_wasm_bg.wasm` from `build.sh`, measured 2026-10-10 with wasm-bindgen 0.2.129 and
wasm-opt (binaryen) 132:

| Stage | Raw | gzip -9 | brotli -q 11 |
|---|---|---|---|
| after wasm-bindgen | 19.97 MB | 5.83 MB | 3.74 MB |
| + `wasm-opt -Oz` (published) | 12.84 MB | 5.29 MB | 3.57 MB |

About 4.2 MB of the raw module is the bundled fonts, which barely compress. The JS glue is about
14 KB (3 KB gzipped).

## Licence

MIT OR Apache-2.0, as WordCraft. The bundled fonts are under the SIL Open Font License; third-party
notices: `NOTICE`.
