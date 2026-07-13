use std::collections::HashMap;
use super::super::decoder::{read_u16_be, read_u32_be, write_u16_be, write_u32_be};

pub fn parse_loca(table_map: &HashMap<String, Vec<u8>>, loca_format: i16, num_glyphs: usize) -> Result<Vec<usize>, String> {
    let loca = table_map.get("loca").ok_or("missing loca")?;
    let mut offsets = vec![0usize; num_glyphs + 1];
    if loca_format == 0 {
        for i in 0..=num_glyphs {
            if let Some(v) = read_u16_be(loca, i * 2) { offsets[i] = v as usize * 2; }
        }
    } else {
        for i in 0..=num_glyphs {
            if let Some(v) = read_u32_be(loca, i * 4) { offsets[i] = v as usize; }
        }
    }
    Ok(offsets)
}

pub fn build_loca_table(new_loca: &[usize], loca_format: i16) -> Vec<u8> {
    let n = new_loca.len();
    if loca_format == 0 {
        let mut out = vec![0u8; n * 2];
        for i in 0..n { write_u16_be(&mut out, i * 2, (new_loca[i] / 2) as u16); }
        out
    } else {
        let mut out = vec![0u8; n * 4];
        for i in 0..n { write_u32_be(&mut out, i * 4, new_loca[i] as u32); }
        out
    }
}
