use texcraft_preview::figure::{Kind, decode, kind};

fn nothing(_: &str) -> Option<Vec<u8>> {
    None
}

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
    let figure = decode("figure.png", &pixel().unwrap(), 0, 1.0, 4096, &nothing).unwrap();
    let image = figure.image;
    assert_eq!(figure.size, [1.0, 1.0]);
    assert_eq!(image.rgba[3], 128);
    assert!(image.rgba[0] < 200 && image.rgba[0] > 90, "red was not premultiplied: {}", image.rgba[0]);
}

#[test]
fn the_formats_retex_takes_are_figures_and_a_damaged_file_is_an_error() {
    assert_eq!(kind("a/b.JPG"), Some(Kind::Raster));
    assert_eq!(kind("x.pdf"), Some(Kind::Pdf));
    assert_eq!(kind("x.svg"), Some(Kind::Svg));
    assert_eq!(kind("x.otf"), None);
    assert!(decode("broken.png", b"\x89PNG\r\n\x1a\nnot really", 0, 1.0, 4096, &nothing).is_err());
    assert!(decode("broken.jpg", &[0xFF, 0xD8, 0xFF, 0x00], 0, 1.0, 4096, &nothing).is_err());
}

#[test]
fn an_svg_draws_the_images_it_places_from_the_project_and_scales() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" xmlns:xlink="http://www.w3.org/1999/xlink" width="10" height="10"><image xlink:href="photo.png" x="0" y="0" width="10" height="10"/></svg>"#;
    let png = pixel().unwrap();
    let lookup = move |href: &str| (href == "photo.png").then(|| png.clone());
    let figure = decode("fig.svg", svg, 0, 4.0, 4096, &lookup).unwrap();
    assert_eq!((figure.image.width, figure.image.height), (40, 40), "the drawing was not drawn at the scale asked");
    let centre = ((20 * 40 + 20) * 4) as usize;
    assert!(figure.image.rgba[centre + 3] > 0, "the placed image is missing");
    let alone = decode("fig.svg", svg, 0, 4.0, 4096, &nothing).unwrap();
    assert_eq!(alone.image.rgba[centre + 3], 0, "an image that cannot be found drew something");
}

#[test]
fn no_side_is_longer_than_the_largest_texture() {
    let svg = br#"<svg xmlns="http://www.w3.org/2000/svg" width="500" height="100"><rect width="500" height="100" fill="red"/></svg>"#;
    let figure = decode("wide.svg", svg, 0, 10.0, 1024, &nothing).unwrap();
    assert!(figure.image.width <= 1024 && figure.image.height <= 1024, "{}x{}", figure.image.width, figure.image.height);
    assert_eq!(figure.size, [500.0, 100.0], "the natural size changed with the drawing's");
}
