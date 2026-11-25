//! The links of a page: its link annotations (ISO 32000-2 §12.5.6.5), each with where it goes,
//! a place in the document (an explicit or a named destination, or a GoTo action) or a URI. A
//! URI is reported as written: whether it may be opened is the caller's check
//! (`craft_pdf::safe_url`).

use craft_pdf::syntax::Pdf;
use craft_pdf::syntax::object::{Array, Dict, Name, Object, ObjectIdentifier};
use craft_pdf::syntax::page::Page;

/// How deep a name tree is followed, and how many of its nodes are read: bounds for a hostile file.
const MAX_TREE_DEPTH: usize = 32;
const MAX_TREE_NODES: usize = 4096;
/// At most this many links are read from a page.
const MAX_LINKS: usize = 4096;

/// Where a link goes.
#[derive(Debug, Clone, PartialEq)]
pub enum Target {
    /// Page `page` (from 0), at `y` points from its top when the destination says.
    Page { page: usize, y: Option<f32> },
    /// A URI, as the file writes it (not checked).
    Uri(String),
}

/// A link on a page: its area, in points from the page's top left corner as displayed, and
/// where it goes.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// Left, top, right, bottom.
    pub rect: [f32; 4],
    pub target: Target,
}

impl Link {
    /// Whether the point `(x, y)` (points from the page's top left) is on the link.
    pub fn contains(&self, x: f32, y: f32) -> bool {
        let [l, t, r, b] = self.rect;
        (l..=r).contains(&x) && (t..=b).contains(&y)
    }
}

/// The links of page `index` of `pdf`.
pub(crate) fn links(pdf: &Pdf, index: usize) -> Vec<Link> {
    let pages = pdf.pages();
    let Some(page) = pages.get(index) else { return Vec::new() };
    let Some(annots) = page.raw().get::<Array<'_>>(b"Annots") else { return Vec::new() };
    let ids: Vec<Option<ObjectIdentifier>> = pages.iter().map(|p| p.raw().obj_id()).collect();
    let catalog = pdf.xref().get::<Dict<'_>>(pdf.xref().root_id());
    let doc = Doc { pdf, ids: &ids, catalog: catalog.as_ref() };
    let mut out = Vec::new();
    for annot in annots.iter::<Dict<'_>>().take(MAX_LINKS) {
        if annot.get::<Name<'_>>(b"Subtype").is_none_or(|s| s.as_ref() != b"Link") {
            continue;
        }
        let Some(rect) = annot.get::<craft_pdf::syntax::object::Rect>(b"Rect") else { continue };
        let target = match annot.get::<Object<'_>>(b"Dest") {
            Some(dest) => doc.destination(dest, 0),
            None => annot.get::<Dict<'_>>(b"A").and_then(|a| doc.action(&a)),
        };
        if let Some(target) = target {
            let (x0, y0) = to_page(page, rect.x0, rect.y0);
            let (x1, y1) = to_page(page, rect.x1, rect.y1);
            out.push(Link { rect: [x0.min(x1), y0.min(y1), x0.max(x1), y0.max(y1)], target });
        }
    }
    out
}

/// A point of `page`'s user space, in points from its top left corner as displayed.
fn to_page(page: &Page<'_>, x: f64, y: f64) -> (f32, f32) {
    let [a, b, c, d, e, f] = page.initial_transform(true).as_coeffs();
    ((a * x + c * y + e) as f32, (b * x + d * y + f) as f32)
}

struct Doc<'p, 'a> {
    pdf: &'a Pdf,
    /// Each page's object, by page index.
    ids: &'p [Option<ObjectIdentifier>],
    catalog: Option<&'p Dict<'a>>,
}

