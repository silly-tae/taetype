use wasm_bindgen::prelude::*;
use crate::font_cache::FontCache;
use crate::registry::get_font_from_registry;

fn char_width_pt(ch: char, fc: &FontCache, weight: u16, opsz: u16, font_size: f64) -> f64 {
    let gid = fc.glyph_id(ch as u32);
    fc.advance_width_rs(weight, opsz, gid) as f64 * font_size / 1000.0
}

// shaped width (ligatures + kerning applied) so measurement agrees with what
// the TJ emission actually draws; the per-char sum remains as the fallback for
// fonts rustybuzz can't open
pub(crate) fn string_width_pt(text: &str, fc: &FontCache, weight: u16, opsz: u16, font_size: f64) -> f64 {
    if let Some(run) = fc.shaped_run(weight, opsz, text, false) {
        return run.advances.iter().sum::<f64>() * font_size / 1000.0;
    }
    text.chars().map(|c| char_width_pt(c, fc, weight, opsz, font_size)).sum()
}

#[wasm_bindgen]
pub fn measure_string_width(
    text: &str,
    font_name: &str,
    style: &str,
    weight: u16,
    opsz: u16,
    font_size: f64,
) -> f64 {
    match get_font_from_registry(font_name, style, weight) {
        Some((fc, _, _)) => string_width_pt(text, &fc, weight, opsz, font_size),
        None => 0.0,
    }
}
