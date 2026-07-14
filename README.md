# taetype

A font engine in Rust and WebAssembly: decode, instance, subset, and shape TTF, OTF, TTC, and WOFF2 fonts.

Extracted from taepdf's internal font engine. Pure Rust at its core – no browser API required, no server, no external font tooling. Runs as WASM in the browser, as a native library in a backend or desktop app, or as a plain Rust dependency.

```bash
npm i taetype
```

```bash
cargo add taetype
```

---

## Examples

Real output from taetype's own APIs, not mockups – see `scripts/gen-images/` to
reproduce any of these numbers yourself.

### Subsetting

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/silly-tae/taetype/v0.1.3/assets/images/01-subsetting-dark.png">
  <img src="https://raw.githubusercontent.com/silly-tae/taetype/v0.1.3/assets/images/01-subsetting-light.png" alt="Inter variable font subset to one paragraph's glyphs: 859 KB to 3 KB, a 99.6% reduction, using 27 of 2,937 glyphs.">
</picture>

`register_font_raw` → `shape_text` → `subset_font_full` on [Inter](https://github.com/rsms/inter)
(SIL OFL 1.1) against one real body paragraph. Numbers come straight out of
`subset_font_full`'s returned `fontBytes.length` – reproduce with
`node scripts/gen-images/01-subsetting-measure.js && node scripts/gen-images/01-subsetting-render.js`.

### Variable font instancing

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/silly-tae/taetype/v0.1.3/assets/images/02-instancing-dark.png">
  <img src="https://raw.githubusercontent.com/silly-tae/taetype/v0.1.3/assets/images/02-instancing-light.png" alt="Inter variable font instanced at five weights (100, 300, 400, 700, 900) at optical size 32, from hairline to black.">
</picture>

Five separate `subset_font_full` calls against the same registered variable font,
one per weight – each column is a real static instance, not a CSS `font-weight`
approximation of a single file. Reproduce with
`node scripts/gen-images/02-instancing-build.js && node scripts/gen-images/02-instancing-render.js`.

### Color glyphs

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/silly-tae/taetype/v0.1.3/assets/images/03-color-dark.png">
  <img src="https://raw.githubusercontent.com/silly-tae/taetype/v0.1.3/assets/images/03-color-light.png" alt="Six colorful emoji glyphs (grinning face, fire, rainbow, party popper, pizza, artist palette) rendered from Noto Color Emoji's real CBDT bitmap strikes.">
</picture>

Each image is `get_glyph_bitmap`'s actual return value on [Noto Color
Emoji](https://github.com/googlefonts/noto-emoji) (SIL OFL 1.1, CBDT/CBLC format) –
a real PNG strike pulled out of the font, drawn with zero compositing. Reproduce with
`node scripts/gen-images/03-color-build.js && node scripts/gen-images/03-color-render.js`.

### Shaping

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="https://raw.githubusercontent.com/silly-tae/taetype/v0.1.3/assets/images/04-shaping-dark.png">
  <img src="https://raw.githubusercontent.com/silly-tae/taetype/v0.1.3/assets/images/04-shaping-light.png" alt="The word office in EB Garamond, unshaped (six separate letters) versus shape_text() output (four glyphs, with f-f-i fused into one real ffi ligature).">
</picture>

Every glyph above is drawn individually at `shape_text`'s own real advances – not
the browser reshaping a real font on its own, which would only prove the *font*
has ligatures, not that taetype computed them. Reproduce with
`node scripts/gen-images/04-shaping-build.js && node scripts/gen-images/04-shaping-render.js`.

---

## What it does

- **Decode** – TTF, OTF, and TTC (font collections). Reads the full table directory, not a fixed allowlist, so nothing in the source font gets dropped.
- **WOFF2** – its own Brotli decompression, written in Rust. Not the browser's `DecompressionStream('brotli')` API, which isn't available everywhere (a real-world gap on Brave/Windows is what prompted this).
- **Instance** – the full OpenType Font Variations model (`fvar`, `avar`, `gvar`, `hvar`, `vvar`, `mvar`, `cvar`). Turn a variable font into a static instance at any weight or optical size.
- **Subset** – TrueType (`glyf`) and CFF, including CID-keyed CFF (FDArray/FDSelect) and `seac` composite accented glyphs. Keep only the glyphs a document actually uses.
- **Shape** – GSUB ligatures, GPOS kerning, complex scripts, vertical writing, via `rustybuzz` (a Rust port of HarfBuzz).
- **Color fonts** – COLR/CPAL layered glyphs, CBDT/CBLC and sbix bitmap strikes.

## What it doesn't do

Rasterization. taetype hands back glyph IDs, outlines, and metrics – not pixels. Pair it with whatever renderer fits your target:

- **Browser** – canvas/DOM already rasterizes, no extra dependency needed
- **Native Rust (CPU)** – `ab_glyph` or `fontdue`
- **Native Rust (mature, C-backed)** – `freetype-rs`, if a C dependency is acceptable
- **GPU / game engines** – rasterize each glyph once into a texture atlas with any of the above, then render textured quads

CFF2 (variable CFF) fonts. Subsetting/embedding a CFF2 font returns an error; instancing silently returns the font unmodified instead of applying the requested weight/optical size. Register these as static, already-instanced files instead – most released variable fonts use the TrueType `glyf`/`gvar` model, which taetype fully supports.

## Use cases

- **Client-side document generation** – its origin: taepdf uses it to shape text and embed only the glyphs a PDF actually needs, entirely in the browser, no server round-trip.
- **Web font optimization** – subset a font down to the glyphs a page uses as a build step, without shelling out to Python/fonttools.
- **Variable-to-static conversion** – serve a single variable font file but hand out fixed-weight instances to targets that don't understand variable fonts (older PDF viewers, some embedding pipelines).
- **In-browser typesetting** – real GSUB/GPOS shaping (ligatures, kerning, complex scripts, vertical writing) for canvas or custom text layout, without a full browser text stack.
- **Server-side image generation** – shape text server-side and rasterize straight to a PNG/JPEG: certificates, share cards, dynamic thumbnails, no browser or PDF viewer needed.
- **E-book / print tooling** – EPUB or other document generators that need subsetted, embedded fonts and don't want a browser in the loop.
- **Font inspection in CI** – check TTC member count, detect variable axes, read family/weight/style metadata, validate WOFF2 files, as a native Rust build step.

## Building

WASM, for the browser and for Node.js (requires [`wasm-pack`](https://rustwasm.github.io/wasm-pack/)):

```bash
cargo install wasm-pack
wasm-pack build --target web --out-dir pkg/web
wasm-pack build --target nodejs --out-dir pkg/node
```

Both share the same compiled `.wasm` binary – only the surrounding JS glue differs. `package.json`'s `exports` field routes browsers/bundlers to `pkg/web` and Node.js to `pkg/node` automatically once published; see the end-to-end examples below for using either directly from source.

Native Rust library:

```bash
cargo build --release
```

## API reference

Every function below is exported from both WASM builds (`pkg/web` for browsers, `pkg/node` for Node.js) and callable directly if you use taetype as a native Rust crate. Fonts are registered under a `(name, style)` key and referenced by that pair afterward.

The browser build needs an explicit `await init()` before first use. The Node build doesn't – `require()`/`import` loads the WASM synchronously, so every function is ready immediately. See the end-to-end examples below for both.

### Registering fonts

```js
register_font_raw(name: string, raw_bytes: Uint8Array): void
```
Register a plain TTF/OTF file. The simplest entry point – in the browser, call `init()` first; in Node, just call this directly.

```js
register_font_ttc(name: string, ttc_bytes: Uint8Array, index: number): void
ttc_font_count(bytes: Uint8Array): number
```
Register one member font of a TrueType Collection (`.ttc`). `ttc_font_count` tells you how many fonts a collection holds before you pick an index.

```js
register_font(name, style, weight, opsz, woff2_bytes, decompressed, index): void
```
Register a WOFF2 font. Unlike the two above, this expects the Brotli stream already decompressed – see the WOFF2 section below for the full pipeline. `weight`/`opsz` let you register multiple static weights (e.g. Regular + Bold) under the same name; taetype auto-detects true variable fonts and serves every weight from one registration regardless of what you pass here.

```js
list_registered_fonts(): string[]
```
Every registered `"name:style"` pair, for debugging or building a font picker.

### WOFF2 pipeline

```js
get_compressed_range(woff2_bytes: Uint8Array): { start: number, length: number }
decompress_brotli(compressed: Uint8Array): Uint8Array
read_font_meta(woff2_bytes, decompressed, index): { style, weight, isVariable }
```
`get_compressed_range` tells you which byte range of the raw WOFF2 file is the actual Brotli-compressed table data (skipping the header). Slice that range out, run it through `decompress_brotli`, then either inspect it with `read_font_meta` or hand both buffers to `register_font`. All three run in Rust – no browser compression API involved.

### Glyphs and metrics

```js
get_glyph_ids(text: string, font_name: string, style: string, weight: number): Uint16Array
font_has_glyph(font_name: string, style: string, codepoint: number): boolean
get_advance_widths(font_name, style, weight, opsz, glyph_ids: Uint16Array): Float64Array
get_vertical_advance(font_name, style, weight, opsz, gid: number): number
measure_string_width(text, font_name, style, weight, opsz, font_size: number): number
```
Per-character glyph lookup, coverage checks, and advance widths (1000 units/em). `measure_string_width` uses real shaping internally when available, so ligatures and kerning are reflected in the measurement, not just summed per-character widths.

### Shaping

```js
shape_text(text, font_name, style, weight, opsz, vertical: boolean):
  { glyphs: Uint16Array, advances: Float64Array, clusters: Uint32Array } | null
```
Full GSUB/GPOS shaping via `rustybuzz`. `clusters` maps each output glyph back to the character index it came from, for cursor placement or highlighting. Returns `null` for fonts `rustybuzz` can't open – fall back to `get_glyph_ids` + `get_advance_widths` in that case.

### Subsetting

```js
subset_font_full(font_name, style, weight, opsz, glyph_ids: Uint16Array):
  { fontBytes, glyphMap, isCff, ascender, descender, capHeight, bbox, flags, italicAngle, fontName }
```
Give it the glyph IDs a document actually uses (typically `shape_text(...).glyphs`), get back a font file containing only those glyphs plus the metrics needed to embed it. `glyphMap` is the old→new glyph ID remap for TrueType subsets (`Uint16Array`); it's `null` for CFF subsets, which blank out unused glyphs in place instead of renumbering.

### Color fonts

```js
get_glyph_bitmap(font_name, style, gid, target_ppem): { png, ppem, originX, originY } | null
get_colr_layers(font_name, style, gid): Uint32Array
```
`get_glyph_bitmap` pulls the nearest-size PNG strike from `sbix` or `CBDT`/`CBLC`. `get_colr_layers` returns a flattened `[gid, r, g, b, a, isForeground, ...]` array (6 values per layer) for COLR v0 glyphs; empty when the glyph has no color layers.

## End-to-end example

Examples below import the bare specifier `taetype` (`npm i taetype`) – `exports` in `package.json` routes browsers/bundlers to the `web` build and Node.js to the `node` build automatically, same import either way. Building from source instead? Swap `taetype` for `./pkg/web/taetype.js` or `./pkg/node/taetype.js` (see [Building](#building)).

### Browser

TTF/OTF, the simplest path:

```js
import init, { register_font_raw, shape_text, subset_font_full } from 'taetype';

await init();

register_font_raw('MyFont', fontBytes); // Uint8Array of a .ttf/.otf file

const shaped = shape_text('Hello, world', 'MyFont', 'normal', 400, 0, false);
const subset = subset_font_full('MyFont', 'normal', 400, 0, shaped.glyphs);
// subset.fontBytes is a font file containing only the glyphs used above
```

WOFF2, decompressing the Brotli stream yourself first:

```js
import init, { get_compressed_range, decompress_brotli, register_font, shape_text } from 'taetype';

await init();

const { start, length } = get_compressed_range(woff2Bytes); // Uint8Array of a .woff2 file
const decompressed = decompress_brotli(woff2Bytes.slice(start, start + length));

register_font('MyFont', 'normal', 400, 0, woff2Bytes, decompressed, 0);

const shaped = shape_text('Hello, world', 'MyFont', 'normal', 400, 0, false);
```

### Node.js

No `init()` step – requiring the module loads the WASM synchronously:

```js
const { register_font_raw, shape_text, subset_font_full } = require('taetype');
const fs = require('fs');

const fontBytes = fs.readFileSync('MyFont.ttf');
register_font_raw('MyFont', fontBytes);

const shaped = shape_text('Hello, world', 'MyFont', 'normal', 400, 0, false);
const subset = subset_font_full('MyFont', 'normal', 400, 0, shaped.glyphs);
// subset.fontBytes is a font file containing only the glyphs used above
```

## License

MIT – see [LICENSE](LICENSE).
