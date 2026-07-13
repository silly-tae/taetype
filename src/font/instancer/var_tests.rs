// Synthetic-table unit tests for the variation machinery that real test fonts
// don't exercise (Inter has no cvar, avar v1 only, and its MVAR tags don't
// surface through the public API). Tables are built by hand per spec.
#![cfg(test)]

use std::collections::HashMap;
use super::axis::apply_avar_all;
use super::mvar::apply_mvar;
use super::cvar::apply_cvar;

fn be16(v: i32) -> [u8; 2] { (v as i16 as u16).to_be_bytes() }

// minimal IVS: 1 axis, 1 region [0, 1, 1], one IVD with 1 delta set { delta }
fn build_ivs(delta: i16) -> Vec<u8> {
    let mut ivs = Vec::new();
    ivs.extend_from_slice(&[0, 1]);              // format 1
    ivs.extend_from_slice(&12u32.to_be_bytes()); // regionListOffset
    ivs.extend_from_slice(&[0, 1]);              // ivdCount = 1
    ivs.extend_from_slice(&22u32.to_be_bytes()); // ivd[0] offset (region list is 10 bytes)
    // region list @12: axisCount=1, regionCount=1, region: start 0, peak 1.0, end 1.0
    ivs.extend_from_slice(&[0, 1, 0, 1]);
    ivs.extend_from_slice(&be16(0));
    ivs.extend_from_slice(&be16(16384));
    ivs.extend_from_slice(&be16(16384));
    // IVD @24: itemCount=1, wordDeltaCount=1(16-bit), regionCount=1, region idx 0, delta
    ivs.extend_from_slice(&[0, 1, 0, 1, 0, 1, 0, 0]);
    ivs.extend_from_slice(&be16(delta as i32));
    ivs
}

#[test]
fn avar2_var_store_shifts_coord() {
    // avar v2, 1 axis, empty segment map, no index map, varStore delta -4096
    // (= -0.25 in 2.14) at peak 1.0 → coord 1.0 becomes 0.75
    let mut avar = Vec::new();
    avar.extend_from_slice(&[0, 2, 0, 0, 0, 0, 0, 1]); // major 2, minor 0, reserved, axisCount 1
    avar.extend_from_slice(&[0, 0]);                   // axis 0: 0 segment pairs
    let var_store_off = 8 + 2 + 8;                     // header + segmap + two offsets
    avar.extend_from_slice(&0u32.to_be_bytes());       // axisIndexMapOffset = 0 (identity)
    avar.extend_from_slice(&(var_store_off as u32).to_be_bytes());
    avar.extend_from_slice(&build_ivs(-4096));

    let mut map = HashMap::new();
    map.insert("avar".to_string(), avar);
    let mut loc = vec![1.0f64];
    apply_avar_all(&map, &mut loc);
    assert!((loc[0] - 0.75).abs() < 1e-6, "got {}", loc[0]);
}

#[test]
fn avar1_segment_map_still_applies() {
    // v1 map on one axis: {-1→-1, 0→0, 0.5→0.25, 1→1}; query 0.5 → 0.25
    let mut avar = Vec::new();
    avar.extend_from_slice(&[0, 1, 0, 0, 0, 0, 0, 1]);
    avar.extend_from_slice(&[0, 4]);
    for (f, t) in [(-16384, -16384), (0, 0), (8192, 4096), (16384, 16384)] {
        avar.extend_from_slice(&be16(f));
        avar.extend_from_slice(&be16(t));
    }
    let mut map = HashMap::new();
    map.insert("avar".to_string(), avar);
    let mut loc = vec![0.5f64];
    apply_avar_all(&map, &mut loc);
    assert!((loc[0] - 0.25).abs() < 1e-6, "got {}", loc[0]);
}

#[test]
fn mvar_applies_hasc_to_os2_and_hhea() {
    // one value record: hasc → delta +50 at coord 1.0
    let mut mvar = Vec::new();
    mvar.extend_from_slice(&[0, 1, 0, 0, 0, 0]); // version 1.0, reserved
    mvar.extend_from_slice(&[0, 8]);             // valueRecordSize
    mvar.extend_from_slice(&[0, 1]);             // valueRecordCount
    let ivs_off = 12 + 8;
    mvar.extend_from_slice(&(ivs_off as u16).to_be_bytes());
    mvar.extend_from_slice(b"hasc");
    mvar.extend_from_slice(&[0, 0, 0, 0]);       // outer 0, inner 0
    mvar.extend_from_slice(&build_ivs(50));

    let mut map = HashMap::new();
    map.insert("MVAR".to_string(), mvar);
    let mut hhea = vec![0u8; 36];
    hhea[4] = 0x03; hhea[5] = 0xE8;              // ascender 1000
    let mut os2 = vec![0u8; 96];
    os2[68] = 0x03; os2[69] = 0xE8;              // sTypoAscender 1000
    let mut post = vec![0u8; 32];

    apply_mvar(&map, &mut hhea, &mut os2, &mut post, &[1.0]).unwrap();
    assert_eq!(i16::from_be_bytes([hhea[4], hhea[5]]), 1050);
    assert_eq!(i16::from_be_bytes([os2[68], os2[69]]), 1050);
}

#[test]
fn cvar_shifts_cvt_values() {
    // 2 cvt entries; one tuple, embedded peak 1.0, private points [1], delta +7
    let mut cvar = Vec::new();
    cvar.extend_from_slice(&[0, 1, 0, 0]);       // version
    cvar.extend_from_slice(&[0, 1]);             // tupleVariationCount = 1 (no shared pts)
    let serialized_off = 8 + 4 + 2;              // header + tuple header + peak
    cvar.extend_from_slice(&(serialized_off as u16).to_be_bytes());
    // tuple header: varDataSize, tupleIndex (0x8000 peak | 0x2000 private points)
    let var_data: &[u8] = &[
        1, 0, 1,    // packed points: count 1, ctrl (runCount-1 = 0, bytes), idx delta 1
        0x00, 7,    // packed deltas: run of 1 byte-delta, value 7
    ];
    cvar.extend_from_slice(&(var_data.len() as u16).to_be_bytes());
    cvar.extend_from_slice(&(0x8000u16 | 0x2000u16).to_be_bytes());
    cvar.extend_from_slice(&be16(16384));        // peak 1.0
    cvar.extend_from_slice(var_data);

    let mut map = HashMap::new();
    map.insert("cvar".to_string(), cvar);
    let mut cvt = Vec::new();
    cvt.extend_from_slice(&be16(100));
    cvt.extend_from_slice(&be16(200));

    apply_cvar(&map, &mut cvt, &[1.0], 1).unwrap();
    assert_eq!(i16::from_be_bytes([cvt[0], cvt[1]]), 100, "untouched entry moved");
    assert_eq!(i16::from_be_bytes([cvt[2], cvt[3]]), 207, "delta not applied");
}
