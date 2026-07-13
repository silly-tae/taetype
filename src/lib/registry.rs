use wasm_bindgen::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use crate::font;
use crate::font_cache::FontCache;

struct FontEntry {
    cache:  Rc<FontCache>,
    weight: u16, // 0 = variable (serves every weight via the instancer)
    opsz:   u16,
}

thread_local! {
    // multiple static weights of one family register under the same (name, style)
    // — they must coexist, not overwrite each other (Regular + Bold as separate
    // files is the standard non-variable layout)
    static FONT_REGISTRY: RefCell<HashMap<(String, String), Vec<FontEntry>>> = RefCell::new(HashMap::new());
}

fn insert_entry(name: &str, style: &str, entry: FontEntry) {
    FONT_REGISTRY.with(|reg| {
        let mut reg = reg.borrow_mut();
        let list = reg.entry((name.to_lowercase(), style.to_string())).or_default();
        match list.iter_mut().find(|e| e.weight == entry.weight) {
            Some(slot) => *slot = entry,
            None       => list.push(entry),
        }
    });
}

#[wasm_bindgen]
pub fn get_compressed_range(woff2_bytes: &[u8]) -> Result<JsValue, JsValue> {
    let (start, length) = font::decoder::get_compressed_range(woff2_bytes)
        .map_err(|e| JsValue::from_str(&e))?;
    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"start".into(),  &JsValue::from_f64(start as f64))?;
    js_sys::Reflect::set(&obj, &"length".into(), &JsValue::from_f64(length as f64))?;
    Ok(obj.into())
}

// WOFF2's own Brotli-compressed table stream, decompressed entirely in Rust —
// see woff2.rs's decompress_brotli for why this replaced the browser's
// DecompressionStream('brotli') call (unsupported in a real, confirmed case:
// Brave on Windows), which used to fail silently and leave every registered
// font (and thus the whole exported PDF) text-less.
#[wasm_bindgen]
pub fn decompress_brotli(compressed: &[u8]) -> Result<Vec<u8>, JsValue> {
    font::decoder::decompress_brotli(compressed).map_err(|e| JsValue::from_str(&e))
}

#[wasm_bindgen]
pub fn read_font_meta(woff2_bytes: &[u8], decompressed: &[u8], index: u32) -> Result<JsValue, JsValue> {
    let table_map = font::decoder::decode_woff2_tables(woff2_bytes, decompressed, index as usize)
        .map_err(|e| JsValue::from_str(&e))?;

    let style  = font::decoder::read_font_style(&table_map);
    let weight = font::decoder::read_os2_weight(&table_map);
    let is_var = font::decoder::read_wght_axis(&table_map);

    let obj = js_sys::Object::new();
    js_sys::Reflect::set(&obj, &"style".into(),      &JsValue::from_str(style))?;
    js_sys::Reflect::set(&obj, &"weight".into(),     &JsValue::from_f64(weight as f64))?;
    js_sys::Reflect::set(&obj, &"isVariable".into(), &JsValue::from_bool(is_var))?;
    Ok(obj.into())
}

#[wasm_bindgen]
pub fn register_font(
    name: &str,
    style: &str,
    weight: u16,
    opsz: u16,
    woff2_bytes: &[u8],
    decompressed: &[u8],
    index: u32,
) -> Result<(), JsValue> {
    let table_map = font::decoder::decode_woff2_tables(woff2_bytes, decompressed, index as usize)
        .map_err(|e| JsValue::from_str(&e))?;
    // don't trust the caller's weight for the variable determination — the fvar
    // table is the ground truth, and weight 0 is the "serves all weights" marker
    let is_var     = font::decoder::read_wght_axis(&table_map);
    let reg_weight = if is_var { 0 } else if weight == 0 { font::decoder::read_os2_weight(&table_map) } else { weight };
    let cache = Rc::new(FontCache::new(table_map));
    insert_entry(name, style, FontEntry { cache, weight: reg_weight, opsz });
    Ok(())
}

#[wasm_bindgen]
pub fn register_font_raw(name: &str, raw_bytes: &[u8]) -> Result<(), JsValue> {
    register_tables(name, font::decoder::extract_ttf_tables(raw_bytes)
        .map_err(|e| JsValue::from_str(&e))?)
}

// one member of a 'ttcf' collection, by index
#[wasm_bindgen]
pub fn register_font_ttc(name: &str, ttc_bytes: &[u8], index: u32) -> Result<(), JsValue> {
    register_tables(name, font::decoder::extract_ttc_tables(ttc_bytes, index as usize)
        .map_err(|e| JsValue::from_str(&e))?)
}

#[wasm_bindgen]
pub fn ttc_font_count(bytes: &[u8]) -> u32 {
    font::decoder::ttc_font_count(bytes) as u32
}

fn register_tables(name: &str, table_map: std::collections::HashMap<String, Vec<u8>>) -> Result<(), JsValue> {
    let style      = font::decoder::read_font_style(&table_map).to_string();
    let weight     = font::decoder::read_os2_weight(&table_map);
    let is_var     = font::decoder::read_wght_axis(&table_map);
    let reg_weight = if is_var { 0 } else { weight };
    let cache = Rc::new(FontCache::new(table_map));
    insert_entry(name, &style, FontEntry { cache, weight: reg_weight, opsz: 0 });
    Ok(())
}

#[wasm_bindgen]
pub fn list_registered_fonts() -> js_sys::Array {
    let arr = js_sys::Array::new();
    FONT_REGISTRY.with(|reg| {
        for (name, style) in reg.borrow().keys() {
            arr.push(&JsValue::from_str(&format!("{}:{}", name, style)));
        }
    });
    arr
}

// Nearest-weight selection: a variable entry (weight 0) serves every request via
// the instancer and always wins; otherwise the closest static weight, ties to
// the bolder file. Callers that lack a weight (cmap lookups) pass 400.
pub(crate) fn get_font_from_registry(name: &str, style: &str, req_weight: u16) -> Option<(Rc<FontCache>, u16, u16)> {
    FONT_REGISTRY.with(|reg| {
        let reg  = reg.borrow();
        let list = reg.get(&(name.to_lowercase(), style.to_string()))?;
        if let Some(var) = list.iter().find(|e| e.weight == 0) {
            return Some((Rc::clone(&var.cache), var.weight, var.opsz));
        }
        let req = if req_weight == 0 { 400 } else { req_weight } as i32;
        list.iter()
            .min_by_key(|e| ((e.weight as i32 - req).abs(), -(e.weight as i32)))
            .map(|e| (Rc::clone(&e.cache), e.weight, e.opsz))
    })
}
