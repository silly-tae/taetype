use std::collections::HashMap;
use super::super::decoder::{read_u16_be, read_u32_be, read_i16_be};
use super::coords::{GlyphCoords, extract_coords, iup, apply_simple_glyph_deltas, count_composite_components, apply_composite_glyph_deltas};

pub struct GvarResult {
    pub glyf_data: Vec<u8>,
    pub new_loca:  Vec<usize>,
    // x delta of each glyph's advance-width phantom point (index num_points+1) —
    // the metrics fallback for variable fonts that ship no HVAR
    pub advance_deltas: Vec<f64>,
}

pub fn apply_gvar(
    table_map:     &HashMap<String, Vec<u8>>,
    glyf_data:     &[u8],
    glyph_offsets: &[usize],
    num_glyphs:    usize,
    location:      &[f64],
    axis_count:    usize,
) -> Result<GvarResult, String> {
    let gvar = table_map.get("gvar").ok_or("missing gvar")?;

    let gvar_axis_count    = read_u16_be(gvar, 4).ok_or("gvar: header truncated")? as usize;
    let shared_tuple_count = read_u16_be(gvar, 6).ok_or("gvar: header truncated")? as usize;
    let shared_tuples_off  = read_u32_be(gvar, 8).ok_or("gvar: header truncated")? as usize;
    let flags              = read_u16_be(gvar, 14).ok_or("gvar: header truncated")?;
    let var_array_off      = read_u32_be(gvar, 16).ok_or("gvar: header truncated")? as usize;
    let use_words          = (flags & 1) != 0;
    let offsets_base       = 20usize;

    let mut shared_tuples: Vec<Vec<f64>> = Vec::with_capacity(shared_tuple_count);
    for i in 0..shared_tuple_count {
        let mut coords = Vec::with_capacity(gvar_axis_count);
        for j in 0..gvar_axis_count {
            let v = read_i16_be(gvar, shared_tuples_off + (i * gvar_axis_count + j) * 2)
                .ok_or("gvar: shared tuple truncated")?;
            coords.push(v as f64 / 16384.0);
        }
        shared_tuples.push(coords);
    }

    let glyph_var_offset = |gid: usize| -> Option<usize> {
        if use_words {
            read_u32_be(gvar, offsets_base + gid * 4).map(|v| v as usize)
        } else {
            read_u16_be(gvar, offsets_base + gid * 2).map(|v| v as usize * 2)
        }
    };

    let mut new_glyphs: Vec<Vec<u8>> = Vec::with_capacity(num_glyphs);
    let mut new_loca: Vec<usize>     = vec![0; num_glyphs + 1];
    let mut advance_deltas: Vec<f64> = vec![0.0; num_glyphs];

    for gid in 0..num_glyphs {
        let glyph_start  = glyph_offsets[gid];
        let glyph_end    = glyph_offsets[gid + 1];
        let var_off      = glyph_var_offset(gid).ok_or("gvar: glyph variation offset truncated")?;
        let var_off_next = glyph_var_offset(gid + 1).ok_or("gvar: glyph variation offset truncated")?;

        if var_off == var_off_next {
            let raw = glyf_data.get(glyph_start..glyph_end)
                .ok_or("gvar: glyph data range out of bounds")?.to_vec();
            let padded_len = (raw.len() + 3) & !3;
            new_loca[gid + 1] = new_loca[gid] + padded_len;
            new_glyphs.push(raw);
            continue;
        }

        // empty glyphs (space) still carry phantom-only tuples — their advance
        // varies even though there is no outline to modify; zero-length and
        // header-only (nc == 0) forms both take the phantom-only path
        let (n_contours, num_points) = if glyph_start == glyph_end {
            (0i16, 0usize)
        } else {
            let nc = read_i16_be(glyf_data, glyph_start).ok_or("gvar: glyph header truncated")?;
            if nc > 0 {
                let np = read_u16_be(glyf_data, glyph_start + 10 + (nc as usize - 1) * 2)
                    .ok_or("gvar: contour endpoint truncated")? as usize + 1;
                (nc, np)
            } else if nc == -1 {
                (nc, count_composite_components(glyf_data, glyph_start))
            } else if nc == 0 {
                (0, 0usize)
            } else {
                let raw = glyf_data.get(glyph_start..glyph_end)
                    .ok_or("gvar: glyph data range out of bounds")?.to_vec();
                let padded_len = (raw.len() + 3) & !3;
                new_loca[gid + 1] = new_loca[gid] + padded_len;
                new_glyphs.push(raw);
                continue;
            }
        };

        let total_points = num_points + 4; // phantom points
        let gvd_base = var_array_off + var_off;
        let raw_tuple_count  = read_u16_be(gvar, gvd_base).ok_or("gvar: tuple header truncated")?;
        let has_shared_pts   = (raw_tuple_count & 0x8000) != 0;
        let tup_count        = (raw_tuple_count & 0x0FFF) as usize;
        let serialized_start = gvd_base
            + read_u16_be(gvar, gvd_base + 2).ok_or("gvar: tuple header truncated")? as usize;

        let mut serialized_pos = serialized_start;
        let mut shared_points: Option<Vec<usize>> = None;
        if has_shared_pts {
            let (pts, next) = parse_packed_points(gvar, serialized_pos, total_points);
            shared_points  = pts;
            serialized_pos = next;
        }

        let mut dx = vec![0.0f64; total_points];
        let mut dy = vec![0.0f64; total_points];

        let mut header_pos       = gvd_base + 4;
        let mut private_data_pos = serialized_pos;

        let mut cached_coords: Option<GlyphCoords> = None;

        for _ in 0..tup_count {
            let var_data_size    = read_u16_be(gvar, header_pos)
                .ok_or("gvar: tuple var data size truncated")? as usize;
            let tuple_index_word = read_u16_be(gvar, header_pos + 2)
                .ok_or("gvar: tuple index truncated")?;
            header_pos += 4;

            let has_peak         = (tuple_index_word & 0x8000) != 0;
            let has_intermediate = (tuple_index_word & 0x4000) != 0;
            let has_private_pts  = (tuple_index_word & 0x2000) != 0;
            let shared_idx       = (tuple_index_word & 0x0FFF) as usize;

            let peak_buf: Vec<f64>;
            let peak: &[f64] = if has_peak {
                let mut p = Vec::with_capacity(gvar_axis_count);
                for _ in 0..gvar_axis_count {
                    let v = read_i16_be(gvar, header_pos).ok_or("gvar: peak tuple truncated")?;
                    p.push(v as f64 / 16384.0);
                    header_pos += 2;
                }
                peak_buf = p;
                &peak_buf
            } else {
                shared_tuples.get(shared_idx).ok_or("gvar: shared tuple index out of range")?
            };

            let (start_tuple, end_tuple) = if has_intermediate {
                let mut s = Vec::with_capacity(gvar_axis_count);
                let mut e = Vec::with_capacity(gvar_axis_count);
                for _ in 0..gvar_axis_count {
                    let v = read_i16_be(gvar, header_pos).ok_or("gvar: intermediate start truncated")?;
                    s.push(v as f64 / 16384.0); header_pos += 2;
                }
                for _ in 0..gvar_axis_count {
                    let v = read_i16_be(gvar, header_pos).ok_or("gvar: intermediate end truncated")?;
                    e.push(v as f64 / 16384.0); header_pos += 2;
                }
                (Some(s), Some(e))
            } else {
                (None, None)
            };

            let scalar    = compute_tuple_scalar(location, peak, start_tuple.as_deref(), end_tuple.as_deref(), axis_count);
            let tuple_end = private_data_pos + var_data_size;

            if scalar.abs() > 1e-10 {
                let points = if has_private_pts {
                    let (pts, next) = parse_packed_points(gvar, private_data_pos, total_points);
                    private_data_pos = next;
                    pts
                } else {
                    shared_points.clone()
                };

                let n_delta = points.as_ref().map_or(total_points, |p| p.len());
                let (xr, next) = parse_packed_deltas(gvar, private_data_pos, n_delta);
                private_data_pos = next;
                let (yr, _) = parse_packed_deltas(gvar, private_data_pos, n_delta);

                match points {
                    None => {
                        for i in 0..total_points {
                            dx[i] += scalar * xr[i];
                            dy[i] += scalar * yr[i];
                        }
                    }
                    Some(ref pts) => {
                        let mut fdx = vec![0.0f64; total_points];
                        let mut fdy = vec![0.0f64; total_points];
                        for (i, &p) in pts.iter().enumerate() {
                            if p < total_points {
                                fdx[p] = xr[i];
                                fdy[p] = yr[i];
                            }
                        }

                        if n_contours > 0 {
                            let cc = cached_coords.get_or_insert_with(|| extract_coords(glyf_data, glyph_start, n_contours as usize));
                            iup(&mut fdx, &mut fdy, pts, &cc.end_pts, num_points, &cc.x_coords, &cc.y_coords);
                        }

                        for i in 0..total_points {
                            dx[i] += scalar * fdx[i];
                            dy[i] += scalar * fdy[i];
                        }
                    }
                }
            }

            private_data_pos = tuple_end;
        }

        advance_deltas[gid] = dx[num_points + 1];

        let modified = if n_contours > 0 {
            apply_simple_glyph_deltas(glyf_data, glyph_start, glyph_end, n_contours as usize, &dx, &dy, cached_coords.as_ref())
        } else if n_contours == -1 {
            apply_composite_glyph_deltas(glyf_data, glyph_start, glyph_end, &dx, &dy, num_points)
        } else {
            // phantom-only forms: nothing to modify, keep the bytes as-is
            // (empty for zero-length glyphs, the 10-byte header otherwise)
            glyf_data.get(glyph_start..glyph_end).map_or(Vec::new(), |s| s.to_vec())
        };

        let padded_len = (modified.len() + 3) & !3;
        new_loca[gid + 1] = new_loca[gid] + padded_len;
        new_glyphs.push(modified);
    }

    let total_size = new_loca[num_glyphs];
    let mut new_glyf = vec![0u8; total_size];
    let mut write_pos = 0;
    for gid in 0..num_glyphs {
        new_glyf[write_pos..write_pos + new_glyphs[gid].len()].copy_from_slice(&new_glyphs[gid]);
        write_pos += (new_glyphs[gid].len() + 3) & !3;
    }

    Ok(GvarResult { glyf_data: new_glyf, new_loca, advance_deltas })
}

