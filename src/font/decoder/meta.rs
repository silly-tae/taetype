use std::collections::HashMap;
use super::io::{read_u16_be, read_u32_be};

pub fn read_font_family_name(table_map: &HashMap<String, Vec<u8>>) -> Option<String> {
    let data = table_map.get("name")?;
    if data.len() < 6 { return None; }

    let count          = read_u16_be(data, 2)? as usize;
    let storage_offset = read_u16_be(data, 4)? as usize;

    if data.len() < 6 + count * 12 { return None; }

    let mut best_score:    i32   = -1;
    let mut best_offset:   usize = 0;
    let mut best_length:   usize = 0;
    let mut best_platform: u16   = 0;

    for i in 0..count {
        let rec = 6 + i * 12;
        // The length check above guarantees rec+12 <= data.len() for every i < count,
        // so these reads can't fail under the current invariant — but skip the record
        // gracefully instead of unwrapping, so a future change to that invariant can't panic.
        let (Some(platform_id), Some(encoding_id), Some(language_id), Some(name_id), Some(length), Some(offset)) = (
            read_u16_be(data, rec),
            read_u16_be(data, rec + 2),
            read_u16_be(data, rec + 4),
            read_u16_be(data, rec + 6),
            read_u16_be(data, rec + 8),
            read_u16_be(data, rec + 10),
        ) else { continue };
        let length = length as usize;
        let offset = offset as usize;

        let score: i32 = match (platform_id, encoding_id, language_id, name_id) {
            (3, 1, 0x0409, 16) => 4,
            (3, 1, 0x0409, 1)  => 3,
            (1, _, _, 16)      => 2,
            (1, _, _, 1)       => 1,
            _ => continue,
        };

        if score > best_score {
            best_score    = score;
            best_offset   = offset;
            best_length   = length;
            best_platform = platform_id;
        }
    }

    if best_score < 0 { return None; }

    let abs = storage_offset + best_offset;
    if abs + best_length > data.len() { return None; }
    let raw = &data[abs..abs + best_length];

    let name: String = if best_platform == 3 {
        let chars: Vec<u16> = raw.chunks_exact(2)
            .map(|b| ((b[0] as u16) << 8) | b[1] as u16)
            .collect();
        String::from_utf16_lossy(&chars)
    } else {
        raw.iter().map(|&b| mac_roman_char(b)).collect()
    };

    let trimmed = name.trim_matches('\0').trim().to_string();
    if trimmed.is_empty() { None } else { Some(trimmed) }
}

// platform-1 name records are MacRoman; 0x00-0x7F is ASCII, the high half needs
// a real mapping (a Latin-1 cast garbles every accented family name)
fn mac_roman_char(b: u8) -> char {
    if b < 0x80 { return b as char; }
    const HIGH: [char; 128] = [
        'Ä','Å','Ç','É','Ñ','Ö','Ü','á','à','â','ä','ã','å','ç','é','è',
        'ê','ë','í','ì','î','ï','ñ','ó','ò','ô','ö','õ','ú','ù','û','ü',
        '†','°','¢','£','§','•','¶','ß','®','©','™','´','¨','≠','Æ','Ø',
        '∞','±','≤','≥','¥','µ','∂','∑','∏','π','∫','ª','º','Ω','æ','ø',
        '¿','¡','¬','√','ƒ','≈','∆','«','»','…','\u{A0}','À','Ã','Õ','Œ','œ',
        '–','—','“','”','‘','’','÷','◊','ÿ','Ÿ','⁄','€','‹','›','ﬁ','ﬂ',
        '‡','·','‚','„','‰','Â','Ê','Á','Ë','È','Í','Î','Ï','Ì','Ó','Ô',
        '\u{F8FF}','Ò','Ú','Û','Ù','ı','ˆ','˜','¯','˘','˙','˚','¸','˝','˛','ˇ',
    ];
    HIGH[(b - 0x80) as usize]
}

pub fn read_os2_weight(table_map: &HashMap<String, Vec<u8>>) -> u16 {
    table_map.get("OS/2")
        .filter(|d| d.len() >= 6)
        .and_then(|d| read_u16_be(d, 4))
        .unwrap_or(400)
}

pub fn read_font_style(table_map: &HashMap<String, Vec<u8>>) -> &'static str {
    let italic_os2 = table_map.get("OS/2")
        .filter(|d| d.len() >= 64)
        .and_then(|d| read_u16_be(d, 62))
        .map_or(false, |v| v & 0x0001 != 0);

    let italic_head = table_map.get("head")
        .filter(|d| d.len() >= 46)
        .and_then(|d| read_u16_be(d, 44))
        .map_or(false, |v| v & 0x0002 != 0);

    if italic_os2 || italic_head { "italic" } else { "normal" }
}

pub fn read_wght_axis(table_map: &HashMap<String, Vec<u8>>) -> bool {
    let fvar = match table_map.get("fvar") {
        Some(f) if f.len() >= 16 => f,
        _ => return false,
    };

    let axes_offset = match read_u16_be(fvar, 4)  { Some(v) => v as usize, None => return false };
    let axis_count  = match read_u16_be(fvar, 8)  { Some(v) => v as usize, None => return false };
    let axis_size   = match read_u16_be(fvar, 10) { Some(v) => v as usize, None => return false };

    const WGHT: u32 = 0x77676874;

    for i in 0..axis_count {
        let off = axes_offset + i * axis_size;
        if off + 16 > fvar.len() { break; }
        if read_u32_be(fvar, off) == Some(WGHT) { return true; }
    }

    false
}
