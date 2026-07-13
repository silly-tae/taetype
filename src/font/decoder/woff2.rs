use std::collections::HashMap;
use super::io::{read_u16_be, read_u32_be, write_i16_be};
use super::glyf::unglyf_transform;
use brotli_decompressor::{BrotliDecompressCustomAlloc, StandardAlloc};

const WOFF2_SIGNATURE: u32 = 0x774F_4632;

// Decompresses WOFF2's own Brotli-compressed table stream fully in Rust/WASM
// — deliberately NOT the browser's DecompressionStream('brotli') Streams API,
// which a real render confirmed is missing in Brave on Windows ("Failed to
// construct 'DecompressionStream': Unsupported compression format: 'brotli'"),
// silently failing every font's registration and leaving the whole exported
// PDF text-less (preview stayed visually fine, since it never touches this
// path at all — it relies on the browser's own already-loaded font, not this
// engine's registration). Doing the decompression here removes the dependency
// on any browser's Streams-API support entirely, rather than feature-
// detecting and working around one browser's gap — the same "own the whole
// stack" choice this project already made for AES/SHA (crypto_r6.ts) instead
// of trusting a platform primitive's availability.
pub fn decompress_brotli(compressed: &[u8]) -> Result<Vec<u8>, String> {
    let mut input: &[u8] = compressed;
    let mut output: Vec<u8> = Vec::new();
    let mut input_buffer = [0u8; 4096];
    let mut output_buffer = [0u8; 4096];
    BrotliDecompressCustomAlloc(
        &mut input,
        &mut output,
        &mut input_buffer,
        &mut output_buffer,
        StandardAlloc::default(),
        StandardAlloc::default(),
        StandardAlloc::default(),
    ).map_err(|e| format!("Brotli decompression failed: {}", e))?;
    Ok(output)
}

const KNOWN_TAGS: &[&str] = &[
    "cmap", "head", "hhea", "hmtx", "maxp", "name", "OS/2", "post",
    "cvt ", "fpgm", "glyf", "loca", "prep", "CFF ", "VORG",
    "EBDT", "EBLC", "gasp", "hdmx", "kern", "LTSH", "PCLT", "VDMX", "vhea", "vmtx",
    "BASE", "GDEF", "GPOS", "GSUB", "EBSC", "JSTF", "MATH",
    "CBDT", "CBLC", "COLR", "CPAL", "SVG ", "sbix",
    "acnt", "avar", "bdat", "bloc", "bsln", "cvar", "fdsc", "feat", "fmtx", "fvar",
    "gvar", "hsty", "just", "lcar", "mort", "morx", "opbd", "prop", "trak",
    "Zapf", "Silf", "Glat", "Gloc", "Feat", "Sill",
];

fn read_uint_base128(data: &[u8], off: usize) -> Result<(u32, usize), String> {
    let mut value: u32 = 0;
    for i in 0..5 {
        if off + i >= data.len() { return Err("UIntBase128: unexpected end of data".into()); }
        let b = data[off + i];
        // spec: leading zeros are forbidden, and the accumulated value must not
        // overflow u32 — the shift below would silently wrap in release builds
        if i == 0 && b == 0x80 { return Err("UIntBase128: leading zero".into()); }
        if value & 0xFE00_0000 != 0 { return Err("UIntBase128 overflow".into()); }
        value = (value << 7) | (b & 0x7F) as u32;
        if b & 0x80 == 0 {
            return Ok((value, i + 1));
        }
    }
    Err("UIntBase128 overflow".into())
}

pub fn read_255_uint16(data: &[u8], off: usize) -> Result<(u16, usize), String> {
    let b = match data.get(off) {
        Some(&b) => b,
        None => return Err("255_uint16: unexpected end of data".into()),
    };
    if b < 253 {
        Ok((b as u16, 1))
    } else if b == 253 {
        let hi = data.get(off + 1).copied().ok_or_else(|| "255_uint16: unexpected end of data".to_string())? as u16;
        let lo = data.get(off + 2).copied().ok_or_else(|| "255_uint16: unexpected end of data".to_string())? as u16;
        Ok(((hi << 8) | lo, 3))
    } else if b == 254 {
        let v = data.get(off + 1).copied().ok_or_else(|| "255_uint16: unexpected end of data".to_string())? as u16;
        Ok((v + 506, 2))
    } else {
        let v = data.get(off + 1).copied().ok_or_else(|| "255_uint16: unexpected end of data".to_string())? as u16;
        Ok((v + 253, 2))
    }
}

