use wasm_bindgen::prelude::*;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use crate::font;
use crate::font::decoder::{read_i16_be, read_u16_be, read_u32_be};
use crate::font::subsetter::SubsetResult;

pub struct FontCache {
    pub(crate) table_map: HashMap<String, Vec<u8>>,
    instance_cache: RefCell<HashMap<(u16, u16), Vec<u8>>>,
    adv_cache: RefCell<HashMap<(u16, u16, u16), u32>>,
    shape_cache: RefCell<HashMap<(u16, u16, String, bool), Rc<crate::shape::ShapedRun>>>,
}

impl FontCache {
    pub(crate) fn new(table_map: HashMap<String, Vec<u8>>) -> Self {
        Self {
            table_map,
            instance_cache: RefCell::new(HashMap::new()),
            adv_cache: RefCell::new(HashMap::new()),
            shape_cache: RefCell::new(HashMap::new()),
        }
    }

    // document text repeats heavily (table rows, labels) — cache shaped runs,
    // bounded so a pathological document can't grow it without limit
    pub(crate) fn shaped_run(&self, weight: u16, opsz: u16, text: &str, vertical: bool) -> Option<Rc<crate::shape::ShapedRun>> {
        let key = (weight, opsz, text.to_string(), vertical);
        if let Some(hit) = self.shape_cache.borrow().get(&key) {
            return Some(Rc::clone(hit));
        }
        let run = Rc::new(crate::shape::shape_run(self, weight, opsz, text, vertical)?);
        let mut cache = self.shape_cache.borrow_mut();
        if cache.len() >= 4096 { cache.clear(); }
        cache.insert(key, Rc::clone(&run));
        Some(run)
    }

    pub fn glyph_id(&self, codepoint: u32) -> u16 {
        match self.table_map.get("cmap") {
            Some(cmap) => font::subsetter::cmap_glyph_id(cmap, codepoint),
            None => 0,
        }
    }

    pub fn font_ascender(&self) -> i32 {
        let scale = self.scale_factor();
        if let Some(hhea) = self.table_map.get("hhea") {
            if hhea.len() >= 6 {
                if let Some(v) = read_i16_be(hhea, 4) {
                    return (v as f64 * scale).round() as i32;
                }
            }
        }
        0
    }

    pub fn font_descender(&self) -> i32 {
        let scale = self.scale_factor();
        if let Some(hhea) = self.table_map.get("hhea") {
            if hhea.len() >= 8 {
                if let Some(v) = read_i16_be(hhea, 6) {
                    return (v as f64 * scale).round() as i32;
                }
            }
        }
        0
    }

    pub fn font_cap_height(&self) -> i32 {
        let scale = self.scale_factor();
        if let Some(os2) = self.table_map.get("OS/2") {
            if os2.len() >= 90 && read_u16_be(os2, 0).unwrap_or(0) >= 2 {
                let cap = read_i16_be(os2, 88).unwrap_or(0) as f64;
                if cap != 0.0 {
                    return (cap * scale).round() as i32;
                }
            }
        }
        self.font_ascender()
    }

    pub fn font_bbox(&self) -> Vec<i32> {
        let scale = self.scale_factor();
        if let Some(head) = self.table_map.get("head") {
            if head.len() >= 44 {
                let (Some(a), Some(b), Some(c), Some(d)) = (
                    read_i16_be(head, 36), read_i16_be(head, 38),
                    read_i16_be(head, 40), read_i16_be(head, 42),
                ) else { return vec![0, 0, 0, 0] };
                return vec![
                    (a as f64 * scale).round() as i32,
                    (b as f64 * scale).round() as i32,
                    (c as f64 * scale).round() as i32,
                    (d as f64 * scale).round() as i32,
                ];
            }
        }
        vec![0, 0, 0, 0]
    }

    pub fn font_flags(&self) -> u32 {
        let mut flags: u32 = 1 << 5;

        let is_fixed_pitch = self.table_map.get("post")
            .filter(|p| p.len() >= 16)
            .and_then(|p| read_u32_be(p, 12))
            .map_or(false, |v| v != 0);
        if is_fixed_pitch { flags |= 1; }

        let family_class = self.table_map.get("OS/2")
            .filter(|o| o.len() >= 32)
            .and_then(|o| read_u16_be(o, 30))
            .unwrap_or(0);
        let class_id = (family_class >> 8) as u8;
        let is_serif  = matches!(class_id, 1 | 2 | 3 | 4 | 5 | 7);
        let is_script = class_id == 10;
        if is_serif  { flags |= 1 << 1; }
        if is_script { flags |= 1 << 3; }

        if self.font_italic_angle() != 0.0 { flags |= 1 << 6; }

        flags
    }

    pub fn font_italic_angle(&self) -> f64 {
        let post = match self.table_map.get("post") {
            Some(p) if p.len() >= 16 => p,
            _ => return 0.0,
        };
        let raw = read_u32_be(post, 4).unwrap_or(0) as i32;
        raw as f64 / 65536.0
    }

    pub fn font_upm(&self) -> u16 {
        self.table_map.get("head")
            .filter(|h| h.len() >= 20)
            .and_then(|h| read_u16_be(h, 18))
            .filter(|&v| v > 0)
            .unwrap_or(2048)
    }
}

impl FontCache {
    fn scale_factor(&self) -> f64 {
        1000.0 / self.font_upm() as f64
    }

    pub(crate) fn get_or_instance(&self, weight: u16, opsz: u16) -> Result<Vec<u8>, JsValue> {
        let key = (weight, opsz);
        {
            if let Some(ttf) = self.instance_cache.borrow().get(&key) {
                return Ok(ttf.clone());
            }
        }
        let ttf = font::instancer::instance_font_from_map(&self.table_map, weight, opsz)
            .unwrap_or_else(|_| font::decoder::build_ttf(&self.table_map));
        self.instance_cache.borrow_mut().insert(key, ttf.clone());
        Ok(ttf)
    }

    pub(crate) fn advance_width_rs(&self, weight: u16, opsz: u16, gid: u16) -> u32 {
        let key = (weight, opsz, gid);
        if let Some(&cached) = self.adv_cache.borrow().get(&key) {
            return cached;
        }
        let adv = match self.get_or_instance(weight, opsz) {
            Ok(ttf) => font::subsetter::ttf_advance_width(&ttf, gid),
            Err(_)  => 1000,
        };
        self.adv_cache.borrow_mut().insert(key, adv);
        adv
    }

    pub(crate) fn subset_font_rs(&self, weight: u16, opsz: u16, gids: &[u16]) -> Result<SubsetResult, String> {
        if self.table_map.contains_key("CFF2") {
            return Err("CFF2 (variable CFF) fonts are not supported for embedding".into());
        }
        if let Some(cff) = self.table_map.get("CFF ") {
            return font::subsetter::subset_cff(cff, gids);
        }
        let ttf = self.get_or_instance(weight, opsz)
            .map_err(|e| e.as_string().unwrap_or_default())?;
        font::subsetter::subset_ttf(&ttf, gids)
    }

}
