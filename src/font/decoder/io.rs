pub fn read_u16_be(data: &[u8], off: usize) -> Option<u16> {
    let b0 = *data.get(off)?;
    let b1 = *data.get(off + 1)?;
    Some(((b0 as u16) << 8) | b1 as u16)
}

pub fn read_u32_be(data: &[u8], off: usize) -> Option<u32> {
    let b0 = *data.get(off)?;
    let b1 = *data.get(off + 1)?;
    let b2 = *data.get(off + 2)?;
    let b3 = *data.get(off + 3)?;
    Some(((b0 as u32) << 24) | ((b1 as u32) << 16) | ((b2 as u32) << 8) | b3 as u32)
}

pub fn read_i16_be(data: &[u8], off: usize) -> Option<i16> {
    read_u16_be(data, off).map(|v| v as i16)
}

pub fn write_u16_be(data: &mut [u8], off: usize, val: u16) {
    data[off] = (val >> 8) as u8;
    data[off + 1] = val as u8;
}

pub fn write_u32_be(data: &mut [u8], off: usize, val: u32) {
    data[off] = (val >> 24) as u8;
    data[off + 1] = (val >> 16) as u8;
    data[off + 2] = (val >> 8) as u8;
    data[off + 3] = val as u8;
}

pub fn write_i16_be(data: &mut [u8], off: usize, val: i16) {
    write_u16_be(data, off, val as u16);
}