struct TableEntry {
    tag:              String,
    transformed:      bool,
    orig_length:      u32,
    transform_length: u32,
}

const TTCF_FLAVOR: u32 = 0x7474_6366;

fn parse_header(data: &[u8]) -> Result<(u16, u32, u32), String> {
    if data.len() < 48 {
        return Err("WOFF2: file too short".into());
    }
    let sig = read_u32_be(data, 0).ok_or("WOFF2: header truncated")?;
    if sig != WOFF2_SIGNATURE {
        return Err("Not a WOFF2 file".into());
    }
    let flavor                = read_u32_be(data, 4).ok_or("WOFF2: header truncated")?;
    let num_tables            = read_u16_be(data, 12).ok_or("WOFF2: header truncated")?;
    let total_compressed_size = read_u32_be(data, 20).ok_or("WOFF2: header truncated")?;
    Ok((num_tables, total_compressed_size, flavor))
}

// 'ttcf'-flavored WOFF2 carries a CollectionDirectory between the table
// directory and the brotli stream: version, numFonts, then per font a table
// count, flavor, and indices into the shared table directory. Returns the
// requested font's table indices plus the directory's end offset (= brotli
// start) — the end offset doesn't depend on which font is selected, since the
// loop always walks every font's entry to find it.
fn parse_collection_dir(data: &[u8], off: usize, n_tables: usize, index: usize) -> Result<(Vec<usize>, usize), String> {
    let mut pos = off + 4;
    let (num_fonts, br) = read_255_uint16(data, pos)?;
    pos += br;
    if num_fonts == 0 {
        return Err("WOFF2 collection: zero fonts".into());
    }
    if index >= num_fonts as usize {
        return Err(format!("WOFF2 collection: font index {} out of range ({} fonts)", index, num_fonts));
    }
    let mut selected: Vec<usize> = Vec::new();
    for f in 0..num_fonts {
        let (font_tables, br) = read_255_uint16(data, pos)?;
        pos += br + 4;
        for _ in 0..font_tables {
            let (idx, br) = read_255_uint16(data, pos)?;
            pos += br;
            if idx as usize >= n_tables {
                return Err("WOFF2 collection: table index out of range".into());
            }
            if f as usize == index { selected.push(idx as usize); }
        }
    }
    Ok((selected, pos))
}

fn parse_table_dir(data: &[u8], num_tables: u16, header_end: usize) -> Result<(Vec<TableEntry>, usize), String> {
    let mut tables = Vec::new();
    let mut off = header_end;

    for _ in 0..num_tables {
        let flags = *data.get(off).ok_or("WOFF2: table directory truncated")?;
        off += 1;
        let tag_idx           = (flags & 0x3F) as usize;
        let transform_version = (flags >> 6) & 0x3;

        let tag = if tag_idx == 63 {
            let tag_bytes = data.get(off..off + 4)
                .ok_or("WOFF2: table tag bytes truncated")?;
            let s = std::str::from_utf8(tag_bytes)
                .map_err(|_| "Invalid table tag bytes".to_string())?
                .to_string();
            off += 4;
            s
        } else {
            KNOWN_TAGS.get(tag_idx)
                .ok_or_else(|| format!("Unknown tag index: {}", tag_idx))?
                .to_string()
        };

        let (orig_length, br1) = read_uint_base128(data, off)?;
        off += br1;

        // transformLength is present iff the table is transformed: for glyf/loca,
        // version 0 means transformed and version 3 is the null transform; for all
        // other tables it is the reverse (0 = null). Reading it unconditionally for
        // glyf/loca misparses the whole directory on null-transformed fonts.
        let transformed = if tag == "glyf" || tag == "loca" {
            transform_version == 0
        } else {
            transform_version != 0
        };
        let transform_length = if transformed {
            let (tl, br2) = read_uint_base128(data, off)?;
            off += br2;
            tl
        } else {
            orig_length
        };

        tables.push(TableEntry { tag, transformed, orig_length, transform_length });
    }

    Ok((tables, off))
}

