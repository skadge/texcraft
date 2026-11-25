//! A page's text, glyph by glyph, for selecting and copying it.

use std::sync::Arc;

use texcraft_preview::{Document, glyph_at, text_between};

use crate::sample::links_pdf;

#[test]
fn the_text_between_two_glyphs_reads_as_written() {
    let doc = Document::open(Arc::new(links_pdf())).unwrap();
    let glyphs = doc.glyphs(0).unwrap();
    assert_eq!(text_between(&glyphs, 0, glyphs.len() - 1), "Hello world");
    // The text sits at y 700 from the bottom of a 792-point page, from x 72: near the top left.
    let first = glyph_at(&glyphs, 72.0, 792.0 - 704.0).unwrap();
    assert_eq!(glyphs[first].text, "H");
    let last = glyph_at(&glyphs, 400.0, 792.0 - 704.0).unwrap();
    assert_eq!(text_between(&glyphs, last, first), "Hello world", "the order of the ends matters");
    assert!(glyphs[0].rect[1] < 792.0 - 700.0 && glyphs[0].rect[3] > 792.0 - 700.0, "the glyph's box: {:?}", glyphs[0].rect);
}

#[test]
fn a_gap_between_words_reads_as_a_space_and_a_new_line_as_a_line_break() {
    let doc = Document::open(Arc::new(links_pdf())).unwrap();
    let glyphs = doc.glyphs(1).unwrap();
    assert_eq!(text_between(&glyphs, 0, glyphs.len() - 1), "Second page\nNext line");
}