pub(crate) fn compute_tuple_scalar(
    location:    &[f64],
    peak:        &[f64],
    start_tuple: Option<&[f64]>,
    end_tuple:   Option<&[f64]>,
    axis_count:  usize,
) -> f64 {
    let mut scalar = 1.0f64;
    // peak/start_tuple/end_tuple are sized to the gvar table's own declared axis count,
    // which for a malformed font may not match axis_count (fvar's); clamp to the shorter.
    for i in 0..axis_count.min(peak.len()) {
        let p   = peak[i];
        let loc = location[i];
        if p == 0.0 { continue; }
        if loc == p { continue; }
        let start = start_tuple.map_or(if p < 0.0 { -1.0 } else { 0.0 }, |s| s[i]);
        let end   = end_tuple.map_or(  if p > 0.0 {  1.0 } else { 0.0 }, |e| e[i]);
        if loc < start || loc > end { return 0.0; }
        scalar *= if loc < p {
            (loc - start) / (p - start)
        } else {
            (end - loc) / (end - p)
        };
    }
    scalar
}

pub(crate) fn parse_packed_points(buf: &[u8], pos: usize, total_points: usize) -> (Option<Vec<usize>>, usize) {
    let mut pos = pos;
    let mut count = match buf.get(pos) {
        Some(&b) => { pos += 1; b as usize }
        None => return (None, pos),
    };
    if count == 0 { return (None, pos); }
    if count & 0x80 != 0 {
        let next = match buf.get(pos) {
            Some(&b) => { pos += 1; b as usize }
            None => return (None, pos),
        };
        count = ((count & 0x7F) << 8) | next;
    }
    if count > total_points { count = total_points; }

    let mut points: Vec<usize> = Vec::with_capacity(count);
    let mut idx = 0usize;
    while points.len() < count {
        let ctrl = match buf.get(pos) {
            Some(&b) => { pos += 1; b }
            None => break,
        };
        let words = (ctrl & 0x80) != 0;
        let len   = (ctrl & 0x7F) as usize + 1;
        for _ in 0..len {
            if points.len() >= count { break; }
            if words {
                let hi = buf.get(pos).copied().unwrap_or(0) as usize;
                let lo = buf.get(pos + 1).copied().unwrap_or(0) as usize;
                pos += 2;
                idx += (hi << 8) | lo;
            } else {
                idx += buf.get(pos).copied().unwrap_or(0) as usize;
                pos += 1;
            }
            if idx < total_points { points.push(idx); }
        }
    }
    (Some(points), pos)
}

