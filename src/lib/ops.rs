use wasm_bindgen::prelude::*;
use crate::font;
use crate::registry::get_font_from_registry;

// weight selects among multiple static files registered under one name — glyph
// IDs differ between files, so this MUST pick the same font the subset embeds
#[wasm_bindgen]
pub fn get_glyph_ids(text: &str, font_name: &str, style: &str, weight: u16) -> js_sys::Uint16Array {
    match get_font_from_registry(font_name, style, weight) {
        None => js_sys::Uint16Array::new_with_length(0),
        Some((fc, _, _)) => {
            let ids: Vec<u16> = text.chars().map(|c| fc.glyph_id(c as u32)).collect();
            js_sys::Uint16Array::from(ids.as_slice())
        }
    }
}

#[wasm_bindgen]
pub fn get_advance_widths(
    font_name: &str,
    style: &str,
    weight: u16,
    opsz: u16,
    glyph_ids: &[u16],
) -> js_sys::Float64Array {
    match get_font_from_registry(font_name, style, weight) {
        None => js_sys::Float64Array::new_with_length(0),
        Some((fc, _, _)) => {
            // advance_width_rs already returns widths normalized to 1000 units/em
            // (see ttf_advance_width) — do not rescale by font_upm() again here.
            let widths: Vec<f64> = glyph_ids.iter()
                .map(|&gid| fc.advance_width_rs(weight, opsz, gid) as f64)
                .collect();
            js_sys::Float64Array::from(widths.as_slice())
        }
    }
}

// { png, ppem, originX, originY } for a color-bitmap glyph (sbix or CBDT),
// or null — renderer emoji wiring consumes this in the TS phase
#[wasm_bindgen]
pub fn get_glyph_bitmap(font_name: &str, style: &str, gid: u16, target_ppem: u16) -> JsValue {
    let (fc, _, _) = match get_font_from_registry(font_name, style, 400) {
        None => return JsValue::null(),
        Some(x) => x,
    };
    match font::color::glyph_bitmap(&fc.table_map, gid, target_ppem) {
        None => JsValue::null(),
        Some(bm) => {
            let obj = js_sys::Object::new();
            js_sys::Reflect::set(&obj, &"png".into(), &js_sys::Uint8Array::from(bm.png.as_slice())).unwrap();
            js_sys::Reflect::set(&obj, &"ppem".into(),    &JsValue::from_f64(bm.ppem as f64)).unwrap();
            js_sys::Reflect::set(&obj, &"originX".into(), &JsValue::from_f64(bm.origin_x as f64)).unwrap();
            js_sys::Reflect::set(&obj, &"originY".into(), &JsValue::from_f64(bm.origin_y as f64)).unwrap();
            obj.into()
        }
    }
}

// COLR v0 layers, flattened [gid, r, g, b, a, isForeground, ...] (6 per layer);
// empty array when the glyph has no color layers
#[wasm_bindgen]
pub fn get_colr_layers(font_name: &str, style: &str, gid: u16) -> js_sys::Uint32Array {
    let (fc, _, _) = match get_font_from_registry(font_name, style, 400) {
        None => return js_sys::Uint32Array::new_with_length(0),
        Some(x) => x,
    };
    match font::color::colr_layers(&fc.table_map, gid) {
        None => js_sys::Uint32Array::new_with_length(0),
        Some(layers) => {
            let flat: Vec<u32> = layers.iter()
                .flat_map(|&(g, r, gc, b, a, fg)| [g as u32, r as u32, gc as u32, b as u32, a as u32, fg as u32])
                .collect();
            js_sys::Uint32Array::from(flat.as_slice())
        }
    }
}

// does this registered font have a real glyph (not .notdef) for the given
// codepoint? Weight is irrelevant to cmap coverage within one family (every
// static weight of a family shares the same character set in practice), so
// this checks the same weight-400 "representative" file get_glyph_bitmap/
// get_colr_layers already use, not the caller's requested weight — per-
// character fallback resolution (renderer/html/fonts.ts) walks registered
// families with this to find one that actually covers a missing codepoint
#[wasm_bindgen]
pub fn font_has_glyph(font_name: &str, style: &str, codepoint: u32) -> bool {
    match get_font_from_registry(font_name, style, 400) {
        None => false,
        Some((fc, _, _)) => fc.glyph_id(codepoint) != 0,
    }
}

