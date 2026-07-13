use std::collections::HashMap;
use super::super::decoder::{read_u16_be, read_u32_be, write_u16_be};
use super::ivs::{parse_item_variation_store, parse_delta_set_index_map, compute_ivs_delta};

pub fn apply_hvar(
    table_map:  &HashMap<String, Vec<u8>>,
    hmtx_data:  &mut Vec<u8>,
    num_glyphs: usize,
    location:   &[f64],
) -> Result<(), String> {
    apply_metric_var(table_map, "HVAR", "hhea", hmtx_data, num_glyphs, location)
}

pub fn apply_vvar(
    table_map:  &HashMap<String, Vec<u8>>,
    vmtx_data:  &mut Vec<u8>,
    num_glyphs: usize,
    location:   &[f64],
) -> Result<(), String> {
    apply_metric_var(table_map, "VVAR", "vhea", vmtx_data, num_glyphs, location)
}

// HVAR and VVAR share the same layout: IVS offset at 4, advance mapping offset
// at 8; hhea/vhea both keep their long-metrics count at offset 34
fn apply_metric_var(
    table_map:  &HashMap<String, Vec<u8>>,
    var_tag:    &str,
    hea_tag:    &str,
    mtx_data:   &mut Vec<u8>,
    num_glyphs: usize,
    location:   &[f64],
) -> Result<(), String> {
    let var     = table_map.get(var_tag).ok_or_else(|| format!("missing {}", var_tag))?;
    let ivs_off = read_u32_be(var, 4).ok_or_else(|| format!("{}: header truncated", var_tag))? as usize;
    let map_off = read_u32_be(var, 8).ok_or_else(|| format!("{}: header truncated", var_tag))? as usize;

    let store = parse_item_variation_store(var, ivs_off)?;
    let map_fn: Box<dyn Fn(usize) -> (usize, usize)> = if map_off != 0 {
        let m = parse_delta_set_index_map(var, map_off)?;
        Box::new(move |gid| m(gid))
    } else {
        Box::new(|gid| (0, gid))
    };

    let hea          = table_map.get(hea_tag).ok_or_else(|| format!("missing {}", hea_tag))?;
    let long_metrics = read_u16_be(hea, 34).ok_or_else(|| format!("{}: truncated", hea_tag))? as usize;
    if long_metrics == 0 {
        return Err(format!("{}: long-metrics count is zero", hea_tag));
    }

    // only gids with their own metrics slot get a delta — glyphs past the count
    // share the last slot, and adding each of their deltas to it compounded (the
    // slot was re-read after every write, drifting the shared advance)
    for gid in 0..long_metrics.min(num_glyphs) {
        let (outer, inner) = map_fn(gid);
        let delta  = compute_ivs_delta(&store, outer, inner, location);
        let aw_off = gid * 4;
        if aw_off + 2 > mtx_data.len() { continue; }
        let aw = read_u16_be(mtx_data, aw_off).unwrap_or(0) as i32;
        write_u16_be(mtx_data, aw_off, (aw + delta).clamp(0, 65535) as u16);
    }
    Ok(())
}
