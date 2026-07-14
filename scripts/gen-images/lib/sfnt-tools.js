// Reusable sfnt read/write helpers for the gen-images scripts. Not part of
// taetype itself – taetype's subset_font_full() deliberately omits cmap/name
// (its origin use case, PDF embedding, addresses glyphs by GID directly, so a
// character-to-glyph map is dead weight there). Browsers rendering text via
// @font-face + fillText/DOM text need cmap to resolve characters to glyphs, so
// this module adds the minimal one back on, purely as demo-script glue.

function readU16(buf, off) { return buf.readUInt16BE(off); }
function readU32(buf, off) { return buf.readUInt32BE(off); }

function parseTables(buf) {
  const numTables = readU16(buf, 4);
  const tables = {};
  for (let i = 0; i < numTables; i++) {
    const rec = 12 + i * 16;
    const tag = buf.toString('ascii', rec, rec + 4);
    const offset = readU32(buf, rec + 8);
    const length = readU32(buf, rec + 12);
    tables[tag] = buf.subarray(offset, offset + length);
  }
  return tables;
}

function checksum32(buf) {
  let sum = 0;
  const words = Math.floor(buf.length / 4);
  for (let i = 0; i < words; i++) sum = (sum + buf.readUInt32BE(i * 4)) >>> 0;
  const rem = buf.length % 4;
  if (rem > 0) {
    let last = 0;
    for (let i = 0; i < rem; i++) last |= buf[buf.length - rem + i] << ((3 - i) * 8);
    sum = (sum + (last >>> 0)) >>> 0;
  }
  return sum >>> 0;
}

function pad4(n) { return (n + 3) & ~3; }

// Mirrors taetype's own font::decoder::build_ttf (src/font/decoder/ttf.rs) —
// same table-directory layout, same head.checksumAdjustment algorithm, so a
// font this function builds is structurally identical to what taetype itself
// would produce, just with one extra table.
function buildFont(tableMap) {
  const tags = Object.keys(tableMap).sort();
  const numTables = tags.length;
  const floorLog2 = 31 - Math.clz32(numTables);
  const searchRange = (1 << floorLog2) * 16;
  const entrySelector = floorLog2;
  const rangeShift = numTables * 16 - searchRange;

  const sfntHdr = 12;
  const dirSize = numTables * 16;
  let dataOff = sfntHdr + dirSize;

  const offsets = [];
  const padded = [];
  for (const tag of tags) {
    const raw = tableMap[tag];
    const p = Buffer.alloc(pad4(raw.length));
    raw.copy(p);
    offsets.push(dataOff);
    dataOff += p.length;
    padded.push(p);
  }

  const out = Buffer.alloc(dataOff);
  out.writeUInt32BE(0x00010000, 0);
  out.writeUInt16BE(numTables, 4);
  out.writeUInt16BE(searchRange, 6);
  out.writeUInt16BE(entrySelector, 8);
  out.writeUInt16BE(rangeShift, 10);

  let dirOff = sfntHdr;
  tags.forEach((tag, i) => {
    const cs = checksum32(padded[i]);
    out.write(tag, dirOff, 4, 'ascii');
    out.writeUInt32BE(cs, dirOff + 4);
    out.writeUInt32BE(offsets[i], dirOff + 8);
    out.writeUInt32BE(tableMap[tag].length, dirOff + 12);
    dirOff += 16;
  });

  tags.forEach((tag, i) => padded[i].copy(out, offsets[i]));

  const headIdx = tags.indexOf('head');
  if (headIdx !== -1) {
    const headOff = offsets[headIdx];
    out.writeUInt32BE(0, headOff + 8); // zero checksumAdjustment before computing
    const fileCs = checksum32(out);
    out.writeUInt32BE((0xb1b0afba - fileCs) >>> 0, headOff + 8);
  }

  return out;
}

