use std::collections::HashMap;
use super::super::decoder::{read_u16_be, read_u32_be, read_i16_be};

pub fn find_axis(fvar: &[u8], tag: &str) -> Option<(usize, f64, f64, f64)> {
    let axes_array_offset = read_u16_be(fvar, 4)? as usize;
    let axis_count        = read_u16_be(fvar, 8)? as usize;
    let axis_size         = read_u16_be(fvar, 10)? as usize;
    for i in 0..axis_count {
        let ao = axes_array_offset + i * axis_size;
        let tag_bytes = fvar.get(ao..ao + 4)?;
        if std::str::from_utf8(tag_bytes).unwrap_or("") == tag {
            let min = read_u32_be(fvar, ao + 4)?  as i32 as f64 / 65536.0;
            let def = read_u32_be(fvar, ao + 8)?  as i32 as f64 / 65536.0;
            let max = read_u32_be(fvar, ao + 12)? as i32 as f64 / 65536.0;
            return Some((i, min, def, max));
        }
    }
    None
}

pub fn parse_fvar(table_map: &HashMap<String, Vec<u8>>) -> Result<(usize, f64, f64, f64, usize), String> {
    let fvar = table_map.get("fvar").ok_or("missing fvar")?;
    let axis_count = read_u16_be(fvar, 8).ok_or("fvar: header truncated")? as usize;
    let (wght_idx, wght_min, wght_def, wght_max) =
        find_axis(fvar, "wght").ok_or("No wght axis in fvar")?;
    Ok((wght_idx, wght_def, wght_min, wght_max, axis_count))
}

// per spec the user value clamps to [min,max] BEFORE normalizing — without it a
// target outside the axis range extrapolates coords past ±1 and distorts glyphs;
// a degenerate axis (def == min or def == max) must not divide by zero
pub fn normalize_axis(value: f64, min: f64, def: f64, max: f64) -> f64 {
    let value = value.clamp(min.min(max), max.max(min));
    if value < def && def > min      { ((value - def) / (def - min)).max(-1.0) }
    else if value > def && max > def { ((value - def) / (max - def)).min(1.0) }
    else                             { 0.0 }
}

// avar remaps the whole normalized location in place: format 1 segment maps per
// axis, then (format 2 only) an ItemVariationStore adjustment where every axis's
// delta is evaluated against the FULL post-segment-map location — which is why
// this operates on the vector, not per axis. Any read failure declines to remap
// (never fails), matching the old per-axis contract.
pub fn apply_avar_all(table_map: &HashMap<String, Vec<u8>>, location: &mut [f64]) {
    let avar = match table_map.get("avar") { Some(v) => v, None => return };
    let major = match read_u16_be(avar, 0) { Some(v @ (1 | 2)) => v, _ => return };
    let axis_count = match read_u16_be(avar, 6) { Some(v) => v as usize, None => return };
    let mut pos = 8usize;

    for i in 0..axis_count {
        let count = match read_u16_be(avar, pos) { Some(v) => v as usize, None => return };
        pos += 2;
        if i < location.len() {
            location[i] = segment_map(avar, pos, count, location[i]);
        }
        pos += count * 4;
    }

    if major != 2 { return; }
    let idx_map_off  = match read_u32_be(avar, pos)     { Some(v) => v as usize, None => return };
    let var_store_off = match read_u32_be(avar, pos + 4) { Some(v) => v as usize, None => return };
    if var_store_off == 0 { return; }
    let store = match super::ivs::parse_item_variation_store(avar, var_store_off) { Ok(s) => s, Err(_) => return };
    let map_fn: Box<dyn Fn(usize) -> (usize, usize)> = if idx_map_off != 0 {
        match super::ivs::parse_delta_set_index_map(avar, idx_map_off) {
            Ok(m)  => Box::new(move |i| m(i)),
            Err(_) => return,
        }
    } else {
        Box::new(|i| (0, i))
    };
    // all deltas evaluate against the pre-adjustment coords, then apply together
    let snapshot: Vec<f64> = location.to_vec();
    for (i, loc) in location.iter_mut().enumerate() {
        let (outer, inner) = map_fn(i);
        let delta = super::ivs::compute_ivs_delta_f64(&store, outer, inner, &snapshot);
        *loc = (*loc + delta / 16384.0).clamp(-1.0, 1.0);
    }
}

fn segment_map(avar: &[u8], pos: usize, count: usize, norm_value: f64) -> f64 {
    for j in 0..count.saturating_sub(1) {
        let from0 = match read_i16_be(avar, pos + j * 4)           { Some(v) => v as f64 / 16384.0, None => return norm_value };
        let to0   = match read_i16_be(avar, pos + j * 4 + 2)       { Some(v) => v as f64 / 16384.0, None => return norm_value };
        let from1 = match read_i16_be(avar, pos + (j + 1) * 4)     { Some(v) => v as f64 / 16384.0, None => return norm_value };
        let to1   = match read_i16_be(avar, pos + (j + 1) * 4 + 2) { Some(v) => v as f64 / 16384.0, None => return norm_value };
        if norm_value >= from0 && norm_value <= from1 {
            return if from1 == from0 {
                to0
            } else {
                to0 + (norm_value - from0) * (to1 - to0) / (from1 - from0)
            };
        }
    }
    norm_value
}