// per-entry extraction keyed by directory index — a collection's directory
// repeats tags across member fonts, so a tag-keyed map would clobber entries
fn extract_tables_indexed(decompressed: &[u8], tables: &[TableEntry]) -> Result<Vec<Vec<u8>>, String> {
    let mut out = Vec::with_capacity(tables.len());
    let mut off = 0;
    for t in tables {
        // a transformed table occupies transformLength bytes in the decompressed
        // stream — advancing by origLength would misalign every table after it
        let len = if t.transformed {
            t.transform_length as usize
        } else {
            t.orig_length as usize
        };
        let bytes = decompressed.get(off..off + len)
            .ok_or_else(|| format!("WOFF2: table '{}' data out of bounds", t.tag))?;
        out.push(bytes.to_vec());
        off += len;
    }
    Ok(out)
}

pub fn get_compressed_range(woff2_bytes: &[u8]) -> Result<(usize, usize), String> {
    let (num_tables, total_compressed_size, flavor) = parse_header(woff2_bytes)?;
    let (tables, dir_end) = parse_table_dir(woff2_bytes, num_tables, 48)?;
    // the directory's end offset doesn't depend on which font is selected —
    // index 0 always exists (num_fonts >= 1), so it's a safe placeholder here
    let brotli_start = if flavor == TTCF_FLAVOR {
        parse_collection_dir(woff2_bytes, dir_end, tables.len(), 0)?.1
    } else {
        dir_end
    };
    Ok((brotli_start, total_compressed_size as usize))
}

// `index` selects which member font of a 'ttcf'-flavored collection to
// extract; ignored (any value accepted) for a plain, non-collection WOFF2.
pub fn decode_woff2_tables(woff2_bytes: &[u8], decompressed: &[u8], index: usize) -> Result<HashMap<String, Vec<u8>>, String> {
    let (num_tables, _, flavor) = parse_header(woff2_bytes)?;
    let (tables, dir_end) = parse_table_dir(woff2_bytes, num_tables, 48)?;

    let extracted = extract_tables_indexed(decompressed, &tables)?;
    let selected: Vec<usize> = if flavor == TTCF_FLAVOR {
        parse_collection_dir(woff2_bytes, dir_end, tables.len(), index)?.0
    } else {
        (0..tables.len()).collect()
    };

    let mut table_map = HashMap::new();
    for &i in &selected {
        table_map.insert(tables[i].tag.clone(), extracted[i].clone());
    }

    // version 0 for glyf means transformed, unconditionally — the stored loca is
    // empty and MUST be reconstructed (the old size-comparison heuristic left loca
    // empty whenever the transformed glyf happened to match the original size)
    let glyf_transformed = selected.iter().any(|&i| tables[i].tag == "glyf" && tables[i].transformed);
    if glyf_transformed {
        let transformed_bytes = table_map.get("glyf")
            .ok_or_else(|| "glyf table missing after extract".to_string())?
            .clone();
        let (new_glyf, new_loca, index_format) = unglyf_transform(&transformed_bytes)?;
        table_map.insert("glyf".to_string(), new_glyf);
        table_map.insert("loca".to_string(), new_loca);
        if let Some(head) = table_map.get_mut("head") {
            // write-site guard: head table is untrusted font data, not self-allocated —
            // must be long enough for the indexFormat field at offset 50 before writing.
            if head.len() >= 52 {
                write_i16_be(head, 50, index_format);
            }
        }
    }

    if selected.iter().any(|&i| tables[i].tag == "hmtx" && tables[i].transformed) {
        let transformed_bytes = table_map.get("hmtx")
            .ok_or_else(|| "hmtx table missing after extract".to_string())?
            .clone();
        let hmtx = unhmtx_transform(&transformed_bytes, &table_map)?;
        table_map.insert("hmtx".to_string(), hmtx);
    }

    Ok(table_map)
}

