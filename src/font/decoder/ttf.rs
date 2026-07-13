use std::collections::HashMap;
use super::io::{read_u16_be, read_u32_be, write_u16_be, write_u32_be};

fn checksum32(data: &[u8]) -> u32 {
    let mut sum: u32 = 0;
    let words = data.len() / 4;
    for i in 0..words {
        // read_u32_be can't fail here: i < words = data.len()/4, so i*4+4 <= data.len().
        sum = sum.wrapping_add(read_u32_be(data, i * 4).unwrap_or(0));
    }
    let rem = data.len() % 4;
    if rem > 0 {
        let mut last: u32 = 0;
        for i in 0..rem {
            last |= (data[data.len() - rem + i] as u32) << ((3 - i) * 8);
        }
        sum = sum.wrapping_add(last);
    }
    sum
}

fn pad4(n: usize) -> usize {
    (n + 3) & !3
}

pub fn build_ttf(table_map: &HashMap<String, Vec<u8>>) -> Vec<u8> {
    let mut tags: Vec<&String> = table_map.keys().collect();
    tags.sort();
    let num_tables = tags.len();
    // leading_zeros(0) underflows the log2 math below — and with panic=abort a
    // degenerate zero-table font would take down the whole WASM engine
    if num_tables == 0 { return Vec::new(); }

    let floor_log2     = (usize::BITS - num_tables.leading_zeros() - 1) as usize;
    let search_range   = (1usize << floor_log2) * 16;
    let entry_selector = floor_log2;
    let range_shift    = num_tables * 16 - search_range;

    let sfnt_hdr  = 12;
    let dir_size  = num_tables * 16;
    let mut data_off = sfnt_hdr + dir_size;

    let mut tbl_offsets: Vec<usize>   = Vec::with_capacity(num_tables);
    let mut tbl_padded:  Vec<Vec<u8>> = Vec::with_capacity(num_tables);

    for &tag in &tags {
        let raw = &table_map[tag];
        let mut p = vec![0u8; pad4(raw.len())];
        p[..raw.len()].copy_from_slice(raw);
        tbl_offsets.push(data_off);
        data_off += p.len();
        tbl_padded.push(p);
    }

    let mut out = vec![0u8; data_off];

    write_u32_be(&mut out, 0,  0x0001_0000);
    write_u16_be(&mut out, 4,  num_tables as u16);
    write_u16_be(&mut out, 6,  search_range as u16);
    write_u16_be(&mut out, 8,  entry_selector as u16);
    write_u16_be(&mut out, 10, range_shift as u16);

    let mut dir_off = sfnt_hdr;
    for (i, &tag) in tags.iter().enumerate() {
        let cs = checksum32(&tbl_padded[i]);
        for (j, &b) in tag.as_bytes().iter().take(4).enumerate() {
            out[dir_off + j] = b;
        }
        write_u32_be(&mut out, dir_off + 4,  cs);
        write_u32_be(&mut out, dir_off + 8,  tbl_offsets[i] as u32);
        write_u32_be(&mut out, dir_off + 12, table_map[tag].len() as u32);
        dir_off += 16;
    }

    for (i, _) in tags.iter().enumerate() {
        let off = tbl_offsets[i];
        out[off..off + tbl_padded[i].len()].copy_from_slice(&tbl_padded[i]);
    }

    if let Some(pos) = tags.iter().position(|t| t.as_str() == "head") {
        let head_off = tbl_offsets[pos];
        write_u32_be(&mut out, head_off + 8, 0);
        let mut file_cs: u32 = 0;
        let total = out.len();
        let words = total / 4;
        for i in 0..words {
            // read_u32_be can't fail here: i < words = out.len()/4, so i*4+4 <= out.len().
            file_cs = file_cs.wrapping_add(read_u32_be(&out, i * 4).unwrap_or(0));
        }
        let rem = total % 4;
        if rem > 0 {
            let mut last: u32 = 0;
            for i in 0..rem {
                last |= (out[total - rem + i] as u32) << ((3 - i) * 8);
            }
            file_cs = file_cs.wrapping_add(last);
        }
        write_u32_be(&mut out, head_off + 8, 0xB1B0_AFBA_u32.wrapping_sub(file_cs));
    }

    out
}

pub fn extract_ttf_tables(data: &[u8]) -> Result<HashMap<String, Vec<u8>>, String> {
    extract_ttf_tables_at(data, 0)
}

// 'ttcf' collections hold several fonts sharing one file — the directory at
// `dir_off` belongs to one member font; table offsets stay file-absolute.
pub fn extract_ttc_tables(data: &[u8], index: usize) -> Result<HashMap<String, Vec<u8>>, String> {
    if read_u32_be(data, 0) != Some(0x7474_6366) {
        return Err("Not a TTC file".into());
    }
    let num_fonts = read_u32_be(data, 8).ok_or("TTC: header truncated")? as usize;
    if index >= num_fonts {
        return Err(format!("TTC: font index {} out of range ({} fonts)", index, num_fonts));
    }
    let dir_off = read_u32_be(data, 12 + index * 4).ok_or("TTC: offset table truncated")? as usize;
    extract_ttf_tables_at(data, dir_off)
}

pub fn ttc_font_count(data: &[u8]) -> usize {
    if read_u32_be(data, 0) != Some(0x7474_6366) { return 0; }
    read_u32_be(data, 8).unwrap_or(0) as usize
}

fn extract_ttf_tables_at(data: &[u8], dir_off: usize) -> Result<HashMap<String, Vec<u8>>, String> {
    if data.len() < dir_off + 12 {
        return Err("TTF/OTF: file too short".into());
    }
    let sfversion = read_u32_be(data, dir_off).ok_or("TTF/OTF: header truncated")?;
    // 0x00010000 = TTF, 0x4F54544F = 'OTTO' (CFF), 0x74727565 = 'true' (Apple TTF)
    if sfversion != 0x0001_0000 && sfversion != 0x4F54_544F && sfversion != 0x7472_7565 {
        return Err(format!("Not a TTF/OTF file (signature: 0x{:08X})", sfversion));
    }
    let num_tables = read_u16_be(data, dir_off + 4).ok_or("TTF/OTF: header truncated")? as usize;
    if data.len() < dir_off + 12 + num_tables * 16 {
        return Err("TTF/OTF: table directory truncated".into());
    }
    let mut map = HashMap::new();
    for i in 0..num_tables {
        let e      = dir_off + 12 + i * 16;
        let tag_bytes = data.get(e..e + 4)
            .ok_or_else(|| format!("TTF/OTF: table tag truncated at entry {}", i))?;
        let tag    = std::str::from_utf8(tag_bytes)
            .map_err(|_| format!("TTF/OTF: invalid tag bytes at entry {}", i))?
            .to_string();
        let offset = read_u32_be(data, e + 8)
            .ok_or_else(|| format!("TTF/OTF: table directory entry '{}' truncated", tag))? as usize;
        let length = read_u32_be(data, e + 12)
            .ok_or_else(|| format!("TTF/OTF: table directory entry '{}' truncated", tag))? as usize;
        if offset.saturating_add(length) > data.len() {
            return Err(format!("TTF/OTF: table '{}' out of bounds", tag));
        }
        map.insert(tag, data[offset..offset + length].to_vec());
    }
    Ok(map)
}
