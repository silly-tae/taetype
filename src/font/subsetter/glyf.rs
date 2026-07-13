use std::collections::BTreeSet;
use super::super::decoder::{read_u16_be, read_u32_be, read_i16_be};

const MORE_COMPONENTS:          u16 = 0x0020;
const ARG_1_AND_2_ARE_WORDS:    u16 = 0x0001;
const WE_HAVE_A_SCALE:          u16 = 0x0008;
const WE_HAVE_AN_X_AND_Y_SCALE: u16 = 0x0040;
const WE_HAVE_A_TWO_BY_TWO:     u16 = 0x0080;

pub fn parse_loca(loca: &[u8], format: i16, num_glyphs: usize) -> Vec<usize> {
    let mut offsets = vec![0usize; num_glyphs + 1];
    if format == 0 {
        for i in 0..=num_glyphs {
            if let Some(v) = read_u16_be(loca, i * 2) { offsets[i] = v as usize * 2; }
        }
    } else {
        for i in 0..=num_glyphs {
            if let Some(v) = read_u32_be(loca, i * 4) { offsets[i] = v as usize; }
        }
    }
    offsets
}

fn compound_components(glyf: &[u8], start: usize, end: usize) -> Vec<u16> {
    let mut ids = Vec::new();
    let mut pos = start + 10;
    let limit = end.min(glyf.len());
    loop {
        if pos + 4 > limit { break; }
        let flags = match read_u16_be(glyf, pos) { Some(v) => v, None => break };
        pos += 2;
        let gid = match read_u16_be(glyf, pos) { Some(v) => v, None => break };
        pos += 2;
        ids.push(gid);
        if flags & ARG_1_AND_2_ARE_WORDS  != 0 { pos += 4; } else { pos += 2; }
        if      flags & WE_HAVE_A_TWO_BY_TWO     != 0 { pos += 8; }
        else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 { pos += 4; }
        else if flags & WE_HAVE_A_SCALE           != 0 { pos += 2; }
        if flags & MORE_COMPONENTS == 0 { break; }
    }
    ids
}

pub fn active_gids(requested: &[u16], glyf: &[u8], loca: &[usize], num_glyphs: usize) -> BTreeSet<u16> {
    let mut set: BTreeSet<u16> = BTreeSet::new();
    set.insert(0);
    let mut stack: Vec<u16> = requested.to_vec();
    while let Some(gid) = stack.pop() {
        if set.contains(&gid) || gid as usize >= num_glyphs { continue; }
        set.insert(gid);
        let (s, e) = (loca[gid as usize], loca[gid as usize + 1]);
        if s < e && read_i16_be(glyf, s) == Some(-1) {
            for comp in compound_components(glyf, s, e) {
                if !set.contains(&comp) { stack.push(comp); }
            }
        }
    }
    set
}

pub fn patch_compound_gids(data: &mut [u8], start: usize, end: usize, gid_map: &[u16]) {
    let mut pos = start + 10;
    loop {
        if pos + 4 > end.min(data.len()) { break; }
        let flags   = ((data[pos] as u16) << 8) | data[pos + 1] as u16;
        pos += 2;
        let orig    = ((data[pos] as u16) << 8) | data[pos + 1] as u16;
        let compact = if (orig as usize) < gid_map.len() { gid_map[orig as usize] } else { 0 };
        data[pos]     = (compact >> 8) as u8;
        data[pos + 1] = compact as u8;
        pos += 2;
        if flags & ARG_1_AND_2_ARE_WORDS  != 0 { pos += 4; } else { pos += 2; }
        if      flags & WE_HAVE_A_TWO_BY_TWO     != 0 { pos += 8; }
        else if flags & WE_HAVE_AN_X_AND_Y_SCALE != 0 { pos += 4; }
        else if flags & WE_HAVE_A_SCALE           != 0 { pos += 2; }
        if flags & MORE_COMPONENTS == 0 { break; }
    }
}
