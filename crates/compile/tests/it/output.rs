//! Accessible and print builds through retex: what they write and the findings they place.

use std::sync::Arc;

use texcraft_compile::{Backend, Job, Options, Outcome, Output, Severity, Status};
use texcraft_project::ProjectPath;

/// A 16 by 16 pixel PNG (RGB, stored without compression): printed 8 cm wide it is far below
/// print resolution.
fn small_png() -> Vec<u8> {
    fn crc(bytes: &[u8]) -> u32 {
        let mut c = 0xffff_ffffu32;
        for b in bytes {
            c ^= u32::from(*b);
            for _ in 0..8 {
                c = if c & 1 == 1 { 0xedb8_8320 ^ (c >> 1) } else { c >> 1 };
            }
        }
        !c
    }
    fn chunk(out: &mut Vec<u8>, kind: &[u8], data: &[u8]) {
        out.extend((data.len() as u32).to_be_bytes());
        let mut body = kind.to_vec();
        body.extend(data);
        out.extend(&body);
        out.extend(crc(&body).to_be_bytes());
    }
    let (w, h) = (16u32, 16u32);
    let mut raw = Vec::new();
    for y in 0..h {
        raw.push(0);
        for x in 0..w {
            raw.extend([(x * 16) as u8, (y * 16) as u8, 100]);
        }
    }
    // zlib with one stored block, and its Adler-32.
    let (mut a, mut b) = (1u32, 0u32);
    for byte in &raw {
        a = (a + u32::from(*byte)) % 65521;
        b = (b + a) % 65521;
    }
    let len = raw.len() as u16;
    let mut z = vec![0x78, 0x01, 0x01];
    z.extend(len.to_le_bytes());
    z.extend((!len).to_le_bytes());
    z.extend(&raw);
    z.extend(((b << 16) | a).to_be_bytes());
    let mut png = b"\x89PNG\r\n\x1a\n".to_vec();
    let mut ihdr = Vec::new();
    ihdr.extend(w.to_be_bytes());
    ihdr.extend(h.to_be_bytes());
    ihdr.extend([8, 2, 0, 0, 0]);
    chunk(&mut png, b"IHDR", &ihdr);
    chunk(&mut png, b"IDAT", &z);
    chunk(&mut png, b"IEND", &[]);
    png
}

/// The image is on line 6.
const DOCUMENT: &str = "\\documentclass{article}\n\\usepackage{graphicx}\n\\title{Output}\n\\begin{document}\nHello world.\n\\includegraphics[width=8cm]{low.png}\n\\end{document}\n";

fn build(name: &str, options: Options) -> Result<Outcome, String> {
    let dir = std::env::temp_dir().join(format!("texcraft-output-{name}-{}", std::process::id()));
    let mut backend = texcraft_compile::retex_backend::Retex::new(Some(dir.clone()));
    let main = ProjectPath::new("main.tex").map_err(|e| e.to_string())?;
    let image = ProjectPath::new("low.png").map_err(|e| e.to_string())?;
    let files = vec![(main.clone(), Arc::from(DOCUMENT.as_bytes())), (image, Arc::from(small_png().as_slice()))];
    let outcome =
        backend.compile(&Job { files, main, id: 1, options, project: None, packages: Default::default(), missing_packages: Default::default() });
    let _ = std::fs::remove_dir_all(&dir);
    if outcome.status != Status::Built {
        return Err(format!("not built: {:?} {:?}", outcome.status, outcome.diagnostics));
    }
    Ok(outcome)
}

/// The numbers of a page box (`/TrimBox [0 0 612 792]`) in a PDF.
fn page_box(pdf: &[u8], name: &str) -> Option<[f32; 4]> {
    let text = String::from_utf8_lossy(pdf);
    let after = text.split(&format!("/{name}")).nth(1)?;
    let inside = after.trim_start().strip_prefix('[')?.split(']').next()?;
    let n: Vec<f32> = inside.split_whitespace().filter_map(|v| v.parse().ok()).collect();
    n.try_into().ok()
}

#[test]
fn an_accessible_build_declares_pdf_ua_and_finds_an_image_without_alternative_text() -> Result<(), String> {
    let outcome = build("ua", Options { output: Output::Accessible, ..Options::default() })?;
    let pdf = outcome.pdf.as_ref().ok_or("no PDF")?;
    assert!(String::from_utf8_lossy(pdf).contains("pdfuaid:part"), "the PDF does not declare PDF/UA in its XMP");
    let image = outcome.accessibility().find(|d| d.message.contains("alternative text")).ok_or("the image without alternative text was not found")?;
    assert_eq!((image.file.as_deref(), image.line), (Some("main.tex"), Some(6)), "the finding is not at the image's line");

    let screen = build("screen", Options::default())?;
    assert_eq!(screen.accessibility().count(), 0, "a screen build was checked for accessibility");
    assert!(screen.preflight.is_none() && screen.print_problems().is_none());
    Ok(())
}

#[test]
fn a_print_build_writes_a_pdfx_with_bleed_and_places_a_low_resolution_image() -> Result<(), String> {
    let outcome = build("print", Options { output: Output::Print, bleed_mm: 5.0, ..Options::default() })?;
    let preflight = outcome.preflight.as_ref().ok_or("no preflight")?;
    let pdfx = preflight.pdfx.as_ref().ok_or("no PDF/X")?;
    let trim = page_box(pdfx, "TrimBox").ok_or("the PDF/X has no TrimBox")?;
    let bleed = page_box(pdfx, "BleedBox").ok_or("the PDF/X has no BleedBox")?;
    let media = page_box(pdfx, "MediaBox").ok_or("the PDF/X has no MediaBox")?;
    assert!(bleed[0] < trim[0] && bleed[2] > trim[2], "the bleed is not outside the trim: {bleed:?} {trim:?}");
    assert!(media[2] - media[0] > bleed[2] - bleed[0], "no room for crop marks outside the bleed");
    assert!(!preflight.rules.is_empty(), "the PDF/X's rules are missing");

    let check = preflight.checks.iter().find(|c| c.kind == "low-resolution").ok_or("the low-resolution image was not found")?;
    assert_eq!((check.file.as_deref(), check.line, check.page), (Some("main.tex"), Some(6), 0));
    assert!(check.width > 0.0 && check.height > 0.0, "the check has no area on the page");
    let message = outcome.diagnostics.iter().find(|d| d.is_print() && d.line == Some(6)).ok_or("no print message at the image's line")?;
    assert!(message.severity >= Severity::Warning);
    assert_eq!(outcome.place_on_page(message), Some(check), "the message is not placed on the page");
    assert!(outcome.print_problems().is_some_and(|n| n >= 1), "the problem is not counted");
    Ok(())
}
