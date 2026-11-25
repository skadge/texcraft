//! The links of a page, read from its link annotations.

use std::sync::Arc;

use texcraft_preview::{Document, Target};

use crate::sample::links_pdf;

#[test]
fn links_go_to_their_page_or_their_uri() {
    let doc = Document::open(Arc::new(links_pdf())).unwrap();
    let links = doc.links(0);
    let targets: Vec<&Target> = links.iter().map(|l| &l.target).collect();
    // Page 2 at y 400 and 300 from the bottom of a 792-point page: higher, then lower, on it.
    let [Target::Page { page: 1, y: Some(explicit) }, Target::Uri(web), Target::Page { page: 1, y: Some(named) }, Target::Uri(script)] = targets[..]
    else {
        panic!("the links read: {targets:?}");
    };
    assert!(explicit < named, "the destinations' heights: {explicit} {named}");
    assert_eq!(web, "https://example.com/");
    // Reported as written; opening it is refused by the caller's check.
    assert!(!craft_pdf::safe_url(script), "{script}");
    // The first link is at y 600–620 from the bottom: above the second, in points from the top.
    assert!(links[0].rect[3] < links[1].rect[1], "{:?} {:?}", links[0].rect, links[1].rect);
    assert!(links[0].contains(100.0, 792.0 - 610.0), "{:?}", links[0].rect);
    assert!(doc.links(1).is_empty());
}
