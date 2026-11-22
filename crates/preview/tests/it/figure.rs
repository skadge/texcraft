use texcraft_preview::figure::{Kind, decode, kind};

/// A PNG of one pixel, written here: RGBA 200, 100, 0, half transparent.
fn pixel() -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::new();
    {
        let mut encoder = png::Encoder::new(&mut out, 1, 1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header()?;
        writer.write_image_data(&[200, 100, 0, 128])?;
    }
    Ok(out)
}

#[test]
fn a_half_transparent_pixel_comes_out_premultiplied_and_sized() {
    let (image, size) = decode("figure.png", &pixel().unwrap()).unwrap();
    assert_eq!(size, [1.0, 1.0]);
    assert_eq!(image.rgba[3], 128);
    assert!(image.rgba[0] < 200 && image.rgba[0] > 90, "red was not premultiplied: {}", image.rgba[0]);
}

#[test]
fn the_formats_retex_takes_are_figures_and_a_damaged_file_is_an_error() {
    assert_eq!(kind("a/b.JPG"), Some(Kind::Raster));
    assert_eq!(kind("x.pdf"), Some(Kind::Pdf));
    assert_eq!(kind("x.svg"), Some(Kind::Svg));
    assert_eq!(kind("x.otf"), None);
    assert!(decode("broken.png", b"\x89PNG\r\n\x1a\nnot really").is_err());
    assert!(decode("broken.jpg", &[0xFF, 0xD8, 0xFF, 0x00]).is_err());
}