// Format 4, one segment per requested codepoint (each idRangeOffset=0, so
// glyph = (codepoint + idDelta) & 0xFFFF — no glyphIdArray needed) plus the
// mandatory 0xFFFF terminator segment. Fine for a handful of isolated BMP
// codepoints, which is all these demo scripts ever need; a general-purpose
// tool would want real segment merging and format 12 for astral codepoints.
function buildFormat4Cmap(mappings) {
  const sorted = [...mappings].sort((a, b) => a.codepoint - b.codepoint);
  const segs = sorted.map((m) => {
    const deltaU16 = (m.gid - m.codepoint) & 0xffff;
    const delta = deltaU16 > 0x7fff ? deltaU16 - 0x10000 : deltaU16; // to signed int16 for writeInt16BE
    return { start: m.codepoint, end: m.codepoint, delta };
  });
  segs.push({ start: 0xffff, end: 0xffff, delta: 1 });

  const segCount = segs.length;
  const floorLog2 = 31 - Math.clz32(segCount);
  const searchRange = 2 * Math.pow(2, floorLog2);
  const entrySelector = floorLog2;
  const rangeShift = segCount * 2 - searchRange;

  const headerLen = 14;
  const arraysLen = segCount * 2 * 4 + 2; // end+start+delta+rangeOffset (2B each) + reservedPad
  const subtableLen = headerLen + arraysLen;

  const sub = Buffer.alloc(subtableLen);
  sub.writeUInt16BE(4, 0); // format
  sub.writeUInt16BE(subtableLen, 2);
  sub.writeUInt16BE(0, 4); // language
  sub.writeUInt16BE(segCount * 2, 6);
  sub.writeUInt16BE(searchRange, 8);
  sub.writeUInt16BE(entrySelector, 10);
  sub.writeUInt16BE(rangeShift, 12);

  let p = headerLen;
  for (const s of segs) { sub.writeUInt16BE(s.end, p); p += 2; }
  p += 2; // reservedPad
  for (const s of segs) { sub.writeUInt16BE(s.start, p); p += 2; }
  for (const s of segs) { sub.writeInt16BE(s.delta, p); p += 2; }
  for (const _s of segs) { sub.writeUInt16BE(0, p); p += 2; } // idRangeOffset, all 0

  // OTS (Chromium's font sanitizer) rejects the whole font if encoding
  // records aren't sorted by (platformID, encodingID) ascending — confirmed
  // by the actual "subtable 1 ... following subtable ..." OTS error.
  const encodingRecords = [
    { platformID: 0, encodingID: 3 }, // Unicode 2.0 BMP
    { platformID: 3, encodingID: 1 }, // Windows, Unicode BMP – what browsers look for first
  ];
  const cmapHeaderLen = 4 + encodingRecords.length * 8;
  const subtableOffset = cmapHeaderLen;

  const cmap = Buffer.alloc(cmapHeaderLen + sub.length);
  cmap.writeUInt16BE(0, 0); // version
  cmap.writeUInt16BE(encodingRecords.length, 2);
  encodingRecords.forEach((rec, i) => {
    const off = 4 + i * 8;
    cmap.writeUInt16BE(rec.platformID, off);
    cmap.writeUInt16BE(rec.encodingID, off + 2);
    cmap.writeUInt32BE(subtableOffset, off + 4);
  });
  sub.copy(cmap, subtableOffset);

  return cmap;
}

// Adds a minimal cmap (see buildFormat4Cmap) to a taetype subset_font_full()
// output so a browser can load it via @font-face and render normal text with
// it, not just draw pre-shaped glyph runs.
function addCmap(fontBytes, mappings) {
  const buf = Buffer.isBuffer(fontBytes) ? fontBytes : Buffer.from(fontBytes);
  const tables = parseTables(buf);
  const tableMap = {};
  for (const [tag, data] of Object.entries(tables)) tableMap[tag] = Buffer.from(data);
  tableMap.cmap = buildFormat4Cmap(mappings);
  return buildFont(tableMap);
}

