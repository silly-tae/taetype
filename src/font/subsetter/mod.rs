mod cff_parse;
mod cff_build;
mod cmap;
mod ttf_dir;
mod glyf;
mod seac;

use std::collections::{BTreeSet, HashMap};
use super::decoder::{build_ttf, read_i16_be, read_u16_be, write_i16_be, write_u16_be, write_u32_be};
use cff_parse::{parse_cff_index, parse_top_dict, parse_charset_sids, parse_private_subrs_offset, parse_fd_select_bytes, parse_fd_dict_private};
use cff_build::{encode_cff_index, encode_cff_int};
use ttf_dir::{parse_ttf_dir, slice_table, owned_table};
use glyf::{parse_loca, active_gids, patch_compound_gids};

pub use cmap::cmap_glyph_id;
pub use ttf_dir::{ttf_advance_width, ttf_advance_height};

pub struct SubsetResult {
    pub ttf:     Vec<u8>,
    pub gid_map: Vec<u16>,
}

// total size of a single-object CFF INDEX wrapping `data_len` bytes — needed
// ahead of encoding because the rebuilt Top DICT's offsets depend on it
fn cff_index_size(data_len: usize) -> usize {
    let max_off  = data_len + 1;
    let off_size = if max_off <= 0xFF { 1 } else if max_off <= 0xFFFF { 2 } else if max_off <= 0xFF_FFFF { 3 } else { 4 };
    2 + 1 + 2 * off_size + data_len
}

