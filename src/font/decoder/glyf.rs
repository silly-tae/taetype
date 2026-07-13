use super::io::{read_u16_be, read_u32_be, read_i16_be, write_i16_be, write_u16_be, write_u32_be};
use super::woff2::read_255_uint16;

// Sign helper: odd flag → positive, even flag → negative (WOFF2 spec).
fn ws(flag: u8, val: i32) -> i32 {
    if flag & 1 != 0 { val } else { -val }
}

fn read_triplet(gs: &[u8], off: usize, flag: u8) -> Result<(i16, i16, usize), String> {
    let err = || "glyf transform: triplet data truncated".to_string();
    let mut dx: i32 = 0;
    let mut dy: i32 = 0;
    let mut pos = off;

    if flag < 10 {
        let b = *gs.get(pos).ok_or_else(err)? as i32; pos += 1;
        dy = ws(flag, ((flag as i32 & 14) << 7) + b);
    } else if flag < 20 {
        let b = *gs.get(pos).ok_or_else(err)? as i32; pos += 1;
        dx = ws(flag, (((flag as i32 - 10) & 14) << 7) + b);
    } else if flag < 84 {
        let b0 = flag as i32 - 20;
        let b1 = *gs.get(pos).ok_or_else(err)? as i32; pos += 1;
        dx = ws(flag,      1 + (b0 & 0x30) + (b1 >> 4));
        dy = ws(flag >> 1, 1 + ((b0 & 0x0C) << 2) + (b1 & 0x0F));
    } else if flag < 120 {
        let b0 = flag as i32 - 84;
        let byte0 = *gs.get(pos).ok_or_else(err)? as i32; pos += 1;
        dx = ws(flag, 1 + ((b0 / 12) << 8) + byte0);
        let byte1 = *gs.get(pos).ok_or_else(err)? as i32; pos += 1;
        dy = ws(flag >> 1, 1 + (((b0 % 12) >> 2) << 8) + byte1);
    } else if flag < 124 {
        let byte0 = *gs.get(pos).ok_or_else(err)? as i32;
        let b1    = *gs.get(pos + 1).ok_or_else(err)? as i32;
        pos += 2;
        dx = ws(flag, byte0 << 4 | (b1 >> 4));
        let byte2 = *gs.get(pos).ok_or_else(err)? as i32; pos += 1;
        dy = ws(flag >> 1, ((b1 & 0x0F) << 8) | byte2);
    } else {
        let b0 = *gs.get(pos).ok_or_else(err)? as i32;
        let b1 = *gs.get(pos + 1).ok_or_else(err)? as i32;
        pos += 2;
        dx = ws(flag, (b0 << 8) | b1);
        let b2 = *gs.get(pos).ok_or_else(err)? as i32;
        let b3 = *gs.get(pos + 1).ok_or_else(err)? as i32;
        pos += 2;
        dy = ws(flag >> 1, (b2 << 8) | b3);
    }

    Ok((dx as i16, dy as i16, pos))
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

pub fn unglyf_transform(tg: &[u8]) -> Result<(Vec<u8>, Vec<u8>, i16), String> {
    if tg.len() < 36 {
        return Err("glyf transform: header too short".into());
    }
    let err_hdr = || "glyf transform: header truncated".to_string();
    let mut hoff = 0usize;
    hoff += 2;
    hoff += 2;
    let num_glyphs   = read_u16_be(tg, hoff).ok_or_else(err_hdr)? as usize; hoff += 2;
    let index_format = read_i16_be(tg, hoff).ok_or_else(err_hdr)?;          hoff += 2;

    let n_contour_stream_size   = read_u32_be(tg, hoff).ok_or_else(err_hdr)? as usize; hoff += 4;
    let n_points_stream_size    = read_u32_be(tg, hoff).ok_or_else(err_hdr)? as usize; hoff += 4;
    let flag_stream_size        = read_u32_be(tg, hoff).ok_or_else(err_hdr)? as usize; hoff += 4;
    let glyph_stream_size       = read_u32_be(tg, hoff).ok_or_else(err_hdr)? as usize; hoff += 4;
    let composite_stream_size   = read_u32_be(tg, hoff).ok_or_else(err_hdr)? as usize; hoff += 4;
    let bbox_stream_size        = read_u32_be(tg, hoff).ok_or_else(err_hdr)? as usize; hoff += 4;
    let instruction_stream_size = read_u32_be(tg, hoff).ok_or_else(err_hdr)? as usize;

    let hdr_size      = 36;
    let n_contour_off = hdr_size;
    let n_points_off  = n_contour_off + n_contour_stream_size;
    let flag_off      = n_points_off  + n_points_stream_size;
    let glyph_off     = flag_off      + flag_stream_size;
    let composite_off = glyph_off     + glyph_stream_size;
    let bbox_off      = composite_off + composite_stream_size;
    let instr_off     = bbox_off      + bbox_stream_size;

    if instr_off + instruction_stream_size > tg.len() {
        return Err("glyf transform: stream extents exceed data".into());
    }
    if n_contour_stream_size < num_glyphs * 2 {
        return Err("glyf transform: n_contour stream too small".into());
    }
    if bbox_stream_size < (num_glyphs + 7) / 8 {
        return Err("glyf transform: bbox stream too small for bitmap".into());
    }

    // Inter pads the bbox bitmap to 4-byte alignment, so derive size arithmetically
    let mut bbox_count = 0usize;
    for i in 0..num_glyphs {
        let byte = *tg.get(bbox_off + (i >> 3)).ok_or("glyf transform: bbox bitmap truncated")?;
        if (byte >> (7 - (i & 7))) & 1 != 0 {
            bbox_count += 1;
        }
    }
    // bbox_count*8 must fit within the declared bbox stream, or the bitmap claims more
    // explicit bboxes than the stream has room for — malformed/adversarial input.
    if bbox_count.checked_mul(8).map_or(true, |v| v > bbox_stream_size) {
        return Err("glyf transform: bbox stream too small for declared bbox count".into());
    }
    let bbox_bitmap_size = bbox_stream_size - bbox_count * 8;
    let bbox_data_off    = bbox_off + bbox_bitmap_size;

    let mut np_off   = n_points_off;
    let mut fl_off   = flag_off;
    let mut gs_off   = glyph_off;
    let mut comp_off = composite_off;
    let mut bbox_idx = 0usize;
    let mut i_off    = instr_off;

    let mut glyph_buffers: Vec<Vec<u8>> = Vec::with_capacity(num_glyphs);
    let mut glyph_offsets: Vec<usize>   = vec![0];

    for gi in 0..num_glyphs {
        let n_contours_raw = read_i16_be(tg, n_contour_off + gi * 2)
            .ok_or("glyf transform: n_contour stream truncated")?;
        let bbox_byte = *tg.get(bbox_off + (gi >> 3)).ok_or("glyf transform: bbox bitmap truncated")?;
        let bbox_bit = (bbox_byte >> (7 - (gi & 7))) & 1;

        let (mut x_min, mut y_min, mut x_max, mut y_max): (i16, i16, i16, i16) = (0, 0, 0, 0);
        if bbox_bit != 0 {
            let err = || "glyf transform: bbox data truncated".to_string();
            x_min = read_i16_be(tg, bbox_data_off + bbox_idx * 8).ok_or_else(err)?;
            y_min = read_i16_be(tg, bbox_data_off + bbox_idx * 8 + 2).ok_or_else(err)?;
            x_max = read_i16_be(tg, bbox_data_off + bbox_idx * 8 + 4).ok_or_else(err)?;
            y_max = read_i16_be(tg, bbox_data_off + bbox_idx * 8 + 6).ok_or_else(err)?;
            bbox_idx += 1;
        }

        if n_contours_raw == 0 {
            glyph_buffers.push(Vec::new());
            glyph_offsets.push(*glyph_offsets.last().unwrap());
            continue;
        }

        if n_contours_raw < 0 {
            // the spec requires composites to carry an explicit bbox (their point
            // data isn't in this table, so nothing exists to derive one from)
            if bbox_bit == 0 {
                return Err("glyf transform: composite glyph without explicit bbox".into());
            }
            const MORE_COMPONENTS: u16      = 0x0020;
            const WE_HAVE_INSTRUCTIONS: u16 = 0x0100;
            const ARG_1_AND_2_ARE_WORDS: u16 = 0x0001;
            const WE_HAVE_A_SCALE: u16      = 0x0008;
            const WE_HAVE_AN_X_AND_Y: u16   = 0x0040;
            const WE_HAVE_A_TWO_BY_TWO: u16 = 0x0080;

            let comp_start = comp_off;
            let mut has_instructions = false;
            loop {
                let comp_flags = read_u16_be(tg, comp_off)
                    .ok_or("glyf transform: composite stream truncated")?;
                comp_off += 2;
                has_instructions = has_instructions || (comp_flags & WE_HAVE_INSTRUCTIONS != 0);
                comp_off += 2;
                comp_off += if comp_flags & ARG_1_AND_2_ARE_WORDS != 0 { 4 } else { 2 };
                if      comp_flags & WE_HAVE_A_SCALE    != 0 { comp_off += 2; }
                else if comp_flags & WE_HAVE_AN_X_AND_Y != 0 { comp_off += 4; }
                else if comp_flags & WE_HAVE_A_TWO_BY_TWO != 0 { comp_off += 8; }
                if comp_flags & MORE_COMPONENTS == 0 { break; }
            }

            let comp_data = tg.get(comp_start..comp_off)
                .ok_or("glyf transform: composite stream out of bounds")?;
            let (instr_bytes, instr_len) = if has_instructions {
                let (len, br) = read_255_uint16(tg, gs_off)?;
                gs_off += br;
                let len = len as usize;
                let bytes = tg.get(i_off..i_off + len)
                    .ok_or("glyf transform: composite instructions out of bounds")?.to_vec();
                i_off += len;
                (bytes, len)
            } else {
                (Vec::new(), 0)
            };

            let glyph_size   = 10 + comp_data.len() + if has_instructions { 2 + instr_len } else { 0 };
            let glyph_padded = pad4(glyph_size);
            let mut buf = vec![0u8; glyph_padded];
            write_i16_be(&mut buf, 0, n_contours_raw);
            write_i16_be(&mut buf, 2, x_min);
            write_i16_be(&mut buf, 4, y_min);
            write_i16_be(&mut buf, 6, x_max);
            write_i16_be(&mut buf, 8, y_max);
            buf[10..10 + comp_data.len()].copy_from_slice(comp_data);
            if has_instructions {
                let i_base = 10 + comp_data.len();
                write_u16_be(&mut buf, i_base, instr_len as u16);
                buf[i_base + 2..i_base + 2 + instr_len].copy_from_slice(&instr_bytes);
            }

            let prev = *glyph_offsets.last().unwrap();
            glyph_buffers.push(buf);
            glyph_offsets.push(prev + glyph_padded);
            continue;
        }

        let n_contours = n_contours_raw as usize;
        let mut end_pts = Vec::with_capacity(n_contours);
        let mut total_points = 0usize;
        for _ in 0..n_contours {
            let (cnt, br) = read_255_uint16(tg, np_off)?;
            np_off += br;
            if cnt == 0 {
                return Err("glyf transform: contour with zero points".into());
            }
            total_points += cnt as usize;
            end_pts.push(total_points - 1);
        }
        // TTF point indices are u16, and every point consumes at least one flag
        // byte — a claimed count beyond either bound is malformed and would
        // otherwise drive multi-GB allocations below (OOM = WASM abort) and
        // silently truncate `end_pts as u16` in the rebuilt glyph
        if total_points > 0xFFFF || fl_off + total_points > flag_off + flag_stream_size {
            return Err("glyf transform: point count exceeds flag stream".into());
        }

        let mut on_curve   = vec![0u8; total_points];
        let mut flag_codes = vec![0u8; total_points];
        for p in 0..total_points {
            let raw = *tg.get(fl_off).ok_or("glyf transform: flag stream truncated")?;
            fl_off += 1;
            on_curve[p]   = if raw >> 7 != 0 { 0 } else { 1 }; // bit7=0 means on-curve in WOFF2
            flag_codes[p] = raw & 0x7F;
        }

        let mut x_abs = vec![0i16; total_points];
        let mut y_abs = vec![0i16; total_points];
        let mut cur_x: i16 = 0;
        let mut cur_y: i16 = 0;
        for p in 0..total_points {
            let (dx, dy, new_off) = read_triplet(tg, gs_off, flag_codes[p])?;
            gs_off = new_off;
            cur_x = cur_x.wrapping_add(dx);
            cur_y = cur_y.wrapping_add(dy);
            x_abs[p] = cur_x;
            y_abs[p] = cur_y;
        }

        let (instr_len, br) = read_255_uint16(tg, gs_off)?;
        gs_off += br;
        let instr_len  = instr_len as usize;
        let instr_bytes = tg.get(i_off..i_off + instr_len)
            .ok_or("glyf transform: instructions out of bounds")?.to_vec();
        i_off += instr_len;

        if bbox_bit == 0 && total_points > 0 {
            x_min = x_abs[0]; x_max = x_abs[0];
            y_min = y_abs[0]; y_max = y_abs[0];
            for p in 1..total_points {
                if x_abs[p] < x_min { x_min = x_abs[p]; }
                if x_abs[p] > x_max { x_max = x_abs[p]; }
                if y_abs[p] < y_min { y_min = y_abs[p]; }
                if y_abs[p] > y_max { y_max = y_abs[p]; }
            }
        }

        let mut ttf_flags = vec![0u8; total_points];
        let mut x_deltas  = vec![0i16; total_points];
        let mut y_deltas  = vec![0i16; total_points];
        let mut prev_x: i16 = 0;
        let mut prev_y: i16 = 0;
        for p in 0..total_points {
            let dxp = x_abs[p].wrapping_sub(prev_x);
            let dyp = y_abs[p].wrapping_sub(prev_y);
            prev_x = x_abs[p]; prev_y = y_abs[p];
            x_deltas[p] = dxp; y_deltas[p] = dyp;

            let mut f: u8 = if on_curve[p] != 0 { 0x01 } else { 0x00 };
            if dxp == 0 {
                f |= 0x10;
            } else if dxp >= -255 && dxp <= 255 {
                f |= 0x02;
                if dxp > 0 { f |= 0x10; }
            }
            if dyp == 0 {
                f |= 0x20;
            } else if dyp >= -255 && dyp <= 255 {
                f |= 0x04;
                if dyp > 0 { f |= 0x20; }
            }
            ttf_flags[p] = f;
        }

        let mut encoded_flags: Vec<u8> = Vec::new();
        let mut p = 0;
        while p < total_points {
            encoded_flags.push(ttf_flags[p]);
            let mut rep = 0usize;
            while p + 1 + rep < total_points && rep < 255 && ttf_flags[p + 1 + rep] == ttf_flags[p] {
                rep += 1;
            }
            if rep > 0 {
                *encoded_flags.last_mut().unwrap() |= 0x08;
                encoded_flags.push(rep as u8);
                p += 1 + rep;
            } else {
                p += 1;
            }
        }

        let mut x_bytes: Vec<u8> = Vec::new();
        for p in 0..total_points {
            let f = ttf_flags[p];
            if f & 0x02 != 0 {
                x_bytes.push(x_deltas[p].unsigned_abs() as u8);
            } else if f & 0x10 == 0 {
                x_bytes.push((x_deltas[p] >> 8) as u8);
                x_bytes.push(x_deltas[p] as u8);
            }
        }

        let mut y_bytes: Vec<u8> = Vec::new();
        for p in 0..total_points {
            let f = ttf_flags[p];
            if f & 0x04 != 0 {
                y_bytes.push(y_deltas[p].unsigned_abs() as u8);
            } else if f & 0x20 == 0 {
                y_bytes.push((y_deltas[p] >> 8) as u8);
                y_bytes.push(y_deltas[p] as u8);
            }
        }

        let glyph_data_size = 10
            + n_contours * 2
            + 2 + instr_len
            + encoded_flags.len()
            + x_bytes.len()
            + y_bytes.len();
        let glyph_padded = pad4(glyph_data_size);
        let mut buf = vec![0u8; glyph_padded];
        let mut woff = 0;

        write_i16_be(&mut buf, woff, n_contours_raw); woff += 2;
        write_i16_be(&mut buf, woff, x_min);          woff += 2;
        write_i16_be(&mut buf, woff, y_min);          woff += 2;
        write_i16_be(&mut buf, woff, x_max);          woff += 2;
        write_i16_be(&mut buf, woff, y_max);          woff += 2;
        for c in 0..n_contours {
            write_u16_be(&mut buf, woff, end_pts[c] as u16); woff += 2;
        }
        write_u16_be(&mut buf, woff, instr_len as u16); woff += 2;
        buf[woff..woff + instr_len].copy_from_slice(&instr_bytes); woff += instr_len;
        for &b in &encoded_flags { buf[woff] = b; woff += 1; }
        for &b in &x_bytes       { buf[woff] = b; woff += 1; }
        for &b in &y_bytes       { buf[woff] = b; woff += 1; }

        let prev = *glyph_offsets.last().unwrap();
        glyph_buffers.push(buf);
        glyph_offsets.push(prev + glyph_padded);
    }

    let glyf_size = glyph_offsets[num_glyphs];
    let mut glyf_data = vec![0u8; glyf_size];
    let mut write_pos = 0;
    for gi in 0..num_glyphs {
        glyf_data[write_pos..write_pos + glyph_buffers[gi].len()]
            .copy_from_slice(&glyph_buffers[gi]);
        write_pos += glyph_buffers[gi].len();
    }

    // the declared short format can't address a reconstructed glyf past 128KB
    // (offsets stored as value/2 in a u16) — bump to long instead of silently
    // truncating; the caller writes the returned format into head
    let index_format = if index_format == 0 && glyf_size > 0xFFFF * 2 { 1 } else { index_format };

    let loca_data = if index_format == 0 {
        let mut d = vec![0u8; (num_glyphs + 1) * 2];
        for i in 0..=num_glyphs {
            write_u16_be(&mut d, i * 2, (glyph_offsets[i] / 2) as u16);
        }
        d
    } else {
        let mut d = vec![0u8; (num_glyphs + 1) * 4];
        for i in 0..=num_glyphs {
            write_u32_be(&mut d, i * 4, glyph_offsets[i] as u32);
        }
        d
    };

    Ok((glyf_data, loca_data, index_format))
}

