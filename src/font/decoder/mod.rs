mod io;
mod glyf;
mod woff2;
mod ttf;
mod meta;

pub use io::{read_u16_be, read_u32_be, read_i16_be, write_u16_be, write_u32_be, write_i16_be};
pub use woff2::{get_compressed_range, decode_woff2_tables, decompress_brotli};
pub use ttf::{build_ttf, extract_ttf_tables, extract_ttc_tables, ttc_font_count};
pub use meta::{read_font_family_name, read_os2_weight, read_font_style, read_wght_axis};