// Chromium's OTS sanitizer rejects a font with no `name` table ("name: missing
// required table") the same way it rejects one with no usable cmap –
// subset_font_full() omits this too (its origin use case doesn't need it),
// so this builds the minimal required NameIDs (1/2/3/4/6) as platform 3
// (Windows), encoding 1 (Unicode BMP), language 0x0409 (en-US) – the same
// platform/encoding taetype's own read_font_family_name() scores highest.
function buildNameTable({ family, subfamily, uniqueId, fullName, postscriptName }) {
  const records = [
    { nameID: 1, value: family },
    { nameID: 2, value: subfamily },
    { nameID: 3, value: uniqueId },
    { nameID: 4, value: fullName },
    { nameID: 6, value: postscriptName },
  ];

  const strings = records.map((r) => Buffer.from(r.value, 'utf16le').swap16()); // UTF-16BE
  const headerLen = 6;
  const recordLen = 12;
  const stringStorageOffset = headerLen + records.length * recordLen;

  let strOff = 0;
  const nameRecords = records.map((r, i) => {
    const rec = { platformID: 3, encodingID: 1, languageID: 0x0409, nameID: r.nameID, length: strings[i].length, offset: strOff };
    strOff += strings[i].length;
    return rec;
  });

  const totalLen = stringStorageOffset + strOff;
  const buf = Buffer.alloc(totalLen);
  buf.writeUInt16BE(0, 0); // format
  buf.writeUInt16BE(records.length, 2);
  buf.writeUInt16BE(stringStorageOffset, 4);

  nameRecords.forEach((rec, i) => {
    const off = headerLen + i * recordLen;
    buf.writeUInt16BE(rec.platformID, off);
    buf.writeUInt16BE(rec.encodingID, off + 2);
    buf.writeUInt16BE(rec.languageID, off + 4);
    buf.writeUInt16BE(rec.nameID, off + 6);
    buf.writeUInt16BE(rec.length, off + 8);
    buf.writeUInt16BE(rec.offset, off + 10);
  });

  strings.forEach((s, i) => s.copy(buf, stringStorageOffset + nameRecords[i].offset));

  return buf;
}

// subset_font_full() carries the ORIGINAL font's post table over verbatim
// (src/font/subsetter/mod.rs:435-436) even though maxp.numGlyphs is correctly
// resized to the subset — a real internal inconsistency, not just a missing
// table like cmap/name. OTS confirms it: "post: Bad number of glyphs: 2937"
// (the original font's total, not the subset's handful). The first 32 bytes
// of every post version share the same layout (version, italicAngle,
// underline metrics, isFixedPitch, memory hints); only the version-2.0
// per-glyph name array afterward references glyph count. Downgrading to
// version 3.0 (header only, no per-glyph data) keeps the real italic-angle/
// underline metrics and makes the glyph-count mismatch structurally
// impossible, since v3.0 carries no per-glyph data to mismatch.
function fixPostTable(postBuf) {
  const header = Buffer.from(postBuf.subarray(0, 32));
  header.writeUInt32BE(0x00030000, 0); // version 3.0
  return header;
}

// Adds cmap + name, and downgrades post – see each fix's doc comment for why
// subset_font_full() needs this glue before a browser will accept it as a
// webfont at all (cmap/name: missing required tables; post: real internal
// glyph-count inconsistency in the subsetter's table-copying logic).
function addCmapAndName(fontBytes, mappings, nameStrings) {
  const buf = Buffer.isBuffer(fontBytes) ? fontBytes : Buffer.from(fontBytes);
  const tables = parseTables(buf);
  const tableMap = {};
  for (const [tag, data] of Object.entries(tables)) tableMap[tag] = Buffer.from(data);
  tableMap.cmap = buildFormat4Cmap(mappings);
  tableMap.name = buildNameTable(nameStrings);
  if (tableMap.post) tableMap.post = fixPostTable(tableMap.post);
  return buildFont(tableMap);
}

module.exports = { parseTables, buildFont, buildFormat4Cmap, buildNameTable, fixPostTable, addCmap, addCmapAndName };
