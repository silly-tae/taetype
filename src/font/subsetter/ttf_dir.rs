use std::collections::HashMap;
use super::super::decoder::{read_u16_be, read_u32_be};

pub struct TtfEntry { pub offset: usize, pub length: usize }

pub fn parse_ttf_dir(ttf: &[u8]) -> HashMap<String, TtfEntry> {
    let mut map = HashMap::new();
    if ttf.len() < 12 { return map; }
    let n = match read_u16_be(ttf, 4) { Some(v) => v as usize, None => return map };
    for i in 0..n {
        let d = 12 + i * 16;
        if d + 16 > ttf.len() { break; }
        let tag = std::str::from_utf8(&ttf[d..d + 4])
            .unwrap_or("    ")
            .trim_end_matches('\0')
            .to_string();
        let (Some(offset), Some(length)) = (read_u32_be(ttf, d + 8), read_u32_be(ttf, d + 12)) else { break };
        map.insert(tag, TtfEntry { offset: offset as usize, length: length as usize });
    }
    map
}

pub fn slice_table<'a>(ttf: &'a [u8], map: &HashMap<String, TtfEntry>, tag: &str) -> Option<&'a [u8]> {
    map.get(tag).and_then(|e| {
        let end = e.offset.checked_add(e.length)?;
        ttf.get(e.offset..end)
    })
}

pub fn owned_table(ttf: &[u8], map: &HashMap<String, TtfEntry>, tag: &str) -> Option<Vec<u8>> {
    slice_table(ttf, map, tag).map(|s| s.to_vec())
}

pub fn ttf_advance_width(ttf: &[u8], glyph_id: u16) -> u32 {
    metric_advance(ttf, glyph_id, "hmtx", "hhea")
}

pub fn ttf_advance_height(ttf: &[u8], glyph_id: u16) -> u32 {
    metric_advance(ttf, glyph_id, "vmtx", "vhea")
}

// hmtx/hhea and vmtx/vhea share the exact layout: long-metrics count at header
// offset 34, 4-byte (advance, bearing) records with a shared trailing advance
fn metric_advance(ttf: &[u8], glyph_id: u16, mtx_tag: &str, hea_tag: &str) -> u32 {
    let dir  = parse_ttf_dir(ttf);
    let mtx  = match slice_table(ttf, &dir, mtx_tag) { Some(t) => t, None => return 0 };
    let hea  = match slice_table(ttf, &dir, hea_tag) { Some(t) => t, None => return 0 };
    let head = match slice_table(ttf, &dir, "head") { Some(t) => t, None => return 0 };
    if head.len() < 20 || hea.len() < 36 { return 0; }
    let upm         = match read_u16_be(head, 18) { Some(v) if v > 0 => v, _ => return 0 };
    let num_metrics = match read_u16_be(hea, 34) { Some(v) => v as usize, None => return 0 };
    let gid         = glyph_id as usize;
    let aw_off      = if gid < num_metrics { gid * 4 } else { (num_metrics.max(1) - 1) * 4 };
    if aw_off + 2 > mtx.len() { return 0; }
    let aw = match read_u16_be(mtx, aw_off) { Some(v) => v as u64, None => return 0 };
    ((aw * 1000 + upm as u64 / 2) / upm as u64) as u32
}
