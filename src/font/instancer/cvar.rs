use std::collections::HashMap;
use super::super::decoder::{read_u16_be, read_i16_be, write_i16_be};
use super::gvar::{parse_packed_points, parse_packed_deltas, compute_tuple_scalar};

// cvar varies the cvt (control value) table with the axes — the same tuple
// variation wire format as a gvar per-glyph entry, but "point numbers" index
// cvt slots and there is a single delta stream (cvt values are scalars).
// Without it, TrueType hinting runs against default-weight control values at
// every other weight.
pub fn apply_cvar(
    table_map:  &HashMap<String, Vec<u8>>,
    cvt:        &mut Vec<u8>,
    location:   &[f64],
    axis_count: usize,
) -> Result<(), String> {
    let cvar = match table_map.get("cvar") { Some(c) => c, None => return Ok(()) };
    let num_cvts = cvt.len() / 2;
    if num_cvts == 0 { return Ok(()); }

    let raw_tuple_count = read_u16_be(cvar, 4).ok_or("cvar: header truncated")?;
    let has_shared_pts  = (raw_tuple_count & 0x8000) != 0;
    let tup_count       = (raw_tuple_count & 0x0FFF) as usize;
    let serialized_start = read_u16_be(cvar, 6).ok_or("cvar: header truncated")? as usize;

    let mut serialized_pos = serialized_start;
    let mut shared_points: Option<Vec<usize>> = None;
    if has_shared_pts {
        let (pts, next) = parse_packed_points(cvar, serialized_pos, num_cvts);
        shared_points  = pts;
        serialized_pos = next;
    }

    let mut deltas = vec![0.0f64; num_cvts];
    let mut header_pos       = 8usize;
    let mut private_data_pos = serialized_pos;

    for _ in 0..tup_count {
        let var_data_size    = read_u16_be(cvar, header_pos).ok_or("cvar: tuple header truncated")? as usize;
        let tuple_index_word = read_u16_be(cvar, header_pos + 2).ok_or("cvar: tuple header truncated")?;
        header_pos += 4;

        // cvar tuples always embed their peak (no shared-tuple array exists)
        let has_peak         = (tuple_index_word & 0x8000) != 0;
        let has_intermediate = (tuple_index_word & 0x4000) != 0;
        let has_private_pts  = (tuple_index_word & 0x2000) != 0;
        if !has_peak { return Err("cvar: tuple without embedded peak".into()); }

        let mut peak = Vec::with_capacity(axis_count);
        for _ in 0..axis_count {
            let v = read_i16_be(cvar, header_pos).ok_or("cvar: peak tuple truncated")?;
            peak.push(v as f64 / 16384.0);
            header_pos += 2;
        }
        let (start_tuple, end_tuple) = if has_intermediate {
            let mut s = Vec::with_capacity(axis_count);
            let mut e = Vec::with_capacity(axis_count);
            for _ in 0..axis_count {
                let v = read_i16_be(cvar, header_pos).ok_or("cvar: intermediate truncated")?;
                s.push(v as f64 / 16384.0); header_pos += 2;
            }
            for _ in 0..axis_count {
                let v = read_i16_be(cvar, header_pos).ok_or("cvar: intermediate truncated")?;
                e.push(v as f64 / 16384.0); header_pos += 2;
            }
            (Some(s), Some(e))
        } else {
            (None, None)
        };

        let scalar    = compute_tuple_scalar(location, &peak, start_tuple.as_deref(), end_tuple.as_deref(), axis_count);
        let tuple_end = private_data_pos + var_data_size;

        if scalar.abs() > 1e-10 {
            let points = if has_private_pts {
                let (pts, next) = parse_packed_points(cvar, private_data_pos, num_cvts);
                private_data_pos = next;
                pts
            } else {
                shared_points.clone()
            };
            let n_delta = points.as_ref().map_or(num_cvts, |p| p.len());
            let (dv, _) = parse_packed_deltas(cvar, private_data_pos, n_delta);
            match points {
                None => for i in 0..num_cvts { deltas[i] += scalar * dv[i]; },
                Some(pts) => for (i, &p) in pts.iter().enumerate() {
                    if p < num_cvts { deltas[p] += scalar * dv[i]; }
                },
            }
        }

        private_data_pos = tuple_end;
    }

    for i in 0..num_cvts {
        let d = deltas[i].round() as i32;
        if d == 0 { continue; }
        if let Some(v) = read_i16_be(cvt, i * 2) {
            write_i16_be(cvt, i * 2, (v as i32 + d).clamp(-32768, 32767) as i16);
        }
    }
    Ok(())
}
