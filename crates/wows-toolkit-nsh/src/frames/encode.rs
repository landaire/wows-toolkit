use base64::Engine;
use base64::engine::general_purpose::STANDARD;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum UriFormat {
    Bmp,
    Png,
}

/// Opaque RGBA (premultiplied equals straight when alpha is 255) as a data URI.
pub fn data_uri(format: UriFormat, rgba: &[u8], width: u32, height: u32) -> String {
    let (mime, encoded) = match format {
        UriFormat::Bmp => ("image/bmp", bmp(rgba, width, height)),
        UriFormat::Png => ("image/png", png(rgba, width, height)),
    };

    let mut uri = String::with_capacity(32 + encoded.len() * 4 / 3 + 4);
    uri.push_str("data:");
    uri.push_str(mime);
    uri.push_str(";base64,");
    STANDARD.encode_string(&encoded, &mut uri);

    uri
}

/// 32-bit BI_RGB top-down BMP (BGRX rows).
pub fn bmp(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    const FILE_HEADER: u32 = 14;
    const INFO_HEADER: u32 = 40;

    let pixel_bytes = width * height * 4;
    let offset = FILE_HEADER + INFO_HEADER;
    let mut out = Vec::with_capacity((offset + pixel_bytes) as usize);

    out.extend_from_slice(b"BM");
    out.extend_from_slice(&(offset + pixel_bytes).to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&offset.to_le_bytes());

    out.extend_from_slice(&INFO_HEADER.to_le_bytes());
    out.extend_from_slice(&(width as i32).to_le_bytes());
    // Negative height marks top-down rows.
    out.extend_from_slice(&(-(height as i32)).to_le_bytes());
    out.extend_from_slice(&1u16.to_le_bytes());
    out.extend_from_slice(&32u16.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&pixel_bytes.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&2835i32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());
    out.extend_from_slice(&0u32.to_le_bytes());

    for [r, g, b, a] in rgba.as_chunks::<4>().0 {
        out.extend_from_slice(&[*b, *g, *r, *a]);
    }

    out
}

fn png(rgba: &[u8], width: u32, height: u32) -> Vec<u8> {
    let mut out = Vec::with_capacity(rgba.len() / 2);
    let mut encoder = png::Encoder::new(&mut out, width, height);
    encoder.set_color(png::ColorType::Rgba);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.set_compression(png::Compression::Fastest);
    encoder.set_filter(png::Filter::NoFilter);

    let mut writer = encoder.write_header().expect("in-memory PNG header write cannot fail");
    writer.write_image_data(rgba).expect("buffer matches the declared size");
    drop(writer);

    out
}
