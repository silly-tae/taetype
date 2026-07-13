use super::super::decoder::{read_u16_be, read_u32_be, read_i16_be};

// ItemVariationStore + DeltaSetIndexMap: the shared delta machinery behind
// HVAR, MVAR, and avar format 2 — parsed once here, applied per consumer.

pub(crate) struct ItemVariationStore {
    pub regions:    Vec<Vec<RegionAxis>>,
    pub ivd_data:   Vec<IVD>,
    pub axis_count: usize,
}

pub(crate) struct RegionAxis { pub start: f64, pub peak: f64, pub end: f64 }

pub(crate) struct IVD {
    pub region_indices: Vec<usize>,
    pub delta_sets:     Vec<Vec<i32>>,
}

pub(crate) fn parse_item_variation_store(buf: &[u8], base: usize) -> Result<ItemVariationStore, String> {
    let region_list_off = read_u32_be(buf, base + 2).ok_or("IVS: header truncated")? as usize;
    let ivd_count       = read_u16_be(buf, base + 6).ok_or("IVS: header truncated")? as usize;

    let region_list_base = base + region_list_off;
    let axis_count       = read_u16_be(buf, region_list_base).ok_or("IVS: region list truncated")? as usize;
    let region_count     = read_u16_be(buf, region_list_base + 2).ok_or("IVS: region list truncated")? as usize;

    let mut regions: Vec<Vec<RegionAxis>> = Vec::with_capacity(region_count);
    for i in 0..region_count {
        let r_off = region_list_base + 4 + i * axis_count * 6;
        let mut axes = Vec::with_capacity(axis_count);
        for j in 0..axis_count {
            let start = read_i16_be(buf, r_off + j * 6).ok_or("IVS: region axis truncated")?;
            let peak  = read_i16_be(buf, r_off + j * 6 + 2).ok_or("IVS: region axis truncated")?;
            let end   = read_i16_be(buf, r_off + j * 6 + 4).ok_or("IVS: region axis truncated")?;
            axes.push(RegionAxis {
                start: start as f64 / 16384.0,
                peak:  peak  as f64 / 16384.0,
                end:   end   as f64 / 16384.0,
            });
        }
        regions.push(axes);
    }

    let mut ivd_offsets: Vec<usize> = Vec::with_capacity(ivd_count);
    for i in 0..ivd_count {
        let off = read_u32_be(buf, base + 8 + i * 4).ok_or("IVS: IVD offset table truncated")?;
        ivd_offsets.push(off as usize);
    }

    let mut ivd_data: Vec<IVD> = Vec::with_capacity(ivd_count);
    for off in ivd_offsets {
        let ivd      = base + off;
        let items    = read_u16_be(buf, ivd).ok_or("IVS: IVD header truncated")? as usize;
        let word_cnt = read_u16_be(buf, ivd + 2).ok_or("IVS: IVD header truncated")? as usize;
        let reg_cnt  = read_u16_be(buf, ivd + 4).ok_or("IVS: IVD header truncated")? as usize;
        let mut reg_idxs = Vec::with_capacity(reg_cnt);
        for j in 0..reg_cnt {
            let idx = read_u16_be(buf, ivd + 6 + j * 2).ok_or("IVS: IVD region index truncated")?;
            reg_idxs.push(idx as usize);
        }
        let long_words  = (word_cnt & 0x8000) != 0;
        let wc          = word_cnt & 0x7FFF;
        if wc > reg_cnt {
            return Err("IVS: IVD wordDeltaCount exceeds regionIndexCount".into());
        }
        let wide_size   = if long_words { 4 } else { 2 };
        let narrow_size = if long_words { 2 } else { 1 };
        let row_bytes   = wc * wide_size + (reg_cnt - wc) * narrow_size;
        let data_start  = ivd + 6 + reg_cnt * 2;

        let mut delta_sets: Vec<Vec<i32>> = Vec::with_capacity(items);
        for r in 0..items {
            let row_off = data_start + r * row_bytes;
            let mut row: Vec<i32> = Vec::with_capacity(reg_cnt);
            for c in 0..wc {
                let v = if long_words {
                    read_u32_be(buf, row_off + c * wide_size).ok_or("IVS: delta row truncated")? as i32
                } else {
                    read_i16_be(buf, row_off + c * wide_size).ok_or("IVS: delta row truncated")? as i32
                };
                row.push(v);
            }
            for c in wc..reg_cnt {
                let byte_off = row_off + wc * wide_size + (c - wc) * narrow_size;
                let v = if long_words {
                    read_i16_be(buf, byte_off).ok_or("IVS: delta row truncated")? as i32
                } else {
                    *buf.get(byte_off).ok_or("IVS: delta row truncated")? as i8 as i32
                };
                row.push(v);
            }
            delta_sets.push(row);
        }
        ivd_data.push(IVD { region_indices: reg_idxs, delta_sets });
    }

    Ok(ItemVariationStore { regions, ivd_data, axis_count })
}

