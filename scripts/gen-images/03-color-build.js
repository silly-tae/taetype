// Phase 3 (representation-todo.md): bitmap color-font path. get_glyph_bitmap()
// already returns a complete, real PNG (src/font/color.rs — sbix_bitmap/
// cbdt_bitmap) — no compositing, no @font-face, no sfnt patching needed here,
// unlike phase 2's instancing grid. Picks the nearest-to-target CBDT strike;
// probed several codepoints first and confirmed this font's largest real strike
// is 109ppem before requesting it.

const fs = require('fs');
const path = require('path');

const repoRoot = path.join(__dirname, '..', '..');
const taetype = require(path.join(repoRoot, 'pkg', 'node', 'taetype.js'));

const fontPath = path.join(repoRoot, 'assets', 'fonts', 'noto-color-emoji', 'NotoColorEmoji.ttf');
const fontBytes = fs.readFileSync(fontPath);

const FONT_NAME = 'Noto';
taetype.register_font_raw(FONT_NAME, fontBytes);

// Single-codepoint emoji only (no ZWJ sequences) — get_glyph_ids is a plain
// cmap lookup, not GSUB-aware, so a multi-codepoint sequence would need
// shape_text instead. Verified all of these resolve to a real bitmap first.
const EMOJI = [
  { name: 'grinning face', codepoint: 0x1f600 },
  { name: 'fire', codepoint: 0x1f525 },
  { name: 'rainbow', codepoint: 0x1f308 },
  { name: 'party popper', codepoint: 0x1f389 },
  { name: 'pizza', codepoint: 0x1f355 },
  { name: 'artist palette', codepoint: 0x1f3a8 },
];

const glyphs = EMOJI.map(({ name, codepoint }) => {
  const ch = String.fromCodePoint(codepoint);
  const gids = taetype.get_glyph_ids(ch, FONT_NAME, 'normal', 400);
  const gid = gids[0];
  const bitmap = taetype.get_glyph_bitmap(FONT_NAME, 'normal', gid, 160);
  if (!bitmap) throw new Error(`get_glyph_bitmap returned null for ${name} (U+${codepoint.toString(16)})`);
  return {
    name,
    codepoint: codepoint.toString(16),
    gid,
    ppem: bitmap.ppem,
    pngBase64: Buffer.from(bitmap.png).toString('base64'),
  };
});

const result = {
  font: 'Noto Color Emoji (NotoColorEmoji.ttf)',
  license: 'SIL OFL 1.1',
  tableFormat: 'CBDT/CBLC',
  glyphs,
};

console.log(
  result.glyphs.map(({ name, codepoint, gid, ppem, pngBase64 }) => ({
    name,
    codepoint,
    gid,
    ppem,
    pngBytes: Buffer.from(pngBase64, 'base64').length,
  })),
);

fs.writeFileSync(path.join(__dirname, '03-color-result.json'), JSON.stringify(result, null, 2));