// Reverse of the WOFF2 hmtx transform (version 1): the encoder omits lsb arrays
// whose values equal each glyph's xMin, so they are rebuilt from the reconstructed
// glyf. Requires glyf/loca/maxp/hhea to already be in their final form.
fn unhmtx_transform(t: &[u8], table_map: &HashMap<String, Vec<u8>>) -> Result<Vec<u8>, String> {
    use super::io::{read_i16_be, write_u16_be};

    let err = || "hmtx transform: data truncated".to_string();
    let flags = *t.first().ok_or_else(err)?;
    let lsb_omitted      = flags & 0x01 != 0;
    let left_sb_omitted  = flags & 0x02 != 0;

    let maxp = table_map.get("maxp").ok_or("hmtx transform: missing maxp")?;
    let hhea = table_map.get("hhea").ok_or("hmtx transform: missing hhea")?;
    let num_glyphs  = read_u16_be(maxp, 4).ok_or("hmtx transform: maxp truncated")? as usize;
    let num_hmetrics = read_u16_be(hhea, 34).ok_or("hmtx transform: hhea truncated")? as usize;
    if num_hmetrics == 0 || num_hmetrics > num_glyphs {
        return Err("hmtx transform: invalid numberOfHMetrics".into());
    }

    let glyf = table_map.get("glyf").ok_or("hmtx transform: missing glyf")?;
    let loca = table_map.get("loca").ok_or("hmtx transform: missing loca")?;
    let head = table_map.get("head").ok_or("hmtx transform: missing head")?;
    let loca_format = read_i16_be(head, 50).ok_or("hmtx transform: head truncated")?;
    let glyph_x_min = |gid: usize| -> i16 {
        let off = if loca_format == 0 {
            read_u16_be(loca, gid * 2).map(|v| v as usize * 2)
        } else {
            read_u32_be(loca, gid * 4).map(|v| v as usize)
        };
        let next = if loca_format == 0 {
            read_u16_be(loca, gid * 2 + 2).map(|v| v as usize * 2)
        } else {
            read_u32_be(loca, gid * 4 + 4).map(|v| v as usize)
        };
        match (off, next) {
            // empty glyphs have no outline and lsb 0
            (Some(o), Some(n)) if n > o => read_i16_be(glyf, o + 2).unwrap_or(0),
            _ => 0,
        }
    };

    let mut pos = 1usize;
    let mut advances = Vec::with_capacity(num_hmetrics);
    for _ in 0..num_hmetrics {
        advances.push(read_u16_be(t, pos).ok_or_else(err)?);
        pos += 2;
    }
    let mut lsbs = Vec::with_capacity(num_hmetrics);
    if lsb_omitted {
        for gid in 0..num_hmetrics { lsbs.push(glyph_x_min(gid)); }
    } else {
        for _ in 0..num_hmetrics {
            lsbs.push(read_i16_be(t, pos).ok_or_else(err)?);
            pos += 2;
        }
    }
    let mut left_sbs = Vec::with_capacity(num_glyphs - num_hmetrics);
    if left_sb_omitted {
        for gid in num_hmetrics..num_glyphs { left_sbs.push(glyph_x_min(gid)); }
    } else {
        for _ in num_hmetrics..num_glyphs {
            left_sbs.push(read_i16_be(t, pos).ok_or_else(err)?);
            pos += 2;
        }
    }

    let mut out = vec![0u8; num_hmetrics * 4 + (num_glyphs - num_hmetrics) * 2];
    for i in 0..num_hmetrics {
        write_u16_be(&mut out, i * 4, advances[i]);
        write_u16_be(&mut out, i * 4 + 2, lsbs[i] as u16);
    }
    for (i, &lsb) in left_sbs.iter().enumerate() {
        write_u16_be(&mut out, num_hmetrics * 4 + i * 2, lsb as u16);
    }
    Ok(out)
}
