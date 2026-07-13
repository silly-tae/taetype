use wasm_bindgen::prelude::*;
use crate::font_cache::FontCache;
use crate::registry::get_font_from_registry;

// Full OpenType shaping via rustybuzz: GSUB ligatures, GPOS kerning, complex
// scripts. Shaping always runs on the INSTANCED ttf (FontCache::get_or_instance)
// so variable fonts shape at the requested weight, and advances are returned in
// 1000-upm units to match the rest of the engine.

pub(crate) struct ShapedRun {
    pub glyphs:   Vec<u16>,
    pub advances: Vec<f64>,
    // char index (not byte offset) of the cluster each glyph belongs to
    pub clusters: Vec<u32>,
}

pub(crate) fn shape_run(fc: &FontCache, weight: u16, opsz: u16, text: &str, vertical: bool) -> Option<ShapedRun> {
    let ttf = fc.get_or_instance(weight, opsz).ok()?;
    let face = rustybuzz::Face::from_slice(&ttf, 0)?;
    let upm = face.units_per_em() as f64;
    if upm <= 0.0 { return None; }
    let scale = 1000.0 / upm;

    let mut buf = rustybuzz::UnicodeBuffer::new();
    buf.push_str(text);
    // vertical writing mode: explicitly setting the direction BEFORE shaping
    // (guess_segment_properties only fills in a direction that's still
    // Invalid, so this is preserved) makes rustybuzz select the font's
    // 'vert'/'vrt2' GSUB features where present — some glyphs (notably
    // certain punctuation) have a genuinely different presentation form when
    // set vertically, not just a rotated rendering of the horizontal glyph
    if vertical { buf.set_direction(rustybuzz::Direction::TopToBottom); }
    let out = rustybuzz::shape(&face, &[], buf);

    // rustybuzz clusters are byte offsets into the utf-8 input — the TS side
    // works in characters, so translate through char_indices
    let mut byte_to_char = std::collections::HashMap::new();
    for (ci, (bi, _)) in text.char_indices().enumerate() {
        byte_to_char.insert(bi as u32, ci as u32);
    }

    let infos = out.glyph_infos();
    let poss  = out.glyph_positions();
    let mut glyphs   = Vec::with_capacity(infos.len());
    let mut advances = Vec::with_capacity(infos.len());
    let mut clusters = Vec::with_capacity(infos.len());
    for (info, pos) in infos.iter().zip(poss.iter()) {
        if info.glyph_id > 0xFFFF { return None; }
        glyphs.push(info.glyph_id as u16);
        // HarfBuzz reports the shaping-direction advance in a DIFFERENT field
        // depending on buffer direction: x_advance for horizontal, y_advance
        // for vertical — using x_advance unconditionally would silently
        // return ~0 for every vertical-shaped glyph. y_advance is negative
        // for TopToBottom (HarfBuzz's own coordinate space has +y up, so
        // moving down the page is negative) — negated to match the positive
        // "downward magnitude" convention get_vertical_advance's vmtx-sourced
        // values already use, so the two remain directly comparable.
        let raw = if vertical { -pos.y_advance } else { pos.x_advance };
        advances.push(raw as f64 * scale);
        clusters.push(*byte_to_char.get(&info.cluster).unwrap_or(&0));
    }
    Some(ShapedRun { glyphs, advances, clusters })
}

// { glyphs: Uint16Array, advances: Float64Array, clusters: Uint32Array } or
// null when the font is unregistered / unshapeable — callers fall back to the
// per-character cmap path
#[wasm_bindgen]
pub fn shape_text(text: &str, font_name: &str, style: &str, weight: u16, opsz: u16, vertical: bool) -> JsValue {
    let (fc, _, _) = match get_font_from_registry(font_name, style, weight) {
        None => return JsValue::null(),
        Some(x) => x,
    };
    let run = match fc.shaped_run(weight, opsz, text, vertical) {
        None => return JsValue::null(),
        Some(r) => r,
    };
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"glyphs".into(),   &js_sys::Uint16Array::from(run.glyphs.as_slice())).unwrap();
    js_sys::Reflect::set(&obj, &"advances".into(), &js_sys::Float64Array::from(run.advances.as_slice())).unwrap();
    js_sys::Reflect::set(&obj, &"clusters".into(), &js_sys::Uint32Array::from(run.clusters.as_slice())).unwrap();
    obj.into()
}
