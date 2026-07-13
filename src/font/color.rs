use std::collections::HashMap;
use super::decoder::{read_u16_be, read_u32_be, read_i16_be};

// Color-font extraction: PNG strikes from sbix (Apple) and CBDT/CBLC (Google),
// and COLR v0 layer lists with CPAL palette 0. The renderer decides how to
// paint (emoji wiring is the TS phase); the engine only surfaces the data.

pub struct GlyphBitmap {
    pub png:      Vec<u8>,
    pub ppem:     u16,
    pub origin_x: i16,
    pub origin_y: i16,
}

pub fn glyph_bitmap(table_map: &HashMap<String, Vec<u8>>, gid: u16, target_ppem: u16) -> Option<GlyphBitmap> {
    if let Some(b) = sbix_bitmap(table_map, gid, target_ppem) { return Some(b); }
    cbdt_bitmap(table_map, gid, target_ppem)
}

fn num_glyphs(table_map: &HashMap<String, Vec<u8>>) -> usize {
    table_map.get("maxp").and_then(|m| read_u16_be(m, 4)).unwrap_or(0) as usize
}

// smallest strike at or above the target renders sharpest; fall back to the
// largest available when the target exceeds every strike
fn pick_strike<T: Copy>(strikes: &[(u16, T)], target_ppem: u16) -> Option<T> {
    let mut best_above: Option<(u16, T)> = None;
    let mut largest:    Option<(u16, T)> = None;
    for &(ppem, v) in strikes {
        if ppem >= target_ppem && best_above.map_or(true, |(p, _)| ppem < p) {
            best_above = Some((ppem, v));
        }
        if largest.map_or(true, |(p, _)| ppem > p) {
            largest = Some((ppem, v));
        }
    }
    best_above.or(largest).map(|(_, v)| v)
}

fn sbix_bitmap(table_map: &HashMap<String, Vec<u8>>, gid: u16, target_ppem: u16) -> Option<GlyphBitmap> {
    let sbix = table_map.get("sbix")?;
    let n_glyphs = num_glyphs(table_map);
    if gid as usize >= n_glyphs { return None; }

    let num_strikes = read_u32_be(sbix, 4)? as usize;
    let mut strikes: Vec<(u16, usize)> = Vec::with_capacity(num_strikes.min(64));
    for i in 0..num_strikes.min(64) {
        let off  = read_u32_be(sbix, 8 + i * 4)? as usize;
        let ppem = read_u16_be(sbix, off)?;
        strikes.push((ppem, off));
    }
    let strike = pick_strike(&strikes, target_ppem)?;
    let ppem   = read_u16_be(sbix, strike)?;

    let g_off  = read_u32_be(sbix, strike + 4 + gid as usize * 4)? as usize;
    let g_next = read_u32_be(sbix, strike + 4 + (gid as usize + 1) * 4)? as usize;
    if g_next <= g_off { return None; } // empty = no bitmap for this glyph
    let data = sbix.get(strike + g_off..strike + g_next)?;
    if data.len() < 8 { return None; }
    let origin_x = read_i16_be(data, 0)?;
    let origin_y = read_i16_be(data, 2)?;
    let gtype    = &data[4..8];
    if gtype != b"png " { return None; } // jpg/tiff/dupe not handled
    Some(GlyphBitmap { png: data[8..].to_vec(), ppem, origin_x, origin_y })
}