pub fn subset_cff(cff: &[u8], requested: &[u16]) -> Result<SubsetResult, String> {
    if cff.len() < 4 {
        return Err("CFF: file too short".into());
    }
    let hdr_size = cff[2] as usize;

    let (_, after_name)        = parse_cff_index(cff, hdr_size)?;
    let (top_dicts, after_top) = parse_cff_index(cff, after_name)?;
    let top_dict = top_dicts.into_iter().next()
        .ok_or("CFF: empty Top DICT INDEX")?;
    let fields = parse_top_dict(&top_dict)?;

    let (_, after_strings) = parse_cff_index(cff, after_top)?;
    let (_, after_gsubrs)  = parse_cff_index(cff, after_strings)?;

    let name_index_bytes   = &cff[hdr_size..after_name];
    let string_index_bytes = &cff[after_top..after_strings];
    let gsubr_index_bytes  = &cff[after_strings..after_gsubrs];

    let (charstrings, _) = parse_cff_index(cff, fields.charstrings_off)?;
    let n_glyphs = charstrings.len();

    let charset_bytes = match fields.charset_off {
        Some(off) => {
            let end = parse_charset_sids(cff, off, n_glyphs)?;
            cff[off..end].to_vec()
        }
        None => vec![],
    };

    let mut active = BTreeSet::<u16>::new();
    active.insert(0);
    for &gid in requested {
        if (gid as usize) < n_glyphs { active.insert(gid); }
    }

    // seac-form charstrings compose accented glyphs from two components — chase
    // those references (iterating, since a component could itself be seac) so
    // blanking below never severs a surviving glyph from its parts
    loop {
        let comps = seac::seac_component_gids(&charstrings, &active, cff, fields.charset_off);
        let before = active.len();
        for c in comps {
            if (c as usize) < n_glyphs { active.insert(c); }
        }
        if active.len() == before { break; }
    }

    let endchar = vec![0x0eu8];
    let new_charstrings: Vec<Vec<u8>> = (0..n_glyphs)
        .map(|gid| {
            if active.contains(&(gid as u16)) { charstrings[gid].clone() }
            else { endchar.clone() }
        })
        .collect();
    let new_charstrings_index = encode_cff_index(&new_charstrings);

    if let Some(fd_array_off) = fields.fd_array_off {
        let fd_select_off = fields.fd_select_off
            .ok_or("CFF CID: missing FDSelect offset")?;
        let ros = fields.ros
            .ok_or("CFF CID: missing ROS")?;

        let fdselect_bytes = parse_fd_select_bytes(cff, fd_select_off, n_glyphs)?;

        let (fd_dicts, _) = parse_cff_index(cff, fd_array_off)?;
        let n_fds = fd_dicts.len();
        if n_fds == 0 { return Err("CFF CID: empty FDArray".into()); }

        let mut fd_priv_sizes   = Vec::with_capacity(n_fds);
        let mut fd_priv_bytes   = Vec::with_capacity(n_fds);
        let mut fd_lsubrs_bytes = Vec::with_capacity(n_fds);
        let mut fd_subrs_rels   = Vec::with_capacity(n_fds);
        let mut fd_matrices     = Vec::with_capacity(n_fds);

        for fd_dict in &fd_dicts {
            let (priv_size, priv_off, fd_matrix) = parse_fd_dict_private(fd_dict);
            let priv_end = priv_off.saturating_add(priv_size);
            if priv_end > cff.len() {
                return Err("CFF CID: FD Private DICT out of bounds".into());
            }
            let priv_data = cff[priv_off..priv_end].to_vec();
            // a Subrs offset inside the Private DICT itself is malformed — the
            // rebuild pads out to new_priv_off + subrs_rel, which only lands
            // correctly when the subrs follow the dict
            let subrs_rel = match parse_private_subrs_offset(&priv_data) {
                r if r >= priv_size => r,
                _ => 0,
            };
            let lsubrs = if subrs_rel > 0 {
                let abs = priv_off + subrs_rel;
                if abs < cff.len() {
                    let (_, end) = parse_cff_index(cff, abs)?;
                    cff[abs..end].to_vec()
                } else { vec![] }
            } else { vec![] };
            fd_priv_sizes.push(priv_size);
            fd_lsubrs_bytes.push(lsubrs);
            fd_subrs_rels.push(subrs_rel);
            fd_priv_bytes.push(priv_data);
            fd_matrices.push(fd_matrix);
        }

        // Measure FDArray INDEX size with correctly-sized placeholders: each FD
        // DICT is Private(5+5+1) plus its FontMatrix raw bytes when present —
        // encode_cff_int is always 5 bytes, so sizes are deterministic
        let fd_placeholder: Vec<Vec<u8>> = (0..n_fds)
            .map(|i| vec![0u8; 11 + fd_matrices[i].as_ref().map_or(0, |m: &Vec<u8>| m.len())])
            .collect();
        let fdarray_size = encode_cff_index(&fd_placeholder).len();

        // CID Top DICT data: ROS(17) + charset(6) + CharStrings(6) + FDArray(7) + FDSelect(7)
        // = 43 bytes, plus the original FontMatrix (raw bytes) when present
        let fm_len = fields.font_matrix_raw.as_ref().map_or(0, |m| m.len());
        let top_dict_data_len   = 43 + fm_len;
        let top_dict_index_size = cff_index_size(top_dict_data_len);
        let base = hdr_size + name_index_bytes.len() + top_dict_index_size
            + string_index_bytes.len() + gsubr_index_bytes.len();

        let charset_val         = if charset_bytes.is_empty() { 0i32 } else { base as i32 };
        let new_fdselect_off    = base + charset_bytes.len();
        let new_charstrings_off = new_fdselect_off + fdselect_bytes.len();
        let new_fdarray_off     = new_charstrings_off + new_charstrings_index.len();

        let mut fd_new_priv_offs = Vec::with_capacity(n_fds);
        let mut cur = new_fdarray_off + fdarray_size;
        for i in 0..n_fds {
            fd_new_priv_offs.push(cur);
            if fd_subrs_rels[i] > 0 && !fd_lsubrs_bytes[i].is_empty() {
                cur += fd_subrs_rels[i] + fd_lsubrs_bytes[i].len();
            } else {
                cur += fd_priv_sizes[i];
            }
        }

        let fd_dict_entries: Vec<Vec<u8>> = (0..n_fds)
            .map(|i| {
                let mut d = Vec::with_capacity(fd_placeholder[i].len());
                if let Some(ref fm) = fd_matrices[i] {
                    d.extend_from_slice(fm);
                }
                d.extend(encode_cff_int(fd_priv_sizes[i] as i32));
                d.extend(encode_cff_int(fd_new_priv_offs[i] as i32));
                d.push(18);
                d
            })
            .collect();
        let new_fdarray_index = encode_cff_index(&fd_dict_entries);
        debug_assert_eq!(new_fdarray_index.len(), fdarray_size);

        let mut top_dict_data = Vec::with_capacity(top_dict_data_len);
        top_dict_data.extend(encode_cff_int(ros.0));
        top_dict_data.extend(encode_cff_int(ros.1));
        top_dict_data.extend(encode_cff_int(ros.2));
        top_dict_data.extend_from_slice(&[12u8, 30u8]); // ROS
        if let Some(ref fm) = fields.font_matrix_raw {
            top_dict_data.extend_from_slice(fm);
        }
        top_dict_data.extend(encode_cff_int(charset_val));
        top_dict_data.push(15); // charset
        top_dict_data.extend(encode_cff_int(new_charstrings_off as i32));
        top_dict_data.push(17); // CharStrings
        top_dict_data.extend(encode_cff_int(new_fdarray_off as i32));
        top_dict_data.extend_from_slice(&[12u8, 36u8]); // FDArray
        top_dict_data.extend(encode_cff_int(new_fdselect_off as i32));
        top_dict_data.extend_from_slice(&[12u8, 37u8]); // FDSelect

        let new_top_dict_index = encode_cff_index(&[top_dict_data]);
        debug_assert_eq!(new_top_dict_index.len(), top_dict_index_size);

        let mut out = Vec::new();
        out.extend_from_slice(&cff[..hdr_size]);
        out.extend_from_slice(name_index_bytes);
        out.extend_from_slice(&new_top_dict_index);
        out.extend_from_slice(string_index_bytes);
        out.extend_from_slice(gsubr_index_bytes);
        out.extend_from_slice(&charset_bytes);
        out.extend_from_slice(&fdselect_bytes);
        out.extend_from_slice(&new_charstrings_index);
        out.extend_from_slice(&new_fdarray_index);

        for i in 0..n_fds {
            out.extend_from_slice(&fd_priv_bytes[i]);
            if fd_subrs_rels[i] > 0 && !fd_lsubrs_bytes[i].is_empty() {
                let target = fd_new_priv_offs[i] + fd_subrs_rels[i];
                while out.len() < target { out.push(0); }
                out.extend_from_slice(&fd_lsubrs_bytes[i]);
            }
        }

        return Ok(SubsetResult { ttf: out, gid_map: vec![] });
    }

    // Name-keyed path
    let priv_end = fields.private_off.saturating_add(fields.private_size);
    if priv_end > cff.len() {
        return Err("CFF: Private DICT out of bounds".into());
    }
    let private_bytes     = cff[fields.private_off..priv_end].to_vec();
    // same overlap guard as the FD path: subrs must follow the dict for the
    // padded layout to place them where the copied Subrs operator points
    let subrs_rel         = match parse_private_subrs_offset(&private_bytes) {
        r if r >= fields.private_size => r,
        _ => 0,
    };
    let local_subrs_bytes = if subrs_rel > 0 {
        let subrs_abs = fields.private_off + subrs_rel;
        if subrs_abs < cff.len() {
            let (_, end) = parse_cff_index(cff, subrs_abs)?;
            cff[subrs_abs..end].to_vec()
        } else {
            vec![]
        }
    } else {
        vec![]
    };

    // Top DICT data: charset(6) + CharStrings(6) + Private(11) = 23 bytes, plus
    // the original FontMatrix (raw bytes) when present
    let fm_len = fields.font_matrix_raw.as_ref().map_or(0, |m| m.len());
    let top_dict_data_len   = 23 + fm_len;
    let top_dict_index_size = cff_index_size(top_dict_data_len);
    let base = hdr_size
        + name_index_bytes.len()
        + top_dict_index_size
        + string_index_bytes.len()
        + gsubr_index_bytes.len();

    let charset_value       = if charset_bytes.is_empty() { 0i32 } else { base as i32 };
    let new_charstrings_off = base + charset_bytes.len();
    let new_private_off     = new_charstrings_off + new_charstrings_index.len();

    let mut top_dict_data = Vec::with_capacity(top_dict_data_len);
    if let Some(ref fm) = fields.font_matrix_raw {
        top_dict_data.extend_from_slice(fm);
    }
    top_dict_data.extend(encode_cff_int(charset_value));
    top_dict_data.push(15); // charset
    top_dict_data.extend(encode_cff_int(new_charstrings_off as i32));
    top_dict_data.push(17); // CharStrings
    top_dict_data.extend(encode_cff_int(fields.private_size as i32));
    top_dict_data.extend(encode_cff_int(new_private_off as i32));
    top_dict_data.push(18); // Private

    let new_top_dict_index = encode_cff_index(&[top_dict_data]);
    debug_assert_eq!(new_top_dict_index.len(), top_dict_index_size);

    let mut out = Vec::new();
    out.extend_from_slice(&cff[..hdr_size]);
    out.extend_from_slice(name_index_bytes);
    out.extend_from_slice(&new_top_dict_index);
    out.extend_from_slice(string_index_bytes);
    out.extend_from_slice(gsubr_index_bytes);
    out.extend_from_slice(&charset_bytes);
    out.extend_from_slice(&new_charstrings_index);
    out.extend_from_slice(&private_bytes);

    if !local_subrs_bytes.is_empty() && subrs_rel > 0 {
        let target = new_private_off + subrs_rel;
        while out.len() < target { out.push(0); }
        out.extend_from_slice(&local_subrs_bytes);
    }

    Ok(SubsetResult { ttf: out, gid_map: vec![] })
}

