// Phase 4 (representation-todo.md): the "faithful path" the task-map flagged as
// the hard decision — draws taetype's OWN shape_text() glyph run, glyph by glyph,
// at taetype's own advances, rather than letting the browser reshape a real font
// via fillText (which would only prove the FONT has ligatures, not that taetype
// computed them). Builds two real, independent font files: one containing only
// the naive per-character glyphs (get_glyph_ids, no shaping), one containing only
// the real shaped glyph run (shape_text) — each remapped to its own Private Use
// Area codepoints via the same addCmapAndName() glue phase 2 built, so each glyph
// is individually addressable via ordinary fillText calls, positioned by hand at
// taetype's real cumulative advances.

const fs = require('fs');
const path = require('path');
const { addCmapAndName } = require('./lib/sfnt-tools');

const repoRoot = path.join(__dirname, '..', '..');
const taetype = require(path.join(repoRoot, 'pkg', 'node', 'taetype.js'));

const fontPath = path.join(repoRoot, 'assets', 'fonts', 'eb-garamond', 'EBGaramond.ttf');
const fontBytes = fs.readFileSync(fontPath);

const FONT_NAME = 'EBGaramond';
const WEIGHT = 400;
const OPSZ = 0;
const WORD = 'office'; // real 'ffi' ligature confirmed live before picking this

taetype.register_font_raw(FONT_NAME, fontBytes);

// builds a PUA-remapped, browser-loadable font from any (gids, per-glyph info)
// pair — shared by both the naive and shaped paths below
function buildPuaFont(gids, familySuffix) {
  const subset = taetype.subset_font_full(FONT_NAME, 'normal', WEIGHT, OPSZ, gids);
  if (!subset.fontBytes) throw new Error(`subset_font_full returned no fontBytes for ${familySuffix}`);
  const mappings = gids.map((origGid, i) => ({ codepoint: 0xe000 + i, gid: subset.glyphMap[origGid] }));
  const fixed = addCmapAndName(subset.fontBytes, mappings, {
    family: `EBGaramondDemo${familySuffix}`,
    subfamily: 'Regular',
    uniqueId: `taetype-demo-shaping-${familySuffix}`,
    fullName: `EBGaramondDemo${familySuffix}`,
    postscriptName: `EBGaramondDemo-${familySuffix}`,
  });
  return { fontBase64: fixed.toString('base64'), bytes: fixed.length };
}

// Naive path: per-character cmap lookup only, no GSUB — the "before"
const naiveGids = Array.from(taetype.get_glyph_ids(WORD, FONT_NAME, 'normal', WEIGHT));
const naiveWidths = Array.from(taetype.get_advance_widths(FONT_NAME, 'normal', WEIGHT, OPSZ, new Uint16Array(naiveGids)));
const naiveFont = buildPuaFont(naiveGids, 'Naive');

// Shaped path: real shape_text() output, real GSUB ligature substitution — the "after"
const shaped = taetype.shape_text(WORD, FONT_NAME, 'normal', WEIGHT, OPSZ, false);
if (!shaped) throw new Error('shape_text returned null');
const shapedGids = Array.from(shaped.glyphs);
const shapedAdvances = Array.from(shaped.advances);
const clusters = Array.from(shaped.clusters);
const shapedFont = buildPuaFont(shapedGids, 'Shaped');

// which shaped glyph is the ligature? the one whose cluster span covers more
// than one source character (clusters[i+1] - clusters[i] > 1, or to the end
// of the word for the last glyph)
const ligatureIndex = clusters.findIndex((c, i) => {
  const next = i + 1 < clusters.length ? clusters[i + 1] : WORD.length;
  return next - c > 1;
});
const ligatureSpan =
  ligatureIndex === -1
    ? null
    : WORD.slice(clusters[ligatureIndex], clusters[ligatureIndex + 1] ?? WORD.length);

const result = {
  font: 'EB Garamond (variable)',
  license: 'SIL OFL 1.1',
  word: WORD,
  naive: { gids: naiveGids, widths: naiveWidths, totalWidth: naiveWidths.reduce((a, b) => a + b, 0), ...naiveFont },
  shaped: {
    gids: shapedGids,
    advances: shapedAdvances,
    clusters,
    totalWidth: shapedAdvances.reduce((a, b) => a + b, 0),
    ligatureIndex,
    ligatureSpan,
    ...shapedFont,
  },
};

console.log({
  word: result.word,
  naive: { glyphCount: naiveGids.length, totalWidth: result.naive.totalWidth, bytes: naiveFont.bytes },
  shaped: {
    glyphCount: shapedGids.length,
    totalWidth: result.shaped.totalWidth,
    ligatureSpan,
    bytes: shapedFont.bytes,
  },
});

fs.writeFileSync(path.join(__dirname, '04-shaping-result.json'), JSON.stringify(result, null, 2));