pub(crate) fn parse_delta_set_index_map(buf: &[u8], base: usize) -> Result<impl Fn(usize) -> (usize, usize), String> {
    let fmt       = *buf.get(base).ok_or("IVS: delta set index map truncated")?;
    let entry_fmt = *buf.get(base + 1).ok_or("IVS: delta set index map truncated")?;
    let map_count: usize = if fmt == 0 {
        read_u16_be(buf, base + 2).ok_or("IVS: delta set index map truncated")? as usize
    } else {
        read_u32_be(buf, base + 2).ok_or("IVS: delta set index map truncated")? as usize
    };
    let inner_bits = (entry_fmt & 0x0F) as usize + 1;
    let entry_size = ((entry_fmt >> 4) & 0x3) as usize + 1;
    let inner_mask = (1usize << inner_bits) - 1;
    let data_off   = if fmt == 0 { 4 } else { 6 };

    // validate the claimed count against the actual data before allocating —
    // a malformed u32 count would otherwise drive a multi-GB with_capacity
    let need = map_count.checked_mul(entry_size)
        .and_then(|n| n.checked_add(base + data_off))
        .ok_or("IVS: delta set index map too large")?;
    if need > buf.len() { return Err("IVS: delta set index map truncated".into()); }

    let mut map: Vec<(usize, usize)> = Vec::with_capacity(map_count);
    for i in 0..map_count {
        let mut val = 0usize;
        for b in 0..entry_size {
            let byte = *buf.get(base + data_off + i * entry_size + b)
                .ok_or("IVS: delta set index map entry truncated")?;
            val = (val << 8) | byte as usize;
        }
        map.push((val >> inner_bits, val & inner_mask));
    }

    Ok(move |gid: usize| {
        if gid < map.len() { map[gid] } else { *map.last().unwrap_or(&(0, 0)) }
    })
}

pub(crate) fn compute_ivs_delta(store: &ItemVariationStore, outer_idx: usize, inner_idx: usize, location: &[f64]) -> i32 {
    compute_ivs_delta_f64(store, outer_idx, inner_idx, location).round() as i32
}

pub(crate) fn compute_ivs_delta_f64(store: &ItemVariationStore, outer_idx: usize, inner_idx: usize, location: &[f64]) -> f64 {
    let ivd       = match store.ivd_data.get(outer_idx) { Some(v) => v, None => return 0.0 };
    let delta_set = match ivd.delta_sets.get(inner_idx) { Some(v) => v, None => return 0.0 };
    let mut delta = 0.0f64;
    for (i, &reg_idx) in ivd.region_indices.iter().enumerate() {
        let region = match store.regions.get(reg_idx) { Some(r) => r, None => continue };
        let mut scalar = 1.0f64;
        for j in 0..store.axis_count.min(region.len()) {
            let RegionAxis { start, peak, end } = region[j];
            let loc = *location.get(j).unwrap_or(&0.0);
            if peak == 0.0 { continue; }
            if loc == peak { continue; }
            if loc < start || loc > end { scalar = 0.0; break; }
            scalar *= if loc < peak {
                (loc - start) / (peak - start)
            } else {
                (end - loc) / (end - peak)
            };
        }
        delta += scalar * delta_set.get(i).copied().unwrap_or(0) as f64;
    }
    delta
}
