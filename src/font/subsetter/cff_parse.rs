use super::super::decoder::read_u16_be;

pub fn parse_cff_index(data: &[u8], off: usize) -> Result<(Vec<Vec<u8>>, usize), String> {
    if off + 2 > data.len() {
        return Err("CFF INDEX: truncated".into());
    }
    let count = read_u16_be(data, off).ok_or("CFF INDEX: truncated")? as usize;
    if count == 0 {
        return Ok((vec![], off + 2));
    }
    if off + 3 > data.len() {
        return Err("CFF INDEX: missing offSize".into());
    }
    let off_size = data[off + 2] as usize;
    if off_size < 1 || off_size > 4 {
        return Err(format!("CFF INDEX: invalid offSize {}", off_size));
    }
    let offsets_start  = off + 3;
    let offsets_count  = count + 1;
    let data_start     = offsets_start + offsets_count * off_size;

    let mut offsets = Vec::with_capacity(offsets_count);
    for i in 0..offsets_count {
        let o = offsets_start + i * off_size;
        if o + off_size > data.len() {
            return Err("CFF INDEX: offsets truncated".into());
        }
        let mut v = 0usize;
        for j in 0..off_size {
            v = (v << 8) | data[o + j] as usize;
        }
        offsets.push(v);
    }

    let data_len = offsets[count].saturating_sub(1);
    if data_start + data_len > data.len() {
        return Err("CFF INDEX: data truncated".into());
    }

    let mut objects = Vec::with_capacity(count);
    for i in 0..count {
        let start = data_start + offsets[i].saturating_sub(1);
        let end   = data_start + offsets[i + 1].saturating_sub(1);
        // offsets in a valid INDEX are monotonic — a decreasing pair from a
        // corrupt font would make start > end, and that slice PANICS (which is
        // an abort for the whole WASM instance under panic=abort)
        if start > end || end > data.len() {
            return Err(format!("CFF INDEX: object {} out of bounds", i));
        }
        objects.push(data[start..end].to_vec());
    }

    Ok((objects, data_start + data_len))
}

fn decode_cff_number(data: &[u8], off: usize) -> Result<(i32, usize), String> {
    if off >= data.len() {
        return Err("CFF DICT: truncated number".into());
    }
    let b0 = data[off] as i32;
    match b0 {
        28 => {
            if off + 3 > data.len() { return Err("CFF DICT: short 3-byte int".into()); }
            let v = ((data[off + 1] as i32) << 8) | data[off + 2] as i32;
            Ok((if v & 0x8000 != 0 { v | !0xFFFF } else { v }, 3))
        }
        29 => {
            if off + 5 > data.len() { return Err("CFF DICT: short 5-byte int".into()); }
            let v = ((data[off + 1] as i32) << 24)
                  | ((data[off + 2] as i32) << 16)
                  | ((data[off + 3] as i32) << 8)
                  |   data[off + 4] as i32;
            Ok((v, 5))
        }
        30 => {
            // real number — skip to the terminating 0xF nibble
            let mut p = off + 1;
            loop {
                if p >= data.len() { return Err("CFF DICT: unterminated real".into()); }
                let b = data[p]; p += 1;
                if (b & 0x0F) == 0x0F || (b >> 4) == 0x0F { break; }
            }
            Ok((0, p - off))
        }
        32..=246  => Ok((b0 - 139, 1)),
        247..=250 => {
            if off + 2 > data.len() { return Err("CFF DICT: short 2-byte int".into()); }
            Ok(((b0 - 247) * 256 + data[off + 1] as i32 + 108, 2))
        }
        251..=254 => {
            if off + 2 > data.len() { return Err("CFF DICT: short 2-byte int".into()); }
            Ok((-(b0 - 251) * 256 - data[off + 1] as i32 - 108, 2))
        }
        _ => Err(format!("CFF DICT: unexpected byte 0x{:02X} at offset {}", b0, off)),
    }
}

pub struct TopDictFields {
    pub charset_off:     Option<usize>,
    pub charstrings_off: usize,
    pub private_size:    usize,
    pub private_off:     usize,
    pub fd_array_off:    Option<usize>,
    pub fd_select_off:   Option<usize>,
    pub ros:             Option<(i32, i32, i32)>,
    // raw operand bytes + operator, verbatim — FontMatrix operands are reals,
    // which decode_cff_number can only skip, not re-encode
    pub font_matrix_raw: Option<Vec<u8>>,
}