pub(crate) fn parse_packed_deltas(buf: &[u8], pos: usize, count: usize) -> (Vec<f64>, usize) {
    let mut deltas = vec![0.0f64; count];
    let mut i = 0;
    let mut pos = pos;
    'outer: while i < count {
        let ctrl = match buf.get(pos) { Some(&b) => { pos += 1; b } None => break };
        let len  = (ctrl & 0x3F) as usize + 1;
        if ctrl & 0x80 != 0 {
            i += len;
        } else if ctrl & 0x40 != 0 {
            for _ in 0..len {
                if i >= count { break; }
                let hi = match buf.get(pos) { Some(&b) => { pos += 1; b as u16 } None => break 'outer };
                let lo = match buf.get(pos) { Some(&b) => { pos += 1; b as u16 } None => break 'outer };
                let raw = (hi << 8) | lo;
                deltas[i] = if raw >= 0x8000 { raw as f64 - 0x10000 as f64 } else { raw as f64 };
                i += 1;
            }
        } else {
            for _ in 0..len {
                if i >= count { break; }
                let raw = match buf.get(pos) { Some(&b) => { pos += 1; b } None => break 'outer };
                deltas[i] = if raw >= 0x80 { raw as f64 - 0x100 as f64 } else { raw as f64 };
                i += 1;
            }
        }
    }
    (deltas, pos)
}
