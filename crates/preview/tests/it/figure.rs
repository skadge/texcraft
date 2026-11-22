use craft_image_codecs::{ChannelLayout, EncodeOptions, Format, Image, SampleType, encode};
use texcraft_preview::figure::{Kind, decode, kind};

#[test]
fn a_half_transparent_pixel_comes_out_premultiplied_and_sized() {
    let pixel = Image::from_raw(1, 1, ChannelLayout::Rgba, SampleType::U8, vec![200, 100, 0, 128]).unwrap();
    let png = encode(&pixel, Format::Png, &EncodeOptions::default()).unwrap();
    let (image, size) = decode("figure.png", &png).unwrap();
    assert_eq!(size, [1.0, 1.0]);
    assert_eq!(image.rgba[3], 128);
    assert!(image.rgba[0] < 200 && image.rgba[0] > 90, "red was not premultiplied: {}", image.rgba[0]);
}

#[test]
fn the_formats_retex_takes_are_figures_and_others_are_not() {
    assert_eq!(kind("a/b.JPG"), Some(Kind::Raster));
    assert_eq!(kind("x.jxl"), Some(Kind::Raster));
    assert_eq!(kind("x.pdf"), Some(Kind::Pdf));
    assert_eq!(kind("x.svg"), Some(Kind::Svg));
    assert_eq!(kind("x.otf"), None);
    assert!(decode("broken.png", b"not a png").is_err());
}
