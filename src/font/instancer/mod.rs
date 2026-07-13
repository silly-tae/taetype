mod axis;
mod loca;
mod coords;
mod gvar;
mod hvar;
mod ivs;
mod mvar;
mod cvar;
mod var_tests;

use std::collections::HashMap;
use super::decoder::{build_ttf, read_u16_be, read_i16_be, write_u16_be, write_i16_be};
use axis::{find_axis, parse_fvar, normalize_axis, apply_avar_all};
use loca::{parse_loca, build_loca_table};
use gvar::apply_gvar;
use hvar::{apply_hvar, apply_vvar};
use mvar::apply_mvar;
use cvar::apply_cvar;

pub fn instance_font_from_map(
    table_map:     &HashMap<String, Vec<u8>>,
    target_weight: u16,
    target_opsz:   u16,
) -> Result<Vec<u8>, String> {
    let (axis_idx, axis_default, axis_min, axis_max, axis_count) = parse_fvar(table_map)?;

    let mut location = vec![0.0f64; axis_count];
    location[axis_idx] = normalize_axis(target_weight as f64, axis_min, axis_default, axis_max);

    if target_opsz > 0 {
        if let Some(fvar) = table_map.get("fvar") {
            if let Some((opsz_idx, opsz_min, opsz_def, opsz_max)) = find_axis(fvar, "opsz") {
                location[opsz_idx] = normalize_axis(target_opsz as f64, opsz_min, opsz_def, opsz_max);
            }
        }
    }

    // avar (format 1 segment maps + format 2 varStore) remaps the whole vector —
    // format 2 deltas for one axis depend on every axis's coordinate
    apply_avar_all(table_map, &mut location);

    let head        = table_map.get("head").ok_or("missing head")?;
    let loca_format = read_i16_be(head, 50).ok_or("head: truncated")?;

    let maxp       = table_map.get("maxp").ok_or("missing maxp")?;
    let num_glyphs = read_u16_be(maxp, 4).ok_or("maxp: truncated")? as usize;

    let glyph_offsets = parse_loca(table_map, loca_format, num_glyphs)?;

    let mut hmtx_data = table_map.get("hmtx").ok_or("missing hmtx")?.clone();
    let mut os2_data  = table_map.get("OS/2").ok_or("missing OS/2")?.clone();

    // deltas apply when ANY axis moved off its default — gating on weight alone
    // silently ignored an opsz-only instance
    let needs_var = location.iter().any(|&v| v != 0.0);

    let mut out_loca_format = loca_format;
    let mut phantom_advance_deltas: Option<Vec<f64>> = None;
    let (glyf_data, new_loca_opt) = if needs_var && table_map.contains_key("gvar") {
        let glyf_owned = table_map.get("glyf").ok_or("missing glyf")?.clone();
        let result     = apply_gvar(table_map, &glyf_owned, &glyph_offsets, num_glyphs, &location, axis_count)?;
        // instanced glyphs can grow past what short loca addresses (offset/2 in a
        // u16) — bump to long format instead of silently truncating offsets
        if out_loca_format == 0 && result.new_loca.last().copied().unwrap_or(0) > 0xFFFF * 2 {
            out_loca_format = 1;
        }
        let new_loca = build_loca_table(&result.new_loca, out_loca_format);
        phantom_advance_deltas = Some(result.advance_deltas);
        (result.glyf_data, Some(new_loca))
    } else {
        (table_map.get("glyf").ok_or("missing glyf")?.clone(), None)
    };

    if needs_var && table_map.contains_key("HVAR") {
        apply_hvar(table_map, &mut hmtx_data, num_glyphs, &location)?;
    } else if needs_var {
        // no HVAR: gvar phantom points are the spec'd source of advance-width
        // variation — without this, HVAR-less fonts keep default-weight metrics
        if let Some(deltas) = &phantom_advance_deltas {
            if let Some(hhea) = table_map.get("hhea") {
                let long_metrics = read_u16_be(hhea, 34).unwrap_or(0) as usize;
                for gid in 0..long_metrics.min(num_glyphs).min(deltas.len()) {
                    let d = deltas[gid].round() as i32;
                    if d == 0 { continue; }
                    let off = gid * 4;
                    if off + 2 > hmtx_data.len() { break; }
                    let aw = read_u16_be(&hmtx_data, off).unwrap_or(0) as i32;
                    write_u16_be(&mut hmtx_data, off, (aw + d).clamp(0, 65535) as u16);
                }
            }
        }
    }

    let mut vmtx_data = table_map.get("vmtx").cloned();
    if needs_var && table_map.contains_key("VVAR") {
        if let Some(ref mut vmtx) = vmtx_data {
            apply_vvar(table_map, vmtx, num_glyphs, &location)?;
        }
    }

    let mut hhea_data = table_map.get("hhea").cloned().unwrap_or_default();
    let mut post_data = table_map.get("post").cloned().unwrap_or_default();
    let mut cvt_data  = table_map.get("cvt ").cloned();
    if needs_var {
        apply_mvar(table_map, &mut hhea_data, &mut os2_data, &mut post_data, &location)?;
        if let Some(ref mut cvt) = cvt_data {
            apply_cvar(table_map, cvt, &location, axis_count)?;
        }
    }

    // write-site guard: os2_data is cloned from untrusted font bytes, not self-allocated —
    // must be long enough for the usWeightClass field at offset 4 before writing.
    if os2_data.len() >= 6 {
        write_u16_be(&mut os2_data, 4, target_weight);
    }

    const STRIP: &[&str] = &["fvar", "gvar", "HVAR", "MVAR", "STAT", "avar", "cvar", "VVAR"];
    let mut out_map: HashMap<String, Vec<u8>> = HashMap::new();
    for (tag, data) in table_map {
        if !STRIP.contains(&tag.as_str()) {
            out_map.insert(tag.clone(), data.clone());
        }
    }
    out_map.insert("glyf".to_string(), glyf_data);
    out_map.insert("hmtx".to_string(), hmtx_data);
    out_map.insert("OS/2".to_string(), os2_data);
    if let Some(vmtx) = vmtx_data { out_map.insert("vmtx".to_string(), vmtx); }
    if !hhea_data.is_empty() { out_map.insert("hhea".to_string(), hhea_data); }
    if !post_data.is_empty() { out_map.insert("post".to_string(), post_data); }
    if let Some(cvt) = cvt_data { out_map.insert("cvt ".to_string(), cvt); }
    if let Some(new_loca) = new_loca_opt {
        out_map.insert("loca".to_string(), new_loca);
    }
    if out_loca_format != loca_format {
        if let Some(head_out) = out_map.get_mut("head") {
            if head_out.len() >= 52 {
                write_i16_be(head_out, 50, out_loca_format);
            }
        }
    }

    Ok(build_ttf(&out_map))
}
