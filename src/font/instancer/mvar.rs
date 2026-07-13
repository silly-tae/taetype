use std::collections::HashMap;
use super::super::decoder::{read_u16_be, read_u32_be, read_i16_be, write_u16_be, write_i16_be};
use super::ivs::{parse_item_variation_store, compute_ivs_delta};

// MVAR varies font-wide metrics with the axes. Each value tag maps to a fixed
// field offset in hhea/OS/2/post. hasc/hdsc/hlgp officially target the OS/2
// typo fields, but the same design metric lives in hhea — both are updated so
// font_ascender()/font_descender() (which read hhea) reflect the instance.
pub fn apply_mvar(
    table_map: &HashMap<String, Vec<u8>>,
    hhea:      &mut Vec<u8>,
    os2:       &mut Vec<u8>,
    post:      &mut Vec<u8>,
    location:  &[f64],
) -> Result<(), String> {
    let mvar = match table_map.get("MVAR") { Some(m) => m, None => return Ok(()) };

    let value_record_size  = read_u16_be(mvar, 6).ok_or("MVAR: header truncated")? as usize;
    let value_record_count = read_u16_be(mvar, 8).ok_or("MVAR: header truncated")? as usize;
    let ivs_off            = read_u16_be(mvar, 10).ok_or("MVAR: header truncated")? as usize;
    if ivs_off == 0 || value_record_count == 0 { return Ok(()); }
    if value_record_size < 8 { return Err("MVAR: valueRecordSize too small".into()); }

    let store = parse_item_variation_store(mvar, ivs_off)?;

    let records_base = 12usize;
    for r in 0..value_record_count {
        let rec = records_base + r * value_record_size;
        let tag   = read_u32_be(mvar, rec).ok_or("MVAR: value record truncated")?;
        let outer = read_u16_be(mvar, rec + 4).ok_or("MVAR: value record truncated")? as usize;
        let inner = read_u16_be(mvar, rec + 6).ok_or("MVAR: value record truncated")? as usize;
        let delta = compute_ivs_delta(&store, outer, inner, location);
        if delta == 0 { continue; }

        // (table, offset, unsigned) per the MVAR value tag registry
        let target: Option<(&mut Vec<u8>, usize, bool)> = match &tag.to_be_bytes() {
            b"hasc" => Some((os2,  68, false)), // sTypoAscender (+ hhea below)
            b"hdsc" => Some((os2,  70, false)), // sTypoDescender
            b"hlgp" => Some((os2,  72, false)), // sTypoLineGap
            b"hcla" => Some((os2,  74, true)),  // usWinAscent
            b"hcld" => Some((os2,  76, true)),  // usWinDescent
            b"xhgt" => Some((os2,  86, false)), // sxHeight
            b"cpht" => Some((os2,  88, false)), // sCapHeight
            b"sbxs" => Some((os2,  10, false)),
            b"sbys" => Some((os2,  12, false)),
            b"sbxo" => Some((os2,  14, false)),
            b"sbyo" => Some((os2,  16, false)),
            b"spxs" => Some((os2,  18, false)),
            b"spys" => Some((os2,  20, false)),
            b"spxo" => Some((os2,  22, false)),
            b"spyo" => Some((os2,  24, false)),
            b"strs" => Some((os2,  26, false)), // yStrikeoutSize
            b"stro" => Some((os2,  28, false)), // yStrikeoutPosition
            b"undo" => Some((post,  8, false)), // underlinePosition
            b"unds" => Some((post, 10, false)), // underlineThickness
            _       => None,
        };
        if let Some((buf, off, unsigned)) = target {
            bump(buf, off, delta, unsigned);
        }
        match &tag.to_be_bytes() {
            b"hasc" => bump(hhea, 4, delta, false),
            b"hdsc" => bump(hhea, 6, delta, false),
            b"hlgp" => bump(hhea, 8, delta, false),
            _ => {}
        }
    }
    Ok(())
}

fn bump(buf: &mut [u8], off: usize, delta: i32, unsigned: bool) {
    if off + 2 > buf.len() { return; }
    if unsigned {
        let v = read_u16_be(buf, off).unwrap_or(0) as i32;
        write_u16_be(buf, off, (v + delta).clamp(0, 65535) as u16);
    } else {
        let v = read_i16_be(buf, off).unwrap_or(0) as i32;
        write_i16_be(buf, off, (v + delta).clamp(-32768, 32767) as i16);
    }
}