// post's version 2.0 payload (glyphNameIndex, sized to the ORIGINAL glyph
// count) would silently disagree with the subset's maxp.numGlyphs if copied
// verbatim — downgrading to version 3.0 keeps the shared 32-byte header
// (italicAngle, underline metrics, isFixedPitch) that PDF viewers and browsers
// actually read, while dropping the only part that can no longer be correct
// post-subset. Every post version shares that header layout, so this is safe
// regardless of the source table's version.
fn fix_post_table(mut post: Vec<u8>) -> Vec<u8> {
    if post.len() < 32 { return post; }
    post.truncate(32);
    write_u32_be(&mut post, 0, 0x0003_0000);
    post
}

pub fn subset_ttf(ttf: &[u8], requested: &[u16]) -> Result<SubsetResult, String> {
    let dir = parse_ttf_dir(ttf);

    let head    = slice_table(ttf, &dir, "head").ok_or("subset: missing head")?;
    let maxp    = slice_table(ttf, &dir, "maxp").ok_or("subset: missing maxp")?;
    let loca_sl = slice_table(ttf, &dir, "loca").ok_or("subset: missing loca")?;
    let glyf    = slice_table(ttf, &dir, "glyf").ok_or("subset: missing glyf")?;

    let loca_fmt   = read_i16_be(head, 50).ok_or("subset: head table truncated")?;
    let num_glyphs = read_u16_be(maxp, 4).ok_or("subset: maxp table truncated")? as usize;
    if num_glyphs == 0 { return Err("subset: maxp reports zero glyphs".into()); }
    let loca_offs  = parse_loca(loca_sl, loca_fmt, num_glyphs);
    let active     = active_gids(requested, glyf, &loca_offs, num_glyphs);

    // BTreeSet is sorted, so compact 0 = lowest orig GID (always 0 = null)
    let active_sorted: Vec<u16> = active.iter().copied().collect();
    let n_active = active_sorted.len();

    let max_orig = *active_sorted.last().unwrap_or(&0) as usize;
    let mut gid_map = vec![0u16; max_orig + 1];
    for (compact, &orig) in active_sorted.iter().enumerate() {
        gid_map[orig as usize] = compact as u16;
    }

    let mut new_glyf: Vec<u8> = Vec::new();
    let mut new_loca = vec![0u32; n_active + 1];

    for (compact, &orig_gid) in active_sorted.iter().enumerate() {
        new_loca[compact] = new_glyf.len() as u32;
        let (s, e) = (loca_offs[orig_gid as usize], loca_offs[orig_gid as usize + 1]);
        if s < e && e <= glyf.len() {
            let glyph_start = new_glyf.len();
            new_glyf.extend_from_slice(&glyf[s..e]);
            let is_compound = read_i16_be(&new_glyf, glyph_start) == Some(-1);
            if is_compound {
                let glyph_end = new_glyf.len();
                patch_compound_gids(&mut new_glyf, glyph_start, glyph_end, &gid_map);
            }
            while new_glyf.len() % 4 != 0 { new_glyf.push(0); }
        }
    }
    new_loca[n_active] = new_glyf.len() as u32;

    // Use u16 loca (format 0) when glyf fits — offsets stored as value/2, max 131070 bytes
    let use_short_loca = new_glyf.len() <= 0x1_FFFE;
    let new_loca_bytes = if use_short_loca {
        let mut b = vec![0u8; (n_active + 1) * 2];
        for (i, &off) in new_loca.iter().enumerate() {
            write_u16_be(&mut b, i * 2, (off / 2) as u16);
        }
        b
    } else {
        let mut b = vec![0u8; (n_active + 1) * 4];
        for (i, &off) in new_loca.iter().enumerate() {
            write_u32_be(&mut b, i * 4, off);
        }
        b
    };

    let mut new_head = head.to_vec();
    write_i16_be(&mut new_head, 50, if use_short_loca { 0 } else { 1 });

    let (new_hmtx, new_hhea) = {
        let orig_hmtx   = slice_table(ttf, &dir, "hmtx").unwrap_or(&[]);
        let orig_hhea   = owned_table(ttf, &dir, "hhea").unwrap_or_default();
        let orig_num_hm = if orig_hhea.len() >= 36 {
            read_u16_be(&orig_hhea, 34).unwrap_or(0) as usize
        } else { 0 };
        let last_aw = if orig_num_hm > 0 && orig_num_hm * 4 <= orig_hmtx.len() {
            read_u16_be(orig_hmtx, (orig_num_hm - 1) * 4).unwrap_or(0)
        } else { 0 };

        let mut h = vec![0u8; n_active * 4];
        for (compact, &orig_gid) in active_sorted.iter().enumerate() {
            let gid = orig_gid as usize;
            let (aw, lsb) = if gid < orig_num_hm {
                let off = gid * 4;
                let aw  = if off + 2 <= orig_hmtx.len() { read_u16_be(orig_hmtx, off).unwrap_or(0) } else { 0 };
                let lsb = if off + 4 <= orig_hmtx.len() { read_i16_be(orig_hmtx, off + 2).unwrap_or(0) } else { 0 };
                (aw, lsb)
            } else {
                let lsb_off = orig_num_hm * 4 + (gid - orig_num_hm) * 2;
                let lsb = if lsb_off + 2 <= orig_hmtx.len() { read_i16_be(orig_hmtx, lsb_off).unwrap_or(0) } else { 0 };
                (last_aw, lsb)
            };
            write_u16_be(&mut h, compact * 4, aw);
            write_i16_be(&mut h, compact * 4 + 2, lsb);
        }
        let mut new_hhea = orig_hhea;
        if new_hhea.len() >= 36 { write_u16_be(&mut new_hhea, 34, n_active as u16); }
        (h, new_hhea)
    };

    // vertical metrics mirror hmtx exactly (advance height + top side bearing,
    // long-metrics count in vhea offset 34) — kept for writing-mode support
    let vertical = {
        let orig_vmtx = slice_table(ttf, &dir, "vmtx");
        let orig_vhea = owned_table(ttf, &dir, "vhea");
        match (orig_vmtx, orig_vhea) {
            (Some(vmtx), Some(mut vhea)) if vhea.len() >= 36 => {
                let orig_num_vm = read_u16_be(&vhea, 34).unwrap_or(0) as usize;
                let last_ah = if orig_num_vm > 0 && orig_num_vm * 4 <= vmtx.len() {
                    read_u16_be(vmtx, (orig_num_vm - 1) * 4).unwrap_or(0)
                } else { 0 };
                let mut v = vec![0u8; n_active * 4];
                for (compact, &orig_gid) in active_sorted.iter().enumerate() {
                    let gid = orig_gid as usize;
                    let (ah, tsb) = if gid < orig_num_vm {
                        let off = gid * 4;
                        let ah  = if off + 2 <= vmtx.len() { read_u16_be(vmtx, off).unwrap_or(0) } else { 0 };
                        let tsb = if off + 4 <= vmtx.len() { read_i16_be(vmtx, off + 2).unwrap_or(0) } else { 0 };
                        (ah, tsb)
                    } else {
                        let tsb_off = orig_num_vm * 4 + (gid - orig_num_vm) * 2;
                        let tsb = if tsb_off + 2 <= vmtx.len() { read_i16_be(vmtx, tsb_off).unwrap_or(0) } else { 0 };
                        (last_ah, tsb)
                    };
                    write_u16_be(&mut v, compact * 4, ah);
                    write_i16_be(&mut v, compact * 4 + 2, tsb);
                }
                write_u16_be(&mut vhea, 34, n_active as u16);
                Some((v, vhea))
            }
            _ => None,
        }
    };

    let new_maxp = {
        let mut m = owned_table(ttf, &dir, "maxp").unwrap_or_default();
        if m.len() >= 6 { write_u16_be(&mut m, 4, n_active as u16); }
        m
    };

    let mut tmap: HashMap<String, Vec<u8>> = HashMap::new();
    if let Some(d) = owned_table(ttf, &dir, "OS/2") { tmap.insert("OS/2".to_string(), d); }
    // glyphs keep their TrueType instructions, which call into fpgm/prep and read
    // cvt — dropping those leaves dangling references that strict rasterizers reject
    for tag in ["cvt ", "fpgm", "prep"] {
        if let Some(d) = owned_table(ttf, &dir, tag) { tmap.insert(tag.to_string(), d); }
    }
    // post carries metadata PDF viewers expect on embedded TrueType — see
    // fix_post_table for why it can't be copied verbatim like the tables above
    if let Some(post) = owned_table(ttf, &dir, "post") {
        tmap.insert("post".to_string(), fix_post_table(post));
    }
    tmap.insert("maxp".to_string(), new_maxp);
    tmap.insert("hmtx".to_string(), new_hmtx);
    tmap.insert("hhea".to_string(), new_hhea);
    tmap.insert("head".to_string(), new_head);
    tmap.insert("glyf".to_string(), new_glyf);
    tmap.insert("loca".to_string(), new_loca_bytes);
    if let Some((new_vmtx, new_vhea)) = vertical {
        tmap.insert("vmtx".to_string(), new_vmtx);
        tmap.insert("vhea".to_string(), new_vhea);
    }

    Ok(SubsetResult { ttf: build_ttf(&tmap), gid_map })
}
