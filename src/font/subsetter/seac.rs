use super::super::decoder::read_u16_be;

// Type2 charstrings may end in the deprecated seac form — `adx ady bchar achar
// endchar` — building an accented glyph from two components addressed by
// StandardEncoding CODE. Blanking a subset without chasing those references
// drops the base or accent from every composed glyph that survives.

// StandardEncoding code → SID (CFF spec Appendix B). Codes 32..=126 map to
// SIDs 1..=95; the high range is sparse.
fn standard_encoding_sid(code: u8) -> u16 {
    match code {
        32..=126 => (code - 31) as u16,
        161 => 96, 162 => 97, 163 => 98, 164 => 99, 165 => 100, 166 => 101,
        167 => 102, 168 => 103, 169 => 104, 170 => 105, 171 => 106, 172 => 107,
        173 => 108, 174 => 109, 175 => 110, 177 => 111, 178 => 112, 179 => 113,
        180 => 114, 182 => 115, 183 => 116, 184 => 117, 185 => 118, 186 => 119,
        187 => 120, 188 => 121, 189 => 122, 191 => 123, 193 => 124, 194 => 125,
        195 => 126, 196 => 127, 197 => 128, 198 => 129, 199 => 130, 200 => 131,
        202 => 132, 203 => 133, 205 => 134, 206 => 135, 207 => 136, 208 => 137,
        225 => 138, 227 => 139, 232 => 140, 233 => 141, 234 => 142, 235 => 143,
        241 => 144, 245 => 145, 248 => 146, 249 => 147, 250 => 148, 251 => 149,
        _ => 0,
    }
}

// Scan a charstring for the seac-endchar form. Conservative: only pure
// number-run + endchar qualifies — any other operator (hints, subr calls,
// moveto) means this is a normal outline and the scan bails. That matches how
// real fonts use seac (the four args and endchar are the whole program).
pub(super) fn seac_components(cs: &[u8]) -> Option<(u8, u8)> {
    let mut stack: Vec<i32> = Vec::new();
    let mut pos = 0usize;
    while pos < cs.len() {
        let b0 = cs[pos];
        match b0 {
            32..=246 => { stack.push(b0 as i32 - 139); pos += 1; }
            247..=250 => {
                let b1 = *cs.get(pos + 1)? as i32;
                stack.push((b0 as i32 - 247) * 256 + b1 + 108);
                pos += 2;
            }
            251..=254 => {
                let b1 = *cs.get(pos + 1)? as i32;
                stack.push(-(b0 as i32 - 251) * 256 - b1 - 108);
                pos += 2;
            }
            28 => {
                let v = read_u16_be(cs, pos + 1)? as i16 as i32;
                stack.push(v);
                pos += 3;
            }
            255 => {
                // 16.16 fixed — integer part only (bchar/achar are small ints anyway)
                let v = read_u16_be(cs, pos + 1)? as i16 as i32;
                stack.push(v);
                pos += 5;
            }
            14 => {
                // endchar: seac form has adx ady bchar achar as the LAST four
                // operands (an optional width may precede them)
                if stack.len() >= 4 {
                    let achar = stack[stack.len() - 1];
                    let bchar = stack[stack.len() - 2];
                    if (0..=255).contains(&bchar) && (0..=255).contains(&achar) {
                        return Some((bchar as u8, achar as u8));
                    }
                }
                return None;
            }
            _ => return None, // any other operator: not a seac charstring
        }
    }
    None
}

// gid whose charset SID equals `target` — charset formats 0/1/2; a missing
// charset means the ISOAdobe identity (gid == SID).
pub(super) fn sid_to_gid(cff: &[u8], charset_off: Option<usize>, n_glyphs: usize, target: u16) -> Option<u16> {
    if target == 0 { return Some(0) }
    let off = match charset_off {
        None => return if (target as usize) < n_glyphs { Some(target) } else { None },
        Some(o) => o,
    };
    let format = *cff.get(off)?;
    let mut pos = off + 1;
    match format {
        0 => {
            for gid in 1..n_glyphs {
                if read_u16_be(cff, pos)? == target { return Some(gid as u16); }
                pos += 2;
            }
        }
        1 | 2 => {
            let mut gid = 1usize;
            while gid < n_glyphs {
                let first  = read_u16_be(cff, pos)?;
                let n_left = if format == 1 {
                    let v = *cff.get(pos + 2)? as usize; pos += 3; v
                } else {
                    let v = read_u16_be(cff, pos + 2)? as usize; pos += 4; v
                };
                let span = (n_left + 1).min(n_glyphs - gid);
                if target >= first && ((target - first) as usize) < span {
                    return Some((gid + (target - first) as usize) as u16);
                }
                gid += span;
            }
        }
        _ => {}
    }
    None
}

pub(super) fn seac_component_gids(
    charstrings: &[Vec<u8>],
    active: &std::collections::BTreeSet<u16>,
    cff: &[u8],
    charset_off: Option<usize>,
) -> Vec<u16> {
    let n_glyphs = charstrings.len();
    let mut found = Vec::new();
    for &gid in active {
        let cs = match charstrings.get(gid as usize) { Some(c) => c, None => continue };
        if let Some((bchar, achar)) = seac_components(cs) {
            for code in [bchar, achar] {
                let sid = standard_encoding_sid(code);
                if let Some(comp) = sid_to_gid(cff, charset_off, n_glyphs, sid) {
                    found.push(comp);
                }
            }
        }
    }
    found
}