impl<'a> Doc<'_, 'a> {
    /// A link action: GoTo (a destination in this document) or URI. Others (launching a
    /// program, JavaScript, another document) are not followed.
    fn action(&self, action: &Dict<'a>) -> Option<Target> {
        let kind = action.get::<Name<'_>>(b"S")?;
        match kind.as_ref() {
            b"GoTo" => self.destination(action.get::<Object<'_>>(b"D")?, 0),
            b"URI" => {
                let uri = action.get::<craft_pdf::syntax::object::String<'_>>(b"URI")?;
                Some(Target::Uri(String::from_utf8_lossy(uri.as_bytes()).into_owned()))
            }
            _ => None,
        }
    }

    /// A destination: an array `[page /XYZ left top zoom]` (or another fit), a name or a string
    /// naming one, or a dictionary holding it under `/D`.
    fn destination(&self, dest: Object<'a>, depth: usize) -> Option<Target> {
        if depth > 2 {
            return None;
        }
        match dest {
            Object::Array(a) => self.explicit(&a),
            Object::Name(n) => self.named(n.as_ref(), depth),
            Object::String(s) => self.named(s.as_bytes(), depth),
            Object::Dict(d) => self.destination(d.get::<Object<'_>>(b"D")?, depth + 1),
            _ => None,
        }
    }

    fn explicit(&self, dest: &Array<'a>) -> Option<Target> {
        let mut items = dest.raw_iter();
        let page = match items.next()? {
            craft_pdf::syntax::object::MaybeRef::Ref(r) => {
                let id: ObjectIdentifier = r.into();
                self.ids.iter().position(|p| *p == Some(id))?
            }
            // Some writers give the page's number (from 0) instead.
            craft_pdf::syntax::object::MaybeRef::NotRef(Object::Number(n)) => usize::try_from(n.as_i64()).ok()?,
            craft_pdf::syntax::object::MaybeRef::NotRef(_) => return None,
        };
        let pages = self.pdf.pages();
        let p = pages.get(page)?;
        let fit = match items.next() {
            Some(craft_pdf::syntax::object::MaybeRef::NotRef(Object::Name(n))) => n.as_ref().to_vec(),
            _ => Vec::new(),
        };
        let numbers: Vec<Option<f64>> = items
            .map(|i| match i {
                craft_pdf::syntax::object::MaybeRef::NotRef(Object::Number(n)) => Some(n.as_f64()),
                _ => None,
            })
            .collect();
        let arg = |i: usize| numbers.get(i).copied().flatten();
        // The top of the destination, in user space, when the fit gives one.
        let (left, top) = match fit.as_slice() {
            b"XYZ" => (arg(0), arg(1)),
            b"FitH" | b"FitBH" => (None, arg(0)),
            b"FitR" => (arg(0), arg(3)),
            _ => (None, None),
        };
        let y = top.map(|top| to_page(p, left.unwrap_or(0.0), top).1.max(0.0));
        Some(Target::Page { page, y })
    }

    /// The destination named `name`: in the catalog's `/Dests` dictionary (PDF 1.1) or its
    /// `/Names` `/Dests` name tree.
    fn named(&self, name: &[u8], depth: usize) -> Option<Target> {
        let catalog = self.catalog?;
        if let Some(dests) = catalog.get::<Dict<'_>>(b"Dests")
            && let Some(dest) = dests.get::<Object<'_>>(name)
        {
            return self.destination(dest, depth + 1);
        }
        let tree = catalog.get::<Dict<'_>>(b"Names")?.get::<Dict<'_>>(b"Dests")?;
        let mut budget = MAX_TREE_NODES;
        let dest = find_in_tree(&tree, name, 0, &mut budget)?;
        self.destination(dest, depth + 1)
    }
}

/// The value of `key` in the name tree under `node`.
fn find_in_tree<'a>(node: &Dict<'a>, key: &[u8], depth: usize, budget: &mut usize) -> Option<Object<'a>> {
    if depth > MAX_TREE_DEPTH || *budget == 0 {
        return None;
    }
    *budget -= 1;
    if let Some(names) = node.get::<Array<'_>>(b"Names") {
        let mut items = names.iter::<Object<'_>>();
        while let (Some(k), Some(v)) = (items.next(), items.next()) {
            if let Object::String(s) = k
                && s.as_bytes() == key
            {
                return Some(v);
            }
        }
    }
    let kids = node.get::<Array<'_>>(b"Kids")?;
    for kid in kids.iter::<Dict<'_>>() {
        // A kid whose `/Limits` leave the key out is skipped.
        if let Some([low, high]) = kid.get::<Array<'_>>(b"Limits").map(|l| {
            let mut i = l.iter::<craft_pdf::syntax::object::String<'_>>();
            [i.next(), i.next()]
        }) && let (Some(low), Some(high)) = (low, high)
            && (key < low.as_bytes() || key > high.as_bytes())
        {
            continue;
        }
        if let Some(v) = find_in_tree(&kid, key, depth + 1, budget) {
            return Some(v);
        }
    }
    None
}
