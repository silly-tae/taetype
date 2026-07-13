use super::super::decoder::{read_u16_be, read_u32_be};

pub fn cmap_glyph_id(cmap: &[u8], codepoint: u32) -> u16 {
    if cmap.len() < 4 { return 0; }
    let num_tables = match read_u16_be(cmap, 2) { Some(v) => v as usize, None => return 0 };
    let mut f4_result: u16 = 0;
    // legacy subtables (trimmed format 6, byte-encoded format 0) rank below
    // format 4 — some old fonts carry ONLY these, and without them every
    // codepoint resolved to GID 0 and the text vanished from the PDF
    let mut legacy_result: u16 = 0;

    for i in 0..num_tables {
        let rec = 4 + i * 8;
        if rec + 8 > cmap.len() { break; }
        let platform_id  = match read_u16_be(cmap, rec)     { Some(v) => v, None => break };
        let encoding_id   = match read_u16_be(cmap, rec + 2) { Some(v) => v, None => break };
        let subtable_off  = match read_u32_be(cmap, rec + 4) { Some(v) => v as usize, None => break };
        if subtable_off + 2 > cmap.len() { continue; }
        let format = match read_u16_be(cmap, subtable_off) { Some(v) => v, None => continue };

        match (platform_id, encoding_id, format) {
            (0, _, 12) | (3, 10, 12) => {
                let gid = format12_lookup(cmap, subtable_off, codepoint);
                if gid != 0 { return gid; }
            }
            (0, _, 4) | (3, 1, 4) if codepoint <= 0xFFFF => {
                if f4_result == 0 {
                    f4_result = format4_lookup(cmap, subtable_off, codepoint as u16);
                }
            }
            (_, _, 6) if codepoint <= 0xFFFF => {
                if legacy_result == 0 {
                    legacy_result = format6_lookup(cmap, subtable_off, codepoint as u16);
                }
            }
            (_, _, 0) if codepoint <= 0xFF => {
                if legacy_result == 0 {
                    legacy_result = format0_lookup(cmap, subtable_off, codepoint as u8);
                }
            }
            _ => {}
        }
    }
    if f4_result != 0 { f4_result } else { legacy_result }
}

// format 6: trimmed table — firstCode + contiguous glyphIdArray[entryCount]
fn format6_lookup(cmap: &[u8], base: usize, cp: u16) -> u16 {
    let first = match read_u16_be(cmap, base + 6) { Some(v) => v, None => return 0 };
    let count = match read_u16_be(cmap, base + 8) { Some(v) => v, None => return 0 };
    if cp < first { return 0; }
    let idx = (cp - first) as usize;
    if idx >= count as usize { return 0; }
    read_u16_be(cmap, base + 10 + idx * 2).unwrap_or(0)
}

// format 0: 256-entry byte mapping
fn format0_lookup(cmap: &[u8], base: usize, cp: u8) -> u16 {
    cmap.get(base + 6 + cp as usize).copied().unwrap_or(0) as u16
}

fn format12_lookup(cmap: &[u8], base: usize, codepoint: u32) -> u16 {
    if base + 16 > cmap.len() { return 0; }
    let num_groups  = match read_u32_be(cmap, base + 12) { Some(v) => v as usize, None => return 0 };
    let groups_base = base + 16;
    if groups_base + num_groups * 12 > cmap.len() { return 0; }

    let mut lo = 0usize;
    let mut hi = num_groups;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let off = groups_base + mid * 12;
        let (Some(start), Some(end)) = (read_u32_be(cmap, off), read_u32_be(cmap, off + 4)) else { return 0 };
        if codepoint < start {
            hi = mid;
        } else if codepoint > end {
            lo = mid + 1;
        } else {
            let gid = match read_u32_be(cmap, off + 8) { Some(v) => v as u64 + (codepoint - start) as u64, None => return 0 };
            return if gid > 0xFFFF { 0 } else { gid as u16 };
        }
    }
    0
}

fn format4_lookup(cmap: &[u8], base: usize, cp: u16) -> u16 {
    if base + 14 > cmap.len() { return 0; }
    let seg_count = match read_u16_be(cmap, base + 6) { Some(v) => v as usize / 2, None => return 0 };

    let end_off   = base + 14;
    let start_off = end_off + seg_count * 2 + 2; // +2 for reservedPad
    let delta_off = start_off + seg_count * 2;
    let range_off = delta_off + seg_count * 2;

    // seg_count is attacker-controlled (via segCountX2 in the cmap subtable header) and can
    // exceed what actually fits in cmap — each read_* below returns None rather than panicking
    // once the loop runs past the real data, so a crafted/truncated table is rejected gracefully.
    for i in 0..seg_count {
        let end = match read_u16_be(cmap, end_off + i * 2) { Some(v) => v, None => return 0 };
        if cp > end { continue; }
        let start = match read_u16_be(cmap, start_off + i * 2) { Some(v) => v, None => return 0 };
        if cp < start { return 0; }

        let delta        = match read_u16_be(cmap, delta_off + i * 2) { Some(v) => v as u32, None => return 0 };
        let range_offset = match read_u16_be(cmap, range_off + i * 2) { Some(v) => v as usize, None => return 0 };

        return if range_offset == 0 {
            ((cp as u32 + delta) & 0xFFFF) as u16
        } else {
            let gid_off = range_off + i * 2 + range_offset + (cp - start) as usize * 2;
            if gid_off + 2 > cmap.len() { return 0; }
            let g = read_u16_be(cmap, gid_off).unwrap_or(0);
            if g == 0 { 0 } else { ((g as u32 + delta) & 0xFFFF) as u16 }
        };
    }
    0
}
