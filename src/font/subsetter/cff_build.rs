use super::super::decoder::write_u16_be;

pub fn encode_cff_index(objects: &[Vec<u8>]) -> Vec<u8> {
    if objects.is_empty() {
        return vec![0, 0];
    }

    let mut offsets = vec![1usize];
    for obj in objects {
        offsets.push(offsets.last().unwrap() + obj.len());
    }
    let max_off  = *offsets.last().unwrap();
    let off_size = if max_off <= 0xFF        { 1usize }
        else if max_off <= 0xFFFF            { 2 }
        else if max_off <= 0xFFFFFF          { 3 }
        else                                 { 4 };

    let header_size: usize = 2 + 1 + (objects.len() + 1) * off_size;
    let data_size:   usize = objects.iter().map(|o| o.len()).sum();
    let mut out = vec![0u8; header_size + data_size];

    write_u16_be(&mut out, 0, objects.len() as u16);
    out[2] = off_size as u8;

    for (i, &o) in offsets.iter().enumerate() {
        let pos = 3 + i * off_size;
        for j in 0..off_size {
            out[pos + off_size - 1 - j] = ((o >> (j * 8)) & 0xFF) as u8;
        }
    }

    let mut data_pos = header_size;
    for obj in objects {
        out[data_pos..data_pos + obj.len()].copy_from_slice(obj);
        data_pos += obj.len();
    }

    out
}

pub fn encode_cff_int(n: i32) -> Vec<u8> {
    // always 5-byte form — deterministic size keeps Top DICT INDEX at exactly 28 bytes
    vec![29, (n >> 24) as u8, (n >> 16) as u8, (n >> 8) as u8, n as u8]
}
