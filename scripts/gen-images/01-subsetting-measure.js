// Phase 1 (representation-todo.md): measures a real taetype subsetting pass –
// original font bytes vs. the bytes subset_font_full() returns for one realistic
// paragraph – and writes the numbers to 01-subsetting-result.json for the chart
// step to consume. No fabricated numbers: every value here comes from actually
// running the pkg/node build against a real OFL font file.

const fs = require('fs');
const path = require('path');

const repoRoot = path.join(__dirname, '..', '..');
const taetype = require(path.join(repoRoot, 'pkg', 'node', 'taetype.js'));

const fontPath = path.join(repoRoot, 'assets', 'fonts', 'inter', 'InterVariable.ttf');
const fontBytes = fs.readFileSync(fontPath);

const FONT_NAME = 'Inter';
const WEIGHT = 400;
const OPSZ = 0;

taetype.register_font_raw(FONT_NAME, fontBytes);

const registered = taetype.list_registered_fonts();
if (!registered.includes(`${FONT_NAME.toLowerCase()}:normal`)) {
  throw new Error(`unexpected registry state: ${JSON.stringify(registered)}`);
}

// A realistic body paragraph, not a pangram – pangrams artificially inflate
// glyph coverage and would understate the real-world subsetting win.
const text =
  'Type is the voice of the page before a single word is read. The right ' +
  'typeface sets pace, mood, and hierarchy long before meaning arrives, and ' +
  'the space between weights carries most of what a reader feels without ' +
  'noticing why. Getting that right on the web means shipping exactly the ' +
  'glyphs a page needs, nothing more, so the browser can paint text as fast ' +
  'as it paints anything else.';

const shaped = taetype.shape_text(text, FONT_NAME, 'normal', WEIGHT, OPSZ, false);
if (!shaped) {
  throw new Error('shape_text returned null – font not shapeable by rustybuzz');
}

const uniqueGlyphs = new Set(shaped.glyphs).size;

const subset = taetype.subset_font_full(FONT_NAME, 'normal', WEIGHT, OPSZ, shaped.glyphs);
if (!subset.fontBytes) {
  throw new Error('subset_font_full returned no fontBytes');
}

// maxp.numGlyphs, read directly from the sfnt table directory – not exposed by
// taetype's public API, so parsed the same way ttf_dir.rs does internally.
function readTotalGlyphs(buf) {
  const numTables = buf.readUInt16BE(4);
  for (let i = 0; i < numTables; i++) {
    const rec = 12 + i * 16;
    if (buf.toString('ascii', rec, rec + 4) === 'maxp') {
      const off = buf.readUInt32BE(rec + 8);
      return buf.readUInt16BE(off + 4);
    }
  }
  throw new Error('maxp table not found');
}
const totalGlyphs = readTotalGlyphs(fontBytes);

const originalBytes = fontBytes.length;
const subsetBytes = subset.fontBytes.length;
const reductionPct = ((1 - subsetBytes / originalBytes) * 100).toFixed(1);

const result = {
  font: 'Inter (InterVariable.ttf)',
  license: 'SIL OFL 1.1',
  weight: WEIGHT,
  sampleText: text,
  sampleTextLength: text.length,
  uniqueGlyphsUsed: uniqueGlyphs,
  totalGlyphs,
  originalBytes,
  subsetBytes,
  reductionPct: Number(reductionPct),
};

console.log(result);

fs.writeFileSync(
  path.join(__dirname, '01-subsetting-result.json'),
  JSON.stringify(result, null, 2),
);
