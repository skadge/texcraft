use texcraft_buffer::Buffer;

#[test]
fn utf16_columns_count_astral_characters_twice_and_round_trip() {
    let b = Buffer::new("ab\n😀x\n");
    let x = 4; // a, b, LF, 😀, then x
    assert_eq!(b.to_utf16(x), (1, 2));
    assert_eq!(b.from_utf16(1, 2), Ok(x));
    assert!(b.from_utf16(1, 1).is_err(), "a column inside a surrogate pair was accepted");
    assert!(b.from_utf16(0, 9).is_err(), "a column past the line's end was accepted");
}

#[test]
fn lines_end_at_lf_only() {
    let b = Buffer::new("a\r\nb\u{2028}c");
    assert_eq!(b.len_lines(), 2);
    assert_eq!(b.line(0), "a\r");
    assert_eq!(b.line(1), "b\u{2028}c");
}

#[test]
fn merged_carets_are_in_order_without_overlaps_and_keep_the_primary() {
    use texcraft_buffer::{Range, Selection};
    let s = Selection::with_primary(vec![Range::point(9), Range::new(2, 5), Range::point(4), Range::point(9), Range::point(0)], 1).merged();
    let ranges = s.ranges();
    assert!(ranges.windows(2).all(|w| w[0].end() < w[1].start()), "{ranges:?}");
    assert_eq!(ranges.len(), 3, "the caret inside 2..5 and the repeated caret at 9 merge: {ranges:?}");
    assert_eq!(s.primary(), Range::new(2, 5));
}
