// Phase 2 (representation-todo.md): for each target weight, calls
// subset_font_full() — confirmed in font_cache.rs:150-161 to run get_or_instance()
// internally, so this IS real instancing, not a separate API — and captures the
// resulting real static font file as base64 for the render step to embed as
// distinct @font-face sources. Fixed opsz=32 (display-optimized): InterVariable.ttf's
// actual fvar axes were parsed directly and confirmed to be wght[100,900] and
// opsz[14,32] before this was written, not assumed.

const fs = require('fs');
const path = require('path');
const { addCmapAndName } = require('./lib/sfnt-tools');

const repoRoot = path.join(__dirname, '..', '..');
const taetype = require(path.join(repoRoot, 'pkg', 'node', 'taetype.js'));

const fontPath = path.join(repoRoot, 'assets', 'fonts', 'inter', 'InterVariable.ttf');
const fontBytes = fs.readFileSync(fontPath);

const FONT_NAME = 'Inter';
const OPSZ = 32;
const WEIGHTS = [100, 300, 400, 700, 900];
const SAMPLE = 'Ag'; // standard type-specimen pair — the double-story g shows weight change most

taetype.register_font_raw(FONT_NAME, fontBytes);

const instances = WEIGHTS.map((weight) => {
  const shaped = taetype.shape_text(SAMPLE, FONT_NAME, 'normal', weight, OPSZ, false);
  if (!shaped) throw new Error(`shape_text returned null at weight ${weight}`);

  const subset = taetype.subset_font_full(FONT_NAME, 'normal', weight, OPSZ, shaped.glyphs);
  if (!subset.fontBytes) throw new Error(`subset_font_full returned no fontBytes at weight ${weight}`);

  // subset_font_full() deliberately omits cmap (its origin use case addresses
  // glyphs by GID, e.g. for PDF embedding) — a browser needs one to render
  // normal text via @font-face, so build a minimal one mapping each shaped
  // character to its real post-subset glyph ID (shaped.clusters ties each
  // shaped glyph back to the character index that produced it; subset.glyphMap
  // is the real old-GID -> new-GID remap subset_ttf() returns).
  const mappings = [...SAMPLE].map((ch, charIdx) => {
    const glyphIdx = shaped.clusters.indexOf(charIdx);
    if (glyphIdx === -1) throw new Error(`no shaped glyph for character index ${charIdx}`);
    const origGid = shaped.glyphs[glyphIdx];
    const newGid = subset.glyphMap ? subset.glyphMap[origGid] : origGid;
    return { codepoint: ch.codePointAt(0), gid: newGid };
  });
  const fontFixed = addCmapAndName(subset.fontBytes, mappings, {
    family: `Inter Instance ${weight}`,
    subfamily: 'Regular',
    uniqueId: `taetype-demo-${weight}-${OPSZ}`,
    fullName: `Inter Instance ${weight}`,
    postscriptName: `InterInstance-${weight}`,
  });

  return {
    weight,
    opsz: OPSZ,
    bytes: fontFixed.length,
    fontBase64: fontFixed.toString('base64'),
  };
});

const result = {
  font: 'Inter (InterVariable.ttf)',
  license: 'SIL OFL 1.1',
  sample: SAMPLE,
  opsz: OPSZ,
  axesConfirmed: { wght: [100, 900], opsz: [14, 32] }, // parsed from the real fvar table, not assumed
  instances: instances.map(({ weight, opsz, bytes }) => ({ weight, opsz, bytes })), // log-friendly, no giant base64
};
console.log(result);

fs.writeFileSync(
  path.join(__dirname, '02-instancing-result.json'),
  JSON.stringify({ ...result, instances }, null, 2),
);