fn cbdt_bitmap(table_map: &HashMap<String, Vec<u8>>, gid: u16, target_ppem: u16) -> Option<GlyphBitmap> {
    let cblc = table_map.get("CBLC")?;
    let cbdt = table_map.get("CBDT")?;

    let num_sizes = read_u32_be(cblc, 4)? as usize;
    // bitmapSizeTable: 48 bytes each, starting at 8
    let mut strikes: Vec<(u16, usize)> = Vec::with_capacity(num_sizes.min(64));
    for i in 0..num_sizes.min(64) {
        let st = 8 + i * 48;
        let ppem_x = *cblc.get(st + 44)? as u16; // ppemX (44) per BitmapSize layout
        strikes.push((ppem_x, st));
    }
    let st = pick_strike(&strikes, target_ppem)?;
    let ppem = *cblc.get(st + 44)? as u16;

    let ist_array_off = read_u32_be(cblc, st)? as usize;     // indexSubTableArrayOffset
    let n_ist         = read_u32_be(cblc, st + 8)? as usize; // numberOfIndexSubTables

    for i in 0..n_ist.min(1024) {
        let rec = ist_array_off + i * 8;
        let first = read_u16_be(cblc, rec)?;
        let last  = read_u16_be(cblc, rec + 2)?;
        if gid < first || gid > last { continue; }
        let ist = ist_array_off + read_u32_be(cblc, rec + 4)? as usize;

        let index_format = read_u16_be(cblc, ist)?;
        let image_format = read_u16_be(cblc, ist + 2)?;
        let image_data_off = read_u32_be(cblc, ist + 4)? as usize;
        // only PNG-bearing CBDT formats are useful here
        if !matches!(image_format, 17 | 18 | 19) { return None; }

        let idx = (gid - first) as usize;
        let (g_off, g_next) = match index_format {
            1 => (
                read_u32_be(cblc, ist + 8 + idx * 4)? as usize,
                read_u32_be(cblc, ist + 8 + (idx + 1) * 4)? as usize,
            ),
            2 => {
                let size = read_u32_be(cblc, ist + 8)? as usize;
                (size * idx, size * (idx + 1))
            }
            3 => (
                read_u16_be(cblc, ist + 8 + idx * 2)? as usize,
                read_u16_be(cblc, ist + 8 + (idx + 1) * 2)? as usize,
            ),
            _ => return None,
        };
        if g_next <= g_off { return None; }
        let data = cbdt.get(image_data_off + g_off..image_data_off + g_next)?;

        // format 17: smallGlyphMetrics(5) + dataLen(4); 18: bigMetrics(8) + dataLen(4);
        // 19: dataLen(4) only (metrics live in the index subtable)
        let (metrics_len, ox, oy) = match image_format {
            17 => (5usize, *data.get(3)? as i8 as i16, *data.get(4)? as i8 as i16),
            18 => (8usize, *data.get(4)? as i8 as i16, *data.get(5)? as i8 as i16),
            _  => (0usize, 0, 0),
        };
        let png_len = read_u32_be(data, metrics_len)? as usize;
        let png = data.get(metrics_len + 4..metrics_len + 4 + png_len)?;
        return Some(GlyphBitmap { png: png.to_vec(), ppem, origin_x: ox, origin_y: oy });
    }
    None
}

// COLR v0 layers for a base glyph: [(layer_gid, r, g, b, a, is_foreground)],
// colors from CPAL palette 0; paletteIndex 0xFFFF means "use the text color"
pub fn colr_layers(table_map: &HashMap<String, Vec<u8>>, gid: u16) -> Option<Vec<(u16, u8, u8, u8, u8, bool)>> {
    let colr = table_map.get("COLR")?;
    let cpal = table_map.get("CPAL")?;

    let n_base     = read_u16_be(colr, 2)? as usize;
    let base_off   = read_u32_be(colr, 4)? as usize;
    let layers_off = read_u32_be(colr, 8)? as usize;

    // base glyph records are sorted by gid — binary search
    let (mut lo, mut hi) = (0usize, n_base);
    let mut hit: Option<(usize, usize)> = None;
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        let rec = base_off + mid * 6;
        let g = read_u16_be(colr, rec)?;
        if g == gid {
            hit = Some((
                read_u16_be(colr, rec + 2)? as usize,
                read_u16_be(colr, rec + 4)? as usize,
            ));
            break;
        } else if g < gid { lo = mid + 1; } else { hi = mid; }
    }
    let (first_layer, n_layers) = hit?;

    // CPAL palette 0
    let n_entries    = read_u16_be(cpal, 2)? as usize;
    let records_off  = read_u32_be(cpal, 8)? as usize;
    let pal0_start   = read_u16_be(cpal, 12)? as usize;

    let mut out = Vec::with_capacity(n_layers.min(256));
    for l in 0..n_layers.min(256) {
        let rec = layers_off + (first_layer + l) * 4;
        let layer_gid = read_u16_be(colr, rec)?;
        let pal_idx   = read_u16_be(colr, rec + 2)?;
        if pal_idx == 0xFFFF {
            out.push((layer_gid, 0, 0, 0, 255, true));
            continue;
        }
        if (pal_idx as usize) >= n_entries { return None; }
        let c = records_off + (pal0_start + pal_idx as usize) * 4;
        let b = *cpal.get(c)?;
        let g = *cpal.get(c + 1)?;
        let r = *cpal.get(c + 2)?;
        let a = *cpal.get(c + 3)?;
        out.push((layer_gid, r, g, b, a, false));
    }
    Some(out)
}
