//! A small PDF written here for the tests (our own work, made by this code): two US Letter pages,
//! with text in Helvetica and, on the first page, four links.
//!
//! Page 1: "Hello world" at (72, 700) and four links, each 128 × 20 points from x = 72:
//! - y 600–620: to page 2 at y 400 (an explicit destination, `/Dest [page /XYZ …]`);
//! - y 500–520: to `https://example.com/` (a URI action);
//! - y 400–420: to the named destination `sec.2`, page 2 at y 300 (through the `/Names` tree);
//! - y 300–320: to `javascript:alert(1)`, a URI no reader should open.
//!
//! Page 2: "Second page" at (72, 700), as TeX writes it: no space drawn, a gap of a third of an em
//! instead; and "Next line" under it.
//!
//! The ui-egui tests include this file too.

/// The PDF's bytes.
pub fn links_pdf() -> Vec<u8> {
    let text = |s: &str| format!("BT /F1 12 Tf 72 700 Td ({s}) Tj ET");
    let page1 = text("Hello world");
    let page2 = "BT /F1 12 Tf 72 700 Td [(Second) -333 (page)] TJ 0 -14 Td (Next line) Tj ET".to_owned();
    let objects = [
        "<< /Type /Catalog /Pages 2 0 R /Names << /Dests << /Names [(sec.2) [4 0 R /XYZ 0 300 null]] >> >> >>".to_owned(),
        "<< /Type /Pages /Kids [3 0 R 4 0 R] /Count 2 /MediaBox [0 0 612 792] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 5 0 R /Resources << /Font << /F1 7 0 R >> >> /Annots [8 0 R 9 0 R 10 0 R 11 0 R] >>".to_owned(),
        "<< /Type /Page /Parent 2 0 R /Contents 6 0 R /Resources << /Font << /F1 7 0 R >> >> >>".to_owned(),
        format!("<< /Length {} >>\nstream\n{page1}\nendstream", page1.len() + 1),
        format!("<< /Length {} >>\nstream\n{page2}\nendstream", page2.len() + 1),
        "<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /Encoding /WinAnsiEncoding >>".to_owned(),
        "<< /Type /Annot /Subtype /Link /Rect [72 600 200 620] /Border [0 0 0] /Dest [4 0 R /XYZ 0 400 null] >>".to_owned(),
        "<< /Type /Annot /Subtype /Link /Rect [72 500 200 520] /Border [0 0 0] /A << /S /URI /URI (https://example.com/) >> >>".to_owned(),
        "<< /Type /Annot /Subtype /Link /Rect [72 400 200 420] /Border [0 0 0] /Dest (sec.2) >>".to_owned(),
        "<< /Type /Annot /Subtype /Link /Rect [72 300 200 320] /Border [0 0 0] /A << /S /URI /URI (javascript:alert\\(1\\)) >> >>".to_owned(),
    ];
    let mut pdf = b"%PDF-1.7\n".to_vec();
    let mut offsets = Vec::new();
    for (i, body) in objects.iter().enumerate() {
        offsets.push(pdf.len());
        pdf.extend_from_slice(format!("{} 0 obj\n{body}\nendobj\n", i + 1).as_bytes());
    }
    let xref = pdf.len();
    pdf.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objects.len() + 1).as_bytes());
    for o in offsets {
        pdf.extend_from_slice(format!("{o:010} 00000 n \n").as_bytes());
    }
    pdf.extend_from_slice(format!("trailer\n<< /Size {} /Root 1 0 R >>\nstartxref\n{xref}\n%%EOF\n", objects.len() + 1).as_bytes());
    pdf
}