// vertical advance (vmtx), 1000-upm units — for future writing-mode support;
// 0 when the font has no vertical metrics
#[wasm_bindgen]
pub fn get_vertical_advance(font_name: &str, style: &str, weight: u16, opsz: u16, gid: u16) -> u32 {
    match get_font_from_registry(font_name, style, weight) {
        None => 0,
        Some((fc, _, _)) => match fc.get_or_instance(weight, opsz) {
            Ok(ttf) => font::subsetter::ttf_advance_height(&ttf, gid),
            Err(_)  => 0,
        },
    }
}

#[wasm_bindgen]
pub fn subset_font_full(
    font_name: &str,
    style: &str,
    weight: u16,
    opsz: u16,
    glyph_ids: &[u16],
) -> JsValue {
    let (fc, _, _) = match get_font_from_registry(font_name, style, weight) {
        None => return JsValue::null(),
        Some(x) => x,
    };

    let obj    = js_sys::Object::new();
    let is_cff = fc.table_map.contains_key("CFF ");

    match fc.subset_font_rs(weight, opsz, glyph_ids) {
        Ok(result) => {
            let font_bytes = js_sys::Uint8Array::from(result.ttf.as_slice());
            js_sys::Reflect::set(&obj, &"fontBytes".into(), &font_bytes).unwrap();
            if result.gid_map.is_empty() {
                js_sys::Reflect::set(&obj, &"glyphMap".into(), &JsValue::null()).unwrap();
            } else {
                let glyph_map = js_sys::Uint16Array::from(result.gid_map.as_slice());
                js_sys::Reflect::set(&obj, &"glyphMap".into(), &glyph_map).unwrap();
            }
            js_sys::Reflect::set(&obj, &"isCff".into(), &JsValue::from_bool(is_cff)).unwrap();
        }
        Err(_) => {
            if let Some(cff_data) = fc.table_map.get("CFF ") {
                let font_bytes = js_sys::Uint8Array::from(cff_data.as_slice());
                js_sys::Reflect::set(&obj, &"fontBytes".into(), &font_bytes).unwrap();
                js_sys::Reflect::set(&obj, &"isCff".into(), &JsValue::from_bool(true)).unwrap();
            } else {
                js_sys::Reflect::set(&obj, &"fontBytes".into(), &JsValue::null()).unwrap();
                js_sys::Reflect::set(&obj, &"isCff".into(), &JsValue::from_bool(false)).unwrap();
            }
            js_sys::Reflect::set(&obj, &"glyphMap".into(), &JsValue::null()).unwrap();
        }
    }

    js_sys::Reflect::set(&obj, &"ascender".into(),    &JsValue::from_f64(fc.font_ascender() as f64)).unwrap();
    js_sys::Reflect::set(&obj, &"descender".into(),   &JsValue::from_f64(fc.font_descender() as f64)).unwrap();
    js_sys::Reflect::set(&obj, &"capHeight".into(),   &JsValue::from_f64(fc.font_cap_height() as f64)).unwrap();

    let bbox = fc.font_bbox();
    let bbox_arr = js_sys::Array::new();
    for v in &bbox { bbox_arr.push(&JsValue::from_f64(*v as f64)); }
    js_sys::Reflect::set(&obj, &"bbox".into(), &bbox_arr).unwrap();

    js_sys::Reflect::set(&obj, &"flags".into(),       &JsValue::from_f64(fc.font_flags() as f64)).unwrap();
    js_sys::Reflect::set(&obj, &"italicAngle".into(), &JsValue::from_f64(fc.font_italic_angle())).unwrap();

    let name_str = font::decoder::read_font_family_name(&fc.table_map)
        .unwrap_or_else(|| font_name.to_string());
    js_sys::Reflect::set(&obj, &"fontName".into(),  &JsValue::from_str(&name_str)).unwrap();

    obj.into()
}