pub fn parse_top_dict(dict: &[u8]) -> Result<TopDictFields, String> {
    let mut operands      = Vec::<i32>::new();
    let mut charset_off   = None;
    let mut cs_off        = 0usize;
    let mut priv_size     = 0usize;
    let mut priv_off      = 0usize;
    let mut fd_array_off  = None;
    let mut fd_select_off = None;
    let mut ros           = None;
    let mut font_matrix_raw: Option<Vec<u8>> = None;
    let mut off           = 0usize;
    let mut operand_start = 0usize;

    while off < dict.len() {
        let b = dict[off];
        if b == 12 {
            if off + 1 < dict.len() {
                let b2 = dict[off + 1];
                match b2 {
                    30 => {
                        if operands.len() >= 3 {
                            let n = operands.len();
                            ros = Some((operands[n-3], operands[n-2], operands[n-1]));
                        }
                    }
                    36 => { if let Some(&v) = operands.last() { fd_array_off  = Some(v as usize); } }
                    37 => { if let Some(&v) = operands.last() { fd_select_off = Some(v as usize); } }
                    7  => {
                        let mut raw = dict[operand_start..off].to_vec();
                        raw.extend_from_slice(&[12, 7]);
                        font_matrix_raw = Some(raw);
                    }
                    _  => {}
                }
            }
            operands.clear();
            off += 2;
            operand_start = off;
        } else if b <= 21 {
            match b {
                15 => {
                    if let Some(&v) = operands.last() {
                        if v > 2 { charset_off = Some(v as usize); }
                    }
                }
                17 => { if let Some(&v) = operands.last() { cs_off = v as usize; } }
                18 => {
                    if operands.len() >= 2 {
                        priv_size = operands[operands.len() - 2] as usize;
                        priv_off  = operands[operands.len() - 1] as usize;
                    }
                }
                _ => {}
            }
            operands.clear();
            off += 1;
            operand_start = off;
        } else {
            let (v, sz) = decode_cff_number(dict, off)?;
            operands.push(v);
            off += sz;
        }
    }

    if cs_off == 0 { return Err("CFF Top DICT: missing CharStrings offset".into()); }
    if priv_off == 0 && fd_array_off.is_none() {
        return Err("CFF Top DICT: missing Private DICT and FDArray".into());
    }

    Ok(TopDictFields {
        charset_off,
        charstrings_off: cs_off,
        private_size:    priv_size,
        private_off:     priv_off,
        fd_array_off,
        fd_select_off,
        ros,
        font_matrix_raw,
    })
}

pub fn parse_charset_sids(cff: &[u8], off: usize, n_glyphs: usize) -> Result<usize, String> {
    if off >= cff.len() { return Err("CFF charset: offset out of range".into()); }
    if n_glyphs <= 1 { return Ok(off); }
    let format = cff[off];
    let mut pos = off + 1;

    match format {
        0 => {
            for _ in 1..n_glyphs {
                if pos + 2 > cff.len() { return Err("CFF charset format 0: truncated".into()); }
                pos += 2;
            }
        }
        1 => {
            let mut gid = 1usize;
            while gid < n_glyphs {
                if pos + 3 > cff.len() { return Err("CFF charset format 1: truncated".into()); }
                pos += 2;
                let n_left = cff[pos] as usize; pos += 1;
                gid += (n_left + 1).min(n_glyphs - gid);
            }
        }
        2 => {
            let mut gid = 1usize;
            while gid < n_glyphs {
                if pos + 4 > cff.len() { return Err("CFF charset format 2: truncated".into()); }
                pos += 2;
                let n_left = read_u16_be(cff, pos).ok_or("CFF charset format 2: truncated")? as usize;
                pos += 2;
                gid += (n_left + 1).min(n_glyphs - gid);
            }
        }
        _ => return Err(format!("CFF charset: unknown format {}", format)),
    }

    Ok(pos)
}

pub fn parse_private_subrs_offset(private_dict: &[u8]) -> usize {
    let mut operands = Vec::<i32>::new();
    let mut off      = 0usize;
    while off < private_dict.len() {
        let b = private_dict[off];
        if b == 12 {
            operands.clear();
            off += 2;
        } else if b <= 21 {
            if b == 19 {
                if let Some(&v) = operands.last() {
                    return if v > 0 { v as usize } else { 0 };
                }
            }
            operands.clear();
            off += 1;
        } else {
            match decode_cff_number(private_dict, off) {
                Ok((v, sz)) => { operands.push(v); off += sz; }
                Err(_)      => break,
            }
        }
    }
    0
}

pub fn parse_fd_select_bytes(cff: &[u8], off: usize, n_glyphs: usize) -> Result<Vec<u8>, String> {
    if off >= cff.len() { return Err("CFF FDSelect: offset out of bounds".into()); }
    let format = cff[off];
    let end = match format {
        0 => off + 1 + n_glyphs,
        3 => {
            if off + 3 > cff.len() { return Err("CFF FDSelect format 3: truncated".into()); }
            let n_ranges = read_u16_be(cff, off + 1)
                .ok_or("CFF FDSelect format 3: truncated")? as usize;
            off + 1 + 2 + n_ranges * 3 + 2
        }
        _ => return Err(format!("CFF FDSelect: unknown format {}", format)),
    };
    if end > cff.len() { return Err("CFF FDSelect: data truncated".into()); }
    Ok(cff[off..end].to_vec())
}

// (priv_size, priv_off, font_matrix_raw) — the matrix is carried as raw bytes
// for the same reason as the Top DICT's (fix #14): real-number operands can't
// round-trip the integer decoder
pub fn parse_fd_dict_private(dict: &[u8]) -> (usize, usize, Option<Vec<u8>>) {
    let mut operands  = Vec::<i32>::new();
    let mut off       = 0usize;
    let mut operand_start = 0usize;
    let mut priv_size = 0usize;
    let mut priv_off  = 0usize;
    let mut font_matrix_raw: Option<Vec<u8>> = None;
    while off < dict.len() {
        let b = dict[off];
        if b == 12 {
            if off + 1 < dict.len() && dict[off + 1] == 7 {
                let mut raw = dict[operand_start..off].to_vec();
                raw.extend_from_slice(&[12, 7]);
                font_matrix_raw = Some(raw);
            }
            operands.clear();
            off += 2;
            operand_start = off;
        } else if b <= 21 {
            if b == 18 && operands.len() >= 2 {
                priv_size = operands[operands.len() - 2] as usize;
                priv_off  = operands[operands.len() - 1] as usize;
            }
            operands.clear();
            off += 1;
            operand_start = off;
        } else {
            match decode_cff_number(dict, off) {
                Ok((v, sz)) => { operands.push(v); off += sz; }
                Err(_)      => break,
            }
        }
    }
    (priv_size, priv_off, font_matrix_raw)
}
