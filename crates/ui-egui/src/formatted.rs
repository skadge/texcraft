//! The formatted view of a `.tex` file (docs/ROADMAP.md): the file's view model
//! ([`texcraft_latex::view`]) drawn as a document, at a reading measure in the reading face.
//!
//! Its text is edited in place ([`crate::formatted_edit`], F3): the caret moves through paragraphs,
//! headings and list items, and each key makes a minimal edit of the source. A double-click in the
//! text selects a word, a triple-click the paragraph; Ctrl+double-click (Cmd on a Mac), Show in
//! Source, or a double-click on a part that is not text (or Enter on a formula, a figure or a part
//! shown as source) shows the source with the caret where that part starts. Formulas are drawn as retex typesets them after the main
//! document's preamble (F2: [`Session::formulas`](texcraft_engine::Session::formulas)), as ink in
//! the colour of the text, at the window's pixels per point; until then, and where the build has no
//! engine for them (the browser), they are shown as their source. What the view cannot show
//! faithfully stays as its source.

use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::sync::Arc;

use egui::text::{LayoutJob, TextFormat};
use egui::{Align, Align2, Color32, FontFamily, FontId, Galley, Pos2, Rect, Sense, Stroke, StrokeKind, Vec2, pos2, vec2};
use texcraft_engine::project::{Contents, Project, ProjectPath};
use texcraft_engine::{FormulaBox, FormulaState};
use texcraft_latex::auxfile::{Aux, CiteStyle, Missing};
use texcraft_latex::edit::{self, Line, Part};
use texcraft_latex::numbers::Number;
use texcraft_latex::view::{Block, BlockKind, ChipKind, Inline, InlineKind, Item, ListKind, Style, TheoremKind};
use texcraft_preview::figure;

use crate::App;
use crate::theme::{Tokens, blend};

#[path = "formatted_table.rs"]
mod table;
pub(crate) use table::CellAt;

#[path = "formatted_figure.rs"]
mod fig;

#[path = "formatted_source.rs"]
pub(crate) mod in_place;
use in_place::{Anchor, Atom, InPlace, What};

/// The view model of the shown file, for the session revision it was made from.
struct Cache {
    path: ProjectPath,
    revision: u64,
    text: Arc<str>,
    blocks: Arc<Vec<Block>>,
    /// The preamble's summary: the class and how many packages.
    summary: String,
    /// The theorem-like environments the main file declares, which the blocks were made with.
    theorems: Vec<TheoremKind>,
}

/// A file `\input` or `\include` reads, shown in its place: its path, text and view, or why it
/// cannot be shown.
type Included = Result<(ProjectPath, Arc<str>, Arc<Vec<Block>>), String>;

/// The files the shown file includes (and those they include), by the name as written, for the
/// project revision and buffer version they were read at.
struct Includes {
    of: (u64, u64),
    files: Arc<HashMap<String, Included>>,
}

/// What cross-references, citations and numbered parts show: from the last build's `.aux` and
/// the document's counting.
pub(crate) struct Refs {
    aux: Option<Arc<Aux>>,
    numbered: Arc<texcraft_engine::numbering::Numbered>,
    /// The bibliography's entries: each key's author and year.
    cited: Arc<HashMap<String, String>>,
}

/// How a chip is shown.
#[derive(Clone)]
enum Chip {
    /// As the PDF shows it ("(3)", "Figure 2", "[1]").
    Shown,
    /// Its key, muted, with "?", and why it has no number.
    Missing(String),
    /// A `\label`: its icon alone, the key on hover.
    Label,
    /// An `\input` in a sentence: its icon and the file's name.
    File,
}

/// The misspelled words' bytes, in order.
type Misspelled = Arc<Vec<Range<usize>>>;

/// An image a figure shows: its texture and natural size, or why it cannot be shown.
type Picture = Result<(egui::TextureHandle, Vec2), String>;

#[derive(Default)]
pub(crate) struct Formatted {
    /// Files shown formatted.
    files: HashSet<ProjectPath>,
    /// Files whose preamble is shown.
    preamble_open: HashSet<ProjectPath>,
    cache: Option<Cache>,
    /// By the image's project path and length.
    images: HashMap<(String, usize), Picture>,
    /// Typeset formulas drawn as textures.
    ink: Ink,
    /// The misspelled words' bytes, with the session revision and the time they were found at.
    misspelled: Option<(u64, f64, Misspelled)>,
    /// The grammar problems' bytes, the same way.
    grammar: Option<(u64, f64, Misspelled)>,
    /// The bibliography's entries (key, author and year), with the session revision.
    cited: Option<(u64, Arc<HashMap<String, String>>)>,
    /// The included files shown in place.
    includes: Option<Includes>,
    /// The caret's keys and pointer.
    pub editing: crate::formatted_edit::Editing,
    /// The toolbar's state: where the caret is.
    pub bar: crate::formatted_bar::State,
    /// The source editor open in the view (a part shown as source, a formula).
    pub in_place: Option<InPlace>,
    /// The new formula dialog.
    pub new_formula: Option<in_place::NewFormula>,
}

/// Typeset formulas as textures, by their batch's PDF, page and pixels per point; those not drawn
/// for a while are dropped.
#[derive(Default)]
struct Ink {
    /// The batches' PDFs, opened, by the address of their bytes (kept alive here).
    docs: HashMap<usize, (Arc<Vec<u8>>, Option<texcraft_preview::Document>)>,
    textures: HashMap<(usize, u32, u32), (Drawn, u64)>,
    frame: u64,
    /// Formulas that may still be rasterised this frame.
    budget: usize,
}

#[derive(Clone)]
enum Drawn {
    Ready(egui::TextureHandle),
    /// The page could not be drawn.
    Broken,
}

/// What [`Ink::texture`] gives.
enum Inked {
    Ready(egui::TextureId),
    /// Not rasterised yet (too many this frame).
    Later,
    Broken,
}

/// Formulas rasterised in one frame at most, so a file full of them does not stall the window.
const INK_PER_FRAME: usize = 24;

impl Ink {
    fn begin(&mut self) {
        self.frame += 1;
        self.budget = INK_PER_FRAME;
    }

    fn end(&mut self) {
        let frame = self.frame;
        self.textures.retain(|_, (_, used)| frame.saturating_sub(*used) < 600);
        let live: HashSet<usize> = self.textures.keys().map(|k| k.0).collect();
        self.docs.retain(|k, _| live.contains(k));
    }

    /// The texture of `b` at `px` pixels per TeX point.
    fn texture(&mut self, ctx: &egui::Context, b: &FormulaBox, px: f32) -> Inked {
        let max_side = ctx.input(|i| i.max_texture_side).min(4096) as f32;
        let px = px.min(max_side / b.width_pt.max(1.0)).min(max_side / (b.height_pt + b.depth_pt).max(1.0));
        let doc_key = Arc::as_ptr(&b.pdf) as usize;
        let key = (doc_key, b.page, (px * 64.0).round() as u32);
        if let Some((drawn, used)) = self.textures.get_mut(&key) {
            *used = self.frame;
            return match drawn {
                Drawn::Ready(t) => Inked::Ready(t.id()),
                Drawn::Broken => Inked::Broken,
            };
        }
        if self.budget == 0 {
            ctx.request_repaint();
            return Inked::Later;
        }
        self.budget -= 1;
        let (_, doc) = self.docs.entry(doc_key).or_insert_with(|| (Arc::clone(&b.pdf), texcraft_preview::Document::open(Arc::clone(&b.pdf)).ok()));
        let drawn = match doc.as_ref().map(|d| d.render_ink(b.page as usize, px)) {
            Some(Ok(img)) => {
                let pixels = egui::ColorImage::from_rgba_premultiplied([img.width as usize, img.height as usize], &img.rgba);
                Drawn::Ready(ctx.load_texture(format!("formula:{doc_key}:{}:{}", b.page, key.2), pixels, egui::TextureOptions::LINEAR))
            }
            _ => Drawn::Broken,
        };
        self.textures.insert(key, (drawn.clone(), self.frame));
        match drawn {
            Drawn::Ready(t) => Inked::Ready(t.id()),
            Drawn::Broken => Inked::Broken,
        }
    }
}

impl Formatted {
    /// The misspelled word holding source byte `at` (its bytes), as last found.
    pub(crate) fn misspelling_at(&self, at: usize) -> Option<Range<usize>> {
        let (_, _, found) = self.misspelled.as_ref()?;
        found.iter().find(|m| m.start <= at && at <= m.end).cloned()
    }

    /// Whether `path` is shown formatted.
    pub fn shows(&self, path: &ProjectPath) -> bool {
        self.files.contains(path)
    }

    pub fn set(&mut self, path: &ProjectPath, on: bool) {
        if on {
            self.files.insert(path.clone());
        } else {
            self.files.remove(path);
        }
    }
}

/// Whether `path` has a formatted view.
pub(crate) fn available(path: &ProjectPath) -> bool {
    path.as_str().ends_with(".tex")
}

/// The class and the number of packages of a preamble, for its folded bar.
fn summary(text: &str) -> String {
    let tree = texcraft_latex::tree::parse(text);
    let mut class = None;
    let mut packages = 0;
    for n in &tree {
        if let texcraft_latex::tree::Kind::Command { name, args, .. } = &n.kind
            && let Some(arg) = args.iter().rev().find(|a| !a.optional)
        {
            match name.as_str() {
                "documentclass" => class = text.get(arg.inner.clone()).map(str::trim),
                "usepackage" => packages += text.get(arg.inner.clone()).map_or(1, |p| p.split(',').filter(|p| !p.trim().is_empty()).count()),
                _ => {}
            }
        }
    }
    let packages = match packages {
        0 => String::new(),
        n => crate::i18n::trn(n, "{n} package", "{n} packages"),
    };
    match class {
        Some(c) if !packages.is_empty() => format!("{c} · {packages}"),
        Some(c) => c.to_owned(),
        None => packages,
    }
}

/// Where a part of the view sends the caret: its source's first byte.
struct Jump(usize);

/// What drawing the view needs and what it reports.
struct Cx<'a> {
    t: Tokens,
    text: &'a str,
    project: &'a Project,
    path: &'a ProjectPath,
    images: &'a mut HashMap<(String, usize), Picture>,
    preamble_open: bool,
    summary: &'a str,
    /// The byte to scroll to (a jump from the outline or the PDF), until a block at it is drawn.
    scroll_to: Option<usize>,
    /// Out: a part was double-clicked or activated.
    jump: Option<Jump>,
    /// Out: the preamble bar was toggled.
    toggle_preamble: bool,
    /// Out: the first focusable part, for F6.
    first: Option<egui::Id>,
    /// Out: a part has the keyboard focus.
    focused: bool,
    /// How paragraphs are aligned inside `center` and `flushright`.
    align: Align,
    /// Footnotes numbered so far.
    footnotes: usize,
    /// The file's formulas, by their first byte, and the document's body size in TeX points.
    typeset: &'a HashMap<usize, FormulaState>,
    /// The formulas of the included files, by file.
    typeset_files: &'a HashMap<ProjectPath, HashMap<usize, FormulaState>>,
    body_pt: f32,
    ink: &'a mut Ink,
    /// Out: the lines of text drawn, in order, for the caret.
    lines: Vec<DrawnLine>,
    /// The caret's source offset while the view has the keyboard, for the empty paragraph it may
    /// stand in.
    caret: Option<usize>,
    /// Bring the line of this source offset into view.
    reveal: Option<usize>,
    /// The caret's source offset, focused or not: the table holding it shows its toolbar; and
    /// the selection's other end.
    edit_caret: Option<usize>,
    edit_anchor: Option<usize>,
    /// Out: a table toolbar's Caption or Label button (the operation, and the table's first byte).
    table_prompt: Option<(&'static str, usize)>,
    /// Give the keyboard to the toolbar of the table holding the caret.
    focus_toolbar: bool,
    /// Out: a table's toolbar holds the keyboard.
    toolbar_focused: bool,
    /// Out: a table toolbar's `table.edit` (its `at` a byte of the text).
    table_action: Option<serde_json::Value>,
    /// Out: a figure's Edit button was pressed (its first byte).
    edit_figure: Option<usize>,
    /// Out: a figure's `figure.edit` (its `at` the figure's first byte), from its handle, its menu
    /// or Add Caption.
    figure_action: Option<serde_json::Value>,
    /// Out: a figure's Label… (its first byte).
    figure_label: Option<usize>,
    /// The misspelled words' bytes, in order.
    misspelled: &'a [Range<usize>],
    /// The grammar problems' bytes, in order.
    grammar: &'a [Range<usize>],
    /// The last build's numbers and the bibliography.
    refs: &'a Refs,
    /// The included files, by the name as written.
    includes: &'a HashMap<String, Included>,
    /// The files whose view this one is drawn inside (an included file's, read-only), outermost
    /// first; empty for the file shown.
    within: Vec<ProjectPath>,
    /// Out: a cross-reference was followed (Ctrl+click): its key.
    follow: Option<String>,
    /// The source editor open in the view, and its formula's preview.
    in_place: Option<&'a mut InPlace>,
    preview: Option<&'a FormulaState>,
    /// Out: the editor was left (Escape, a click outside).
    close_in_place: bool,
    /// Out: a part was clicked or activated to edit it in place.
    open: Option<InPlace>,
    /// Out: an included file's Edit Here was pressed.
    edit_included: Option<ProjectPath>,
}

/// A line of text as drawn: what its characters stand for, its layout and where it is, the slot
/// its selection is painted in (under its text), and its pointer response.
pub(crate) struct DrawnLine {
    pub line: Line,
    pub galley: Arc<Galley>,
    pub origin: Pos2,
    pub slot: egui::layers::ShapeIdx,
    pub response: egui::Response,
    /// An empty paragraph only the caret shows (just made with Enter).
    pub phantom: bool,
    /// The table cell it is.
    pub cell: Option<CellAt>,
    /// Its runs that open in the source editor (formulas, commands shown as source).
    pub atoms: Vec<Atom>,
}

/// The line of the pieces of a paragraph, or an empty one at `empty_at`.
fn line_of(pieces: &[Piece], empty_at: usize) -> Line {
    if pieces.is_empty() {
        return Line::empty(empty_at);
    }
    let parts = pieces
        .iter()
        .map(|p| {
            let chars = p.chars.len();
            match (&p.kind, &p.offsets) {
                (PieceKind::Text, Some(o)) => Part::Text(o.clone()),
                (PieceKind::Text, None) if p.range.is_empty() => Part::Decoration(chars),
                _ => Part::Atom { chars, range: p.range.clone() },
            }
        })
        .collect();
    Line::new(parts)
}

pub(crate) fn show(app: &mut App, ui: &mut egui::Ui, path: &ProjectPath) {
    // An editor left open on another file: its edit goes in now.
    if app.editor.formatted.in_place.as_ref().is_some_and(|ip| &ip.path != path) {
        in_place::close(app);
    }
    // The view is made from the editor's buffer, which it edits: its text is the file's.
    let version = app.editor.source.doc.buffer.version();
    let numbered = app.session.numbered();
    let fresh = app.editor.formatted.cache.as_ref().is_some_and(|c| &c.path == path && c.revision == version && c.theorems == numbered.theorems);
    if !fresh {
        let text: Arc<str> = Arc::from(app.editor.source.doc.buffer.text());
        // The whole file again: files are small, incremental parsing is still to come.
        let blocks = Arc::new(texcraft_latex::view::view_with(&text, &numbered.theorems));
        let summary = match blocks.first() {
            Some(Block { kind: BlockKind::Preamble, range }) => summary(text.get(range.clone()).unwrap_or_default()),
            _ => String::new(),
        };
        let theorems = numbered.theorems.clone();
        app.editor.formatted.cache = Some(Cache { path: path.clone(), revision: version, text, blocks, summary, theorems });
    }
    let Some(cache) = app.editor.formatted.cache.as_ref() else { return };
    let (text, blocks, summary) = (Arc::clone(&cache.text), Arc::clone(&cache.blocks), cache.summary.clone());
    let ctx = ui.ctx().clone();
    let doc_id = text_id(path);
    let focused = ctx.memory(|m| m.has_focus(doc_id));
    let edits = crate::formatted_edit::begin(app, &ctx, doc_id, focused);
    // A jump from the outline, the PDF or the build log scrolls to its line.
    let scroll_to = match app.view.goto.take() {
        Some((p, line)) if &p == path => Some(text.split_inclusive('\n').take(line.saturating_sub(1) as usize).map(str::len).sum()),
        other => {
            app.view.goto = other;
            None
        }
    };
    let includes = includes(app, path, &blocks, version);
    // The formulas of the file and of those shown in it, in one batch.
    let mut files = vec![path.clone()];
    files.extend(includes.values().filter_map(|i| i.as_ref().ok()).map(|(p, ..)| p.clone()).filter(|p| p != path));
    files.sort_by(|a, b| (a != path).cmp(&(b != path)).then_with(|| a.cmp(b)));
    let found = app.session.formulas_of(&files).unwrap_or_default();
    if found.formulas.iter().any(|f| matches!(f.state, FormulaState::Pending)) {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
    }
    let mut typeset_files: HashMap<ProjectPath, HashMap<usize, FormulaState>> = HashMap::new();
    for f in found.formulas {
        typeset_files.entry(f.file).or_default().insert(f.range.start, f.state);
    }
    let typeset = typeset_files.remove(path).unwrap_or_default();
    let preview = match app.editor.formatted.in_place.as_ref() {
        Some(InPlace { what: What::Formula { display, env }, buffer, .. }) if !buffer.trim().is_empty() => {
            let (math, display, env) = (buffer.clone(), *display, env.clone());
            Some(app.session.formula_preview(path, &math, display, env.as_deref()))
        }
        _ => None,
    };
    let t = app.tokens;
    let caret = crate::formatted_edit::caret_byte(app);
    let anchor = {
        let d = &app.editor.source.doc;
        d.buffer.char_to_byte(d.selection.primary().anchor)
    };
    let reveal = std::mem::take(&mut app.editor.formatted.editing.reveal).then_some(caret);
    let vim = app.editor.source.vim.is_some();
    let misspelled = misspellings(app, ui, path);
    let grammar = grammar_problems(app, ui, path);
    let refs = Refs { aux: app.session.aux().cloned(), numbered: Arc::clone(&numbered), cited: cited(app) };
    let ink = &mut app.editor.formatted.ink;
    ink.begin();
    let mut cx = Cx {
        t,
        text: &text,
        project: app.session.project(),
        path,
        images: &mut app.editor.formatted.images,
        preamble_open: app.editor.formatted.preamble_open.contains(path),
        summary: &summary,
        scroll_to,
        jump: None,
        toggle_preamble: false,
        first: None,
        focused: false,
        align: Align::Min,
        footnotes: 0,
        typeset: &typeset,
        typeset_files: &typeset_files,
        body_pt: found.body_pt.max(1.0),
        ink,
        lines: Vec::new(),
        caret: focused.then_some(caret),
        reveal,
        edit_caret: Some(caret),
        edit_anchor: Some(anchor),
        table_prompt: None,
        focus_toolbar: std::mem::take(&mut app.editor.formatted.editing.focus_toolbar),
        toolbar_focused: false,
        table_action: None,
        edit_figure: None,
        figure_action: None,
        figure_label: None,
        misspelled: &misspelled,
        grammar: &grammar,
        refs: &refs,
        includes: &includes,
        within: Vec::new(),
        follow: None,
        in_place: app.editor.formatted.in_place.as_mut(),
        preview: preview.as_ref(),
        close_in_place: false,
        open: None,
        edit_included: None,
    };
    let page = 6.0 * t.unit;
    app.view.cursor = None;
    let (painter, doc_r) = egui::ScrollArea::vertical()
        .id_salt(("formatted", path.as_str()))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let avail = ui.available_width();
            let measure = measure(ui, &t).min(avail - 2.0 * page).max(120.0);
            ui.add_space(page * 1.5);
            let left = ((avail - measure) / 2.0).max(0.0);
            ui.horizontal(|ui| {
                ui.add_space(left);
                ui.vertical(|ui| {
                    ui.set_width(measure);
                    ui.spacing_mut().item_spacing = vec2(0.0, 0.0);
                    if vim {
                        let note = tl!("Vim's keys work in the source; here the standard keys edit the text.");
                        ui.label(egui::RichText::new(note).font(t.small()).color(t.text_muted));
                        ui.add_space(t.reading_size);
                    }
                    draw_blocks(ui, &mut cx, &blocks, 0);
                    // A finger's drag on the text scrolls the view (#26).
                    for d in &cx.lines {
                        crate::touch::scroll(ui, &d.response);
                    }
                });
            });
            ui.add_space(page * 4.0);
            keyboard_scroll(ui, cx.focused, focused);
            // The editable text: it takes the keyboard, the lines take the pointer.
            let had = ui.memory(|m| m.has_focus(doc_id));
            let r = ui.interact(ui.min_rect(), doc_id, Sense::focusable_noninteractive());
            // A click on the text keeps the keyboard there: egui gives it up when a click lands
            // on a widget other than the focused one, and the text's lines are other widgets
            // (#24: the caret showed while the button was down, then went away).
            if had && cx.lines.iter().any(|d| d.response.clicked()) {
                r.request_focus();
            }
            (ui.painter().clone(), r)
        })
        .inner;
    cx.ink.end();
    let (jump, toggle, first, lines) = (cx.jump.take(), cx.toggle_preamble, cx.first, std::mem::take(&mut cx.lines));
    let (table_action, edit_figure, toolbar_focused) = (cx.table_action.take(), cx.edit_figure, cx.toolbar_focused);
    let table_prompt = cx.table_prompt.take();
    let (figure_action, figure_label) = (cx.figure_action.take(), cx.figure_label);
    let follow = cx.follow.take();
    let (close_in_place, open) = (cx.close_in_place, cx.open.take());
    let edit_included = cx.edit_included.take();
    app.region(crate::Region::Editor, if lines.is_empty() { first.unwrap_or(doc_id) } else { doc_id });
    // Escape on a table's toolbar gives the keyboard back to the cell.
    let e = &mut app.editor.formatted.editing;
    if std::mem::replace(&mut e.toolbar_focused, toolbar_focused) && ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        e.focus = true;
    }
    crate::formatted_edit::finish(app, ui, (path, &text), &lines, (&painter, &doc_r), edits);
    if close_in_place {
        in_place::close(app);
    }
    if let Some(ip) = open {
        in_place::open(app, ip);
    }
    in_place::new_formula_dialog(app, &ui.ctx().clone(), path);
    if let Some(action) = table_action {
        run_table_action(app, path, &text, action);
    }
    if let Some((op, at)) = table_prompt {
        float_prompt(app, path, "table.edit", op, at);
    }
    if let Some(at) = figure_label {
        float_prompt(app, path, "figure.edit", "label", at);
    }
    if let Some(action) = figure_action {
        run_figure_action(app, path, action);
    }
    if let Some(key) = follow {
        follow_reference(app, &key);
    }
    if let Some(file) = edit_included {
        app.run("file.open", serde_json::json!({"path": file.as_str()}));
        app.editor.formatted.set(&file, true);
    }
    if let Some(at) = edit_figure {
        crate::figure_dialog::open(app, path, &text, at);
    }
    if toggle && !app.editor.formatted.preamble_open.remove(path) {
        app.editor.formatted.preamble_open.insert(path.clone());
    }
    if let Some(Jump(byte)) = jump {
        to_source(app, ui, path, &text, byte);
    }
}

/// The project's bibliography entries, each key's author and year, made again when the project
/// changes.
fn cited(app: &mut App) -> Arc<HashMap<String, String>> {
    let revision = app.session.revision();
    if let Some((r, found)) = &app.editor.formatted.cited
        && *r == revision
    {
        return Arc::clone(found);
    }
    let project = app.session.project();
    let mut found = HashMap::new();
    for path in project.paths().filter(|p| p.extension() == "bib") {
        for e in project.text(path).map(texcraft_latex::refs::entries).unwrap_or_default() {
            found.insert(e.key.clone(), texcraft_latex::refs::author_year(&e));
        }
    }
    let found = Arc::new(found);
    app.editor.formatted.cited = Some((revision, Arc::clone(&found)));
    found
}

/// The files the view of `path` includes, read again when the project or the buffer changes:
/// each `\input` and `\include` found (in those files too), by its name as written.
fn includes(app: &mut App, path: &ProjectPath, blocks: &[Block], version: u64) -> Arc<HashMap<String, Included>> {
    let of = (app.session.revision(), version);
    if let Some(found) = app.editor.formatted.includes.as_ref().filter(|i| i.of == of) {
        return Arc::clone(&found.files);
    }
    let theorems = app.session.numbered().theorems.clone();
    let mut files: HashMap<String, Included> = HashMap::new();
    let mut seen: HashSet<ProjectPath> = HashSet::from([path.clone()]);
    let mut todo: Vec<String> = Vec::new();
    let names = |blocks: &[Block], todo: &mut Vec<String>| {
        texcraft_latex::view::walk(blocks, &mut |b| {
            if let BlockKind::Include { name, .. } = &b.kind {
                todo.push(name.clone());
            }
        });
    };
    names(blocks, &mut todo);
    // A file reads at most a few levels of files; one that includes itself stops there.
    while let Some(name) = todo.pop() {
        if files.contains_key(&name) || files.len() > 64 {
            continue;
        }
        let found = match app.session.included(&name) {
            None => Err(name.clone()),
            Some(file) => match app.session.project().text(&file) {
                Ok(text) => {
                    let text: Arc<str> = Arc::from(text);
                    let blocks = Arc::new(texcraft_latex::view::view_with(&text, &theorems));
                    if seen.insert(file.clone()) {
                        names(&blocks, &mut todo);
                    }
                    Ok((file, text, blocks))
                }
                Err(_) => Err(name.clone()),
            },
        };
        files.insert(name, found);
    }
    let files = Arc::new(files);
    app.editor.formatted.includes = Some(Includes { of, files: Arc::clone(&files) });
    files
}

/// Goes to the label `key` names: in this file or another of the project's, its caret there.
fn follow_reference(app: &mut App, key: &str) {
    let project = app.session.project();
    let found = project.paths().filter(|p| p.extension() == "tex").find_map(|p| {
        project.text(p).ok().and_then(|t| texcraft_latex::refs::labels(t).into_iter().find(|l| l.key == key)).map(|l| (p.clone(), l.line))
    });
    let Some((path, line)) = found else {
        app.view.status = Some((tl!("No label {key} in the project's files.").replace("{key}", key), true));
        return;
    };
    if app.session.current() != Some(&path) {
        app.run("file.open", serde_json::json!({"path": path.as_str()}));
        app.editor.formatted.set(&path, true);
    } else {
        let b = &app.editor.source.doc.buffer;
        let at = b.line_to_char((line as usize).min(b.len_lines().saturating_sub(1)));
        app.editor.source.doc.selection = texcraft_buffer::Selection::single(at, at);
        app.editor.formatted.editing.reveal = true;
        app.editor.formatted.editing.focus = true;
    }
    app.view.goto = Some((path, line + 1));
}

/// The misspelled words of the shown file, found again when it changes and at most each second
/// otherwise (a word added to the dictionary).
fn misspellings(app: &mut App, ui: &egui::Ui, path: &ProjectPath) -> Arc<Vec<Range<usize>>> {
    if !crate::spell::checked(path) {
        return Arc::default();
    }
    let now = ui.input(|i| i.time);
    let revision = app.session.revision();
    if let Some((r, at, found)) = &app.editor.formatted.misspelled
        && *r == revision
        && now - at < 1.0
    {
        return Arc::clone(found);
    }
    let found: Arc<Vec<Range<usize>>> = Arc::new(app.session.misspellings(path, None).unwrap_or_default().into_iter().map(|m| m.range).collect());
    app.editor.formatted.misspelled = Some((revision, now, Arc::clone(&found)));
    found
}

/// The grammar problems of the shown file, as the misspelled words are found; the view repaints
/// while passages are checked in the background.
fn grammar_problems(app: &mut App, ui: &egui::Ui, path: &ProjectPath) -> Misspelled {
    if !crate::spell::checked(path) {
        return Arc::default();
    }
    let now = ui.input(|i| i.time);
    let revision = app.session.revision();
    let busy = app.session.grammar_busy();
    if busy {
        ui.ctx().request_repaint_after(std::time::Duration::from_millis(150));
    }
    if let Some((r, at, found)) = &app.editor.formatted.grammar
        && *r == revision
        && (now - at < 1.0 || (!busy && now - at < 5.0))
    {
        return Arc::clone(found);
    }
    let found: Misspelled = Arc::new(app.session.grammar_problems(path, None).unwrap_or_default().into_iter().map(|g| g.range).collect());
    app.editor.formatted.grammar = Some((revision, now, Arc::clone(&found)));
    found
}

/// A line under the marked words of a paragraph's text pieces: wavy (misspelled) or double
/// (grammar), in `color`.
fn paint_marks(
    ui: &egui::Ui,
    (color, double): (Color32, bool),
    galley: &Galley,
    pieces: &[Piece],
    origin: Pos2,
    (text, misspelled): (&str, &[Range<usize>]),
) {
    if misspelled.is_empty() {
        return;
    }
    for p in pieces {
        let (PieceKind::Text, Some(offsets)) = (&p.kind, &p.offsets) else { continue };
        let from = misspelled.partition_point(|m| m.end <= p.range.start);
        for m in misspelled[from..].iter().take_while(|m| m.start < p.range.end) {
            // The view's text is the editor's: a word the session has not seen yet is not marked.
            if text.get(m.clone()).is_none() {
                continue;
            }
            let inside: Vec<usize> = offsets
                .iter()
                .take(offsets.len().saturating_sub(1))
                .enumerate()
                .filter(|(_, o)| m.start <= **o && **o < m.end)
                .map(|(k, _)| k)
                .collect();
            let (Some(first), Some(last)) = (inside.first(), inside.last()) else { continue };
            for r in span_rects(galley, (p.chars.start + first, p.chars.start + last + 1)) {
                let r = r.translate(origin.to_vec2());
                let y = r.bottom() - 2.0;
                if double {
                    for dy in [-1.5, 0.5] {
                        ui.painter().hline(r.x_range(), y + dy, Stroke::new(1.0, color));
                    }
                    continue;
                }
                let step = 2.5;
                let points: Vec<Pos2> = (0..=(r.width() / step).max(1.0) as usize)
                    .map(|i| pos2(r.left() + i as f32 * step, y + if i % 2 == 0 { -1.2 } else { 1.2 }))
                    .collect();
                ui.painter().add(egui::Shape::line(points, Stroke::new(1.0, color)));
            }
        }
    }
}

/// Runs a table toolbar's `table.edit`, its position made characters, after the view's own edits
/// went to the session; says what the result notes (a package the document does not load).
fn run_table_action(app: &mut App, path: &ProjectPath, text: &str, mut action: serde_json::Value) {
    let byte = action["at"].as_u64().unwrap_or(0) as usize;
    action["at"] = serde_json::json!(text.get(..byte + 1).map_or(0, |s| s.chars().count()));
    action["path"] = serde_json::json!(path.as_str());
    crate::editor::send_edits(app);
    if let Some(done) = app.run("table.edit", action)
        && done.get("loaded").and_then(serde_json::Value::as_bool) == Some(false)
    {
        let main = app.session.project().main().map_or_else(|| path.file_name().to_owned(), |m| m.as_str().to_owned());
        let said = tl!("Booktabs rules need the booktabs package: add \\usepackage{booktabs} to the preamble of {main}.").replace("{main}", &main);
        app.view.status = Some((said, true));
    }
    app.editor.formatted.editing.focus = true;
}

/// Runs a figure's `figure.edit`, its position made characters; with `caret_in_caption`, the
/// caret then goes in the caption it added.
fn run_figure_action(app: &mut App, path: &ProjectPath, mut action: serde_json::Value) {
    crate::editor::send_edits(app);
    let Ok(text) = app.session.project().text(path).map(str::to_owned) else { return };
    let byte = action["at"].as_u64().unwrap_or(0) as usize + 1;
    let caret_in_caption = action.as_object_mut().and_then(|o| o.remove("caret_in_caption")).is_some();
    action["at"] = serde_json::json!(text.get(..byte).map_or(0, |s| s.chars().count()));
    action["path"] = serde_json::json!(path.as_str());
    if app.run("figure.edit", action).is_none() || !caret_in_caption {
        app.editor.formatted.editing.focus = true;
        return;
    }
    let Ok(text) = app.session.project().text(path).map(str::to_owned) else { return };
    if let Some((inner, _)) = texcraft_latex::figure::find(&text, byte).and_then(|f| f.caption) {
        let at = text.get(..inner.start).map_or(0, |s| s.chars().count());
        crate::editor::select_after_edit(app, path, at, at);
    }
    app.editor.formatted.editing.focus = true;
    app.editor.formatted.editing.reveal = true;
}

/// Asks for a table's or a figure's caption or label (`op`), starting from the one it has, then
/// runs `command` (`table.edit`, `figure.edit`); `at` is a byte inside the table or figure.
fn float_prompt(app: &mut App, path: &ProjectPath, command: &'static str, op: &'static str, at: usize) {
    crate::editor::send_edits(app);
    let Ok(text) = app.session.project().text(path).map(str::to_owned) else { return };
    let table = command == "table.edit";
    let float = if table { texcraft_latex::figure::find_table_float(&text, at) } else { texcraft_latex::figure::find(&text, at + 1) };
    let src = |r: &Range<usize>| text.get(r.clone()).unwrap_or_default().to_owned();
    let (title, label) = match (table, op) {
        (true, "caption") => (tl!("Table Caption"), tl!("The caption, in LaTeX")),
        (true, _) => (tl!("Table Label"), tl!("The label references use, such as tab:results")),
        (false, _) => (tl!("Figure Label"), tl!("The label references use, such as fig:plot")),
    };
    let value = if op == "caption" {
        float.as_ref().and_then(|f| f.caption.as_ref()).map(|c| src(&c.0)).unwrap_or_default()
    } else {
        float.as_ref().and_then(|f| f.label.as_ref()).map(&src).unwrap_or_default()
    };
    let chars = text.get(..at + 1).map_or(0, |s| s.chars().count());
    let file = path.as_str().to_owned();
    let params = move |v: &str| {
        if table { serde_json::json!({"path": file, "at": chars, "op": op, op: v}) } else { serde_json::json!({"path": file, "at": chars, op: v}) }
    };
    let prompt = crate::dialogs::Prompt { title, label, value, action: tl!("Apply"), command, params: Box::new(params), focus: true };
    app.view.dialog = Some(crate::dialogs::Dialog::Prompt(prompt));
}

/// The id of the formatted view's editable text for `path`.
pub(crate) fn text_id(path: &ProjectPath) -> egui::Id {
    egui::Id::unique(("formatted-text", path.as_str()))
}

/// Page Up, Page Down, Home and End scroll the view while one of its parts has the focus; Page Up
/// and Page Down while its text does (Home and End move the caret there).
fn keyboard_scroll(ui: &mut egui::Ui, part: bool, text: bool) {
    if !part && !text {
        return;
    }
    let page = ui.clip_rect().height() * 0.85;
    let delta = ui.input(|i| {
        if i.key_pressed(egui::Key::PageDown) {
            -page
        } else if i.key_pressed(egui::Key::PageUp) {
            page
        } else if part && i.key_pressed(egui::Key::End) {
            -1.0e7
        } else if part && i.key_pressed(egui::Key::Home) {
            1.0e7
        } else {
            0.0
        }
    });
    if delta != 0.0 {
        ui.scroll_with_delta(vec2(0.0, delta));
    }
}

/// Shows the source of `path` with the caret at byte `byte`.
/// Whether the current file is shown in the formatted view.
pub(crate) fn shown(app: &App) -> bool {
    app.session.current().is_some_and(|p| app.editor.formatted.shows(p) && available(p))
}

/// Goes from the formatted view to the source, the caret and the selection where they were
/// (`view.show_in_source`): the two views share them.
pub(crate) fn show_in_source(app: &mut App) {
    let Some(path) = app.session.current().cloned() else { return };
    app.editor.formatted.set(&path, false);
    let now = app.ctx.input(|i| i.time);
    let source = &mut app.editor.source;
    let line = source.doc.buffer.char_to_line(source.doc.selection.primary().head.min(source.doc.buffer.len_chars()));
    source.flash_line(line, now);
    source.reveal = true;
    app.editor.focus = true;
}

fn to_source(app: &mut App, ui: &egui::Ui, path: &ProjectPath, text: &str, byte: usize) {
    app.editor.formatted.set(path, false);
    let at = text.get(..byte).map_or(0, |s| s.chars().count());
    let source = &mut app.editor.source;
    let at = at.min(source.doc.buffer.len_chars());
    source.select(at, at);
    let line = source.doc.buffer.char_to_line(at);
    source.flash_line(line, ui.input(|i| i.time));
    source.reveal = true;
    app.editor.focus = true;
}

/// The reading measure: about `reading_measure` average characters of the reading face.
fn measure(ui: &egui::Ui, t: &Tokens) -> f32 {
    let sample = "abcdefghijklmnopqrstuvwxyz";
    let width = ui.painter().layout_no_wrap(sample.to_owned(), t.reading(t.reading_size, false, false), t.text).size().x;
    width / sample.len() as f32 * t.reading_measure
}

fn draw_blocks(ui: &mut egui::Ui, cx: &mut Cx<'_>, blocks: &[Block], depth: usize) {
    for (i, block) in blocks.iter().enumerate() {
        if i > 0 {
            ui.add_space(gap_before(&cx.t, &block.kind));
        }
        if let Some(prev) = i.checked_sub(1).and_then(|p| blocks.get(p)) {
            phantom(ui, cx, (block_end(prev), block.range.start), false);
        }
        let top = ui.cursor().top();
        draw_block(ui, cx, block, depth);
        if let Some(target) = cx.scroll_to
            && block.range.end > target
        {
            let rect = Rect::from_x_y_ranges(ui.max_rect().x_range(), top..=ui.cursor().top().max(top + 1.0));
            ui.scroll_to_rect(rect, Some(Align::TOP));
            cx.scroll_to = None;
        }
    }
    if let Some(last) = blocks.last() {
        phantom(ui, cx, (block_end(last), cx.text.len()), true);
    }
}

/// The space above a block, after another.
fn gap_before(t: &Tokens, kind: &BlockKind) -> f32 {
    let em = t.reading_size;
    match kind {
        BlockKind::Heading { level, .. } if *level <= 3 => em * 1.75,
        BlockKind::Heading { .. } => em * 1.25,
        BlockKind::Figure { .. }
        | BlockKind::Formula { .. }
        | BlockKind::Source
        | BlockKind::Container { .. }
        | BlockKind::Theorem { .. }
        | BlockKind::Include { .. }
        | BlockKind::Table(_)
        | BlockKind::UnsupportedTable(_) => em,
        _ => em * 0.75,
    }
}

fn draw_block(ui: &mut egui::Ui, cx: &mut Cx<'_>, block: &Block, depth: usize) {
    let range = &block.range;
    match &block.kind {
        BlockKind::Preamble => preamble(ui, cx, range),
        BlockKind::Title { title, author, date } => title_block(ui, cx, range, [title, author, date]),
        BlockKind::Heading { level, content, label, .. } => heading(ui, cx, range, *level, content, label.as_ref()),
        BlockKind::Paragraph(inlines) => {
            let size = cx.t.reading_size;
            let _ = paragraph(
                ui,
                cx,
                inlines,
                range,
                &Para { size, bold: false, color: cx.t.text, role: egui::Role::Paragraph, empty_at: None, cell: None },
            );
        }
        BlockKind::List { kind, items } => list(ui, cx, *kind, items, depth),
        BlockKind::Container { name, blocks } => container(ui, cx, name, blocks, depth),
        BlockKind::Theorem { kind, title, blocks, .. } => theorem(ui, cx, range, kind, title, blocks, depth),
        BlockKind::Include { name, .. } => include(ui, cx, range, name, depth),
        BlockKind::Formula { source, env, closed } => formula(ui, cx, range, source, env.as_deref(), *closed),
        BlockKind::Figure { image, caption, label } => fig::card(ui, cx, range, image.as_ref(), caption, label.as_ref()),
        BlockKind::Table(tb) => table::table_block(ui, cx, range, tb),
        BlockKind::UnsupportedTable(why) => table::unsupported(ui, cx, range, why),
        BlockKind::Source => source_block(ui, cx, range),
    }
}

/// How a paragraph is set.
struct Para {
    size: f32,
    bold: bool,
    color: Color32,
    role: egui::Role,
    /// Where the caret stands in it when it shows nothing (an empty item).
    empty_at: Option<usize>,
    /// The table cell it is.
    cell: Option<CellAt>,
}

/// A run of a laid-out paragraph: the characters it takes in the galley and what it stands for.
struct Piece {
    chars: Range<usize>,
    range: Range<usize>,
    kind: PieceKind,
    /// For text: the source each of its characters stands for ([`edit::offsets`]).
    offsets: Option<Vec<usize>>,
    /// For a formula: its math, and whether it is displayed.
    math: Option<(Range<usize>, bool)>,
}

enum PieceKind {
    Text,
    /// A line break (`\\`).
    Break,
    /// A chip: its kind, its command, how it is shown.
    Chip(ChipKind, String, Chip),
    /// A formula shown as its source: being typeset (true), or this build cannot typeset it.
    Formula(bool),
    /// A typeset formula, drawn over the space its no-break space holds.
    Typeset(FormulaBox),
    /// A formula retex could not typeset: its source, underlined, and retex's messages.
    FormulaError(String),
    Source,
    Link(String),
    Footnote(String),
}

/// Builds a paragraph's layout job.
struct Builder<'a> {
    t: &'a Tokens,
    text: &'a str,
    size: f32,
    bold: bool,
    color: Color32,
    job: LayoutJob,
    pieces: Vec<Piece>,
    chars: usize,
    /// The paragraph as words, for its accessible name.
    plain: String,
    footnotes: &'a mut usize,
    typeset: &'a HashMap<usize, FormulaState>,
    refs: &'a Refs,
    /// Screen points per TeX point.
    scale: f32,
    /// The reading face's no-break space at this size: its advance, and how far the baseline is
    /// below the middle of its line.
    probe: (f32, f32),
}

/// White space as single spaces.
fn collapse(s: &str) -> String {
    s.split_whitespace().collect::<Vec<_>>().join(" ")
}

impl Builder<'_> {
    fn line_height(&self) -> f32 {
        (self.size * self.t.reading_leading).round()
    }

    fn format(&self, style: Style, color: Color32) -> TextFormat {
        let bold = style.bold || self.bold;
        let size = if style.small_caps { self.size * 0.82 } else { self.size };
        let font = if style.mono {
            FontId::new(size * 0.88, FontFamily::Monospace)
        } else if style.sans {
            FontId::new(size * 0.94, if bold { crate::fonts::semibold() } else { FontFamily::Proportional })
        } else {
            self.t.reading(size, bold, style.italic && !bold)
        };
        TextFormat {
            font_id: font,
            color,
            line_height: Some(self.line_height()),
            italics: style.italic && bold,
            underline: if style.underline { Stroke::new(1.0, color) } else { Stroke::NONE },
            valign: Align::Center,
            ..TextFormat::default()
        }
    }

    fn append(&mut self, s: &str, format: TextFormat, range: Range<usize>, kind: PieceKind) {
        if s.is_empty() {
            return;
        }
        let start = self.chars;
        self.chars += s.chars().count();
        self.job.append(s, 0.0, format);
        self.pieces.push(Piece { chars: start..self.chars, range, kind, offsets: None, math: None });
    }

    /// A no-break space as wide as typeset formula `b`, in a line tall enough for its height above
    /// and depth below the baseline it shares with the text.
    fn typeset_space(&mut self, b: &FormulaBox, range: Range<usize>) {
        let (advance, below_middle) = self.probe;
        let (w, h, d) = (b.width_pt * self.scale, b.height_pt * self.scale, b.depth_pt * self.scale);
        let gap = self.size * 0.15;
        let line = self.line_height().max(2.0 * (h - below_middle) + gap).max(2.0 * (d + below_middle) + gap);
        let format = TextFormat {
            font_id: self.t.reading(self.size, false, false),
            color: Color32::TRANSPARENT,
            line_height: Some(line.ceil()),
            extra_letter_spacing: w - advance,
            valign: Align::Center,
            ..TextFormat::default()
        };
        self.append("\u{a0}", format, range, PieceKind::Typeset(b.clone()));
    }

    fn atom_format(&self, font: FontId, color: Color32) -> TextFormat {
        TextFormat { font_id: font, color, line_height: Some(self.line_height()), valign: Align::Center, ..TextFormat::default() }
    }

    fn inline(&mut self, inline: &Inline, outer: Style) {
        let range = inline.range.clone();
        match &inline.kind {
            InlineKind::Text { text, style } => {
                let style = merge(outer, *style);
                let shown = if style.small_caps { text.to_uppercase() } else { text.clone() };
                self.plain.push_str(text);
                let format = self.format(style, self.color);
                let offsets = edit::offsets(self.text, range.clone(), text).filter(|o| o.len() == shown.chars().count() + 1);
                self.append(&shown, format, range, PieceKind::Text);
                if let Some(p) = self.pieces.last_mut() {
                    p.offsets = offsets;
                }
            }
            InlineKind::LineBreak => {
                self.plain.push(' ');
                let format = self.format(outer, self.color);
                self.append("\n", format, range, PieceKind::Break);
            }
            InlineKind::Formula { source, display, .. } => {
                let math = collapse(self.text.get(source.clone()).unwrap_or_default());
                self.plain.push_str(&math);
                let kind = match self.typeset.get(&range.start) {
                    Some(FormulaState::Typeset(b)) => {
                        self.typeset_space(b, range);
                        if let Some(p) = self.pieces.last_mut() {
                            p.math = Some((source.clone(), *display));
                        }
                        return;
                    }
                    Some(FormulaState::Failed(messages)) => PieceKind::FormulaError(messages.join("\n")),
                    Some(FormulaState::Pending) => PieceKind::Formula(true),
                    Some(FormulaState::Unavailable(_)) | None => PieceKind::Formula(false),
                };
                let font = FontId::new(self.t.code_size * self.size / self.t.reading_size * 0.95, FontFamily::Monospace);
                let format = self.atom_format(font, self.t.syntax_math);
                self.append(&format!("\u{a0}{math}\u{a0}"), format, range, kind);
                if let Some(p) = self.pieces.last_mut() {
                    p.math = Some((source.clone(), *display));
                }
            }
            InlineKind::Chip { kind, command, key } => {
                let (chip, words) = chip(self.refs, *kind, command, key);
                let font = FontId::new(self.size * 0.8, FontFamily::Proportional);
                let color = if matches!(chip, Chip::Missing(_)) { self.t.text_muted } else { self.t.text };
                let format = self.atom_format(font, color);
                // No-break spaces hold the chip's icon (after the first) and keep it on one line.
                let icon = if matches!(chip, Chip::Label | Chip::File) { "\u{a0}".repeat(CHIP_ICON_SLOT) } else { String::new() };
                let shown = match chip {
                    // What the PDF shows, at the text's size, in a quiet pill.
                    Chip::Shown => {
                        let format = TextFormat { font_id: self.format(outer, self.color).font_id, ..format };
                        self.plain.push_str(&words);
                        self.append(
                            &format!("\u{a0}{}\u{a0}", words.replace(' ', "\u{a0}")),
                            format,
                            range,
                            PieceKind::Chip(*kind, command.clone(), chip),
                        );
                        return;
                    }
                    Chip::Missing(_) => format!("{}\u{a0}?", key.replace(' ', "\u{a0}")),
                    Chip::Label => String::new(),
                    Chip::File => key.replace(' ', "\u{a0}"),
                };
                self.plain.push_str(&words);
                let shown = format!("\u{a0}{icon}{shown}\u{a0}");
                self.append(&shown, format, range, PieceKind::Chip(*kind, command.clone(), chip));
            }
            InlineKind::Footnote(inner) => {
                *self.footnotes += 1;
                let font = FontId::new(self.size * 0.65, crate::fonts::semibold());
                let format = TextFormat { valign: Align::TOP, ..self.atom_format(font, self.t.accent) };
                let note = plain(self.text, inner);
                self.append(&format!("\u{200a}{}", self.footnotes), format, range, PieceKind::Footnote(note));
            }
            InlineKind::Link { url, text } => {
                let start = self.chars;
                let color = self.color;
                self.color = self.t.accent;
                let style = Style { underline: true, ..outer };
                for i in text {
                    self.inline(i, style);
                }
                self.color = color;
                self.pieces.retain(|p| p.chars.start < start);
                self.pieces.push(Piece { chars: start..self.chars, range, kind: PieceKind::Link(url.clone()), offsets: None, math: None });
            }
            InlineKind::Source => {
                let src = collapse(self.text.get(range.clone()).unwrap_or_default());
                self.plain.push_str(&src);
                let font = FontId::new(self.t.code_size * self.size / self.t.reading_size * 0.95, FontFamily::Monospace);
                let format = self.atom_format(font, self.t.text_muted);
                self.append(&format!("\u{a0}{src}\u{a0}"), format, range, PieceKind::Source);
            }
        }
    }
}

/// `inner` applied inside `outer`.
fn merge(outer: Style, inner: Style) -> Style {
    Style {
        bold: outer.bold || inner.bold,
        italic: outer.italic != inner.italic && (outer.italic || inner.italic),
        underline: outer.underline || inner.underline,
        mono: outer.mono || inner.mono,
        sans: outer.sans || inner.sans,
        small_caps: outer.small_caps || inner.small_caps,
    }
}

/// The words of inline runs, for tooltips and accessible names.
fn plain(text: &str, inlines: &[Inline]) -> String {
    let mut out = String::new();
    for i in inlines {
        match &i.kind {
            InlineKind::Text { text, .. } => out.push_str(text),
            InlineKind::LineBreak => out.push(' '),
            InlineKind::Formula { source, .. } => out.push_str(&collapse(text.get(source.clone()).unwrap_or_default())),
            InlineKind::Chip { kind: ChipKind::Label, .. } => {}
            InlineKind::Chip { key, .. } => out.push_str(key),
            InlineKind::Footnote(_) => {}
            InlineKind::Link { text: inner, .. } => out.push_str(&plain(text, inner)),
            InlineKind::Source => out.push_str(&collapse(text.get(i.range.clone()).unwrap_or_default())),
        }
    }
    out
}

/// The rectangles chars `s..e` of `galley` take, one per row.
pub(crate) fn span_rects(galley: &Galley, (s, e): (usize, usize)) -> Vec<Rect> {
    let mut out = Vec::new();
    let mut row_start = 0;
    for row in &galley.rows {
        let n = row.char_count_excluding_newline().0;
        let (a, z) = (s.max(row_start), e.min(row_start + n));
        if a < z {
            let x0 = row.pos.x + row.x_offset(egui::text::CharIndex(a - row_start));
            let x1 = row.pos.x + row.x_offset(egui::text::CharIndex(z - row_start));
            out.push(Rect::from_min_max(pos2(x0.min(x1), row.min_y()), pos2(x0.max(x1), row.max_y())));
        }
        row_start += row.char_count_including_newline().0;
    }
    out
}

/// The piece under `pos` (relative to the galley).
fn piece_at<'p>(galley: &Galley, pieces: &'p [Piece], pos: Vec2) -> Option<&'p Piece> {
    let at = galley.cursor_from_pos(pos).index.0;
    // The cursor sits between characters: take the one it is closest to that is under the pointer.
    let inside = |p: &&Piece| span_rects(galley, (p.chars.start, p.chars.end)).iter().any(|r| r.expand2(vec2(1.0, 0.0)).contains(pos.to_pos2()));
    pieces.iter().filter(|p| p.chars.start <= at && at <= p.chars.end).find(inside)
}

/// How a chip is shown and its words (for the paragraph's name): what the PDF shows for a
/// resolved reference or citation; the key and why it is unresolved otherwise.
fn chip(refs: &Refs, kind: ChipKind, command: &str, key: &str) -> (Chip, String) {
    let resolved = match kind {
        ChipKind::Label => return (Chip::Label, String::new()),
        ChipKind::File => return (Chip::File, key.to_owned()),
        ChipKind::Reference => Aux::reference(refs.aux.as_deref(), command, key),
        ChipKind::Citation => Aux::citation(refs.aux.as_deref(), refs.numbered.cite_style.unwrap_or(CiteStyle::Plain), command, key),
    };
    match resolved {
        Ok(shown) => (Chip::Shown, shown.replace('\u{a0}', " ")),
        Err(why) => {
            let words = match why {
                Missing::NotBuilt => tl!("Not built yet: the number shows after the document is built.").to_owned(),
                Missing::Unknown(k) if kind == ChipKind::Citation => tl!("No bibliography entry named {key} in the last build.").replace("{key}", &k),
                Missing::Unknown(k) => tl!("No label named {key} in the last build.").replace("{key}", &k),
                Missing::Biblatex => tl!("The labels biblatex gives citations are not shown yet.").to_owned(),
                Missing::Silent => tl!("\\nocite lists the entry in the bibliography; nothing shows in the text.").to_owned(),
            };
            (Chip::Missing(words), format!("{key}?"))
        }
    }
}

fn chip_icon(kind: ChipKind) -> &'static str {
    match kind {
        ChipKind::Label => "tag",
        ChipKind::Reference => "link",
        ChipKind::Citation => "book-open",
        ChipKind::File => "file-text",
    }
}

/// What hovering a piece explains.
fn piece_tip(kind: &PieceKind, text: &str, range: &Range<usize>, cited: &HashMap<String, String>) -> Option<String> {
    let why = match kind {
        PieceKind::Chip(_, _, Chip::Missing(why)) => Some(why.clone()),
        _ => None,
    };
    let key = || {
        let src = text.get(range.clone()).unwrap_or_default();
        src.split_once('{').map_or(src, |(_, k)| k.trim_end_matches('}')).to_owned()
    };
    Some(match kind {
        PieceKind::Text | PieceKind::Break => return None,
        PieceKind::Chip(ChipKind::Label, ..) => tl!("Label {key}").replace("{key}", &key()),
        PieceKind::Chip(ChipKind::Reference, command, _) => {
            let said = tl!("Reference (\\{command}) to {key}").replace("{command}", command).replace("{key}", &key());
            match why {
                Some(why) => format!("{said}. {why} {}", tl!("Ctrl+click to go to it.")),
                None => format!("{said}. {}", tl!("Ctrl+click to go to it.")),
            }
        }
        PieceKind::Chip(ChipKind::Citation, command, _) => {
            let said = tl!("Citation (\\{command}) of {key}").replace("{command}", command).replace("{key}", &key());
            // Each key's entry, as an author-year reader sees it.
            let who: Vec<&str> = key().split(',').filter_map(|k| cited.get(k.trim()).map(String::as_str)).collect();
            let said = if who.is_empty() { said } else { format!("{said}: {}", who.join("; ")) };
            match why {
                Some(why) => format!("{said}. {why}"),
                None => said,
            }
        }
        PieceKind::Chip(ChipKind::File, command, _) => {
            tl!("File read with \\{command}: {key}").replace("{command}", command).replace("{key}", &key())
        }
        PieceKind::Formula(true) => tl!("The formula's source, while it is typeset. Click to edit it here.").to_owned(),
        PieceKind::Formula(false) => {
            tl!("The formula's source: this version typesets formulas in the desktop app only. Click to edit it here.").to_owned()
        }
        PieceKind::Typeset(_) => tl!("A formula. Click, or select it and press Enter, to edit it here.").to_owned(),
        PieceKind::FormulaError(message) => tl!("This formula has an error: {message}").replace("{message}", message),
        PieceKind::Source => {
            tl!("Shown as its source: the formatted view does not show this command yet. Click, or select it and press Enter, to edit it here.")
                .to_owned()
        }
        PieceKind::Link(url) => url.clone(),
        PieceKind::Footnote(note) => tl!("Footnote: {text}").replace("{text}", note),
    })
}

/// A laid-out paragraph with its pills, icons, tooltips, focus and jumps. Returns its response.
fn paragraph(ui: &mut egui::Ui, cx: &mut Cx<'_>, inlines: &[Inline], range: &Range<usize>, para: &Para) -> egui::Response {
    let t = cx.t;
    let width = ui.available_width();
    let scale = para.size / cx.body_pt;
    let typeset = cx.typeset;
    let mut b = Builder {
        t: &t,
        text: cx.text,
        size: para.size,
        bold: para.bold,
        color: para.color,
        job: LayoutJob::default(),
        pieces: Vec::new(),
        chars: 0,
        plain: String::new(),
        footnotes: &mut cx.footnotes,
        typeset,
        refs: cx.refs,
        scale,
        probe: probe(ui, &t, para.size),
    };
    for i in inlines {
        b.inline(i, Style::default());
    }
    if b.job.is_empty() {
        // An empty line (an empty list item) is laid out a line tall at the text's size, so its
        // caret and selection stand in that line, not half a line above it.
        let format = b.format(Style::default(), para.color);
        b.job.append("", 0.0, format);
    }
    let (mut job, pieces, plain_text) = (b.job, b.pieces, b.plain);
    job.wrap.max_width = width;
    job.halign = cx.align;
    let galley = ui.painter().layout_job(job);
    let line = (para.size * t.reading_leading).round();
    let (rect, r) = ui.allocate_exact_size(vec2(width, galley.size().y.max(line)), Sense::CLICK | Sense::DRAG);
    let origin = match cx.align {
        Align::Min => rect.min - galley.rect.min.to_vec2(),
        Align::Center => pos2(rect.center().x - galley.rect.center().x, rect.top() - galley.rect.top()),
        Align::Max => pos2(rect.right() - galley.rect.right(), rect.top() - galley.rect.top()),
    };
    // The selection goes under the text.
    let slot = ui.painter().add(egui::Shape::Noop);
    if ui.is_rect_visible(rect) {
        paint_pieces(ui, &t, &galley, &pieces, origin, para.size);
        ui.painter().galley(origin, Arc::clone(&galley), t.text);
        paint_marks(ui, (t.spelling, false), &galley, &pieces, origin, (cx.text, cx.misspelled));
        paint_marks(ui, (t.grammar, true), &galley, &pieces, origin, (cx.text, cx.grammar));
        paint_chip_icons(ui, &t, &galley, &pieces, origin, para.size);
        paint_typeset(ui, cx, &galley, &pieces, origin, (scale, para.color));
    }
    let hovered = r.hover_pos().and_then(|p| piece_at(&galley, &pieces, p - origin));
    // Ctrl+click on a cross-reference goes to its label.
    if r.clicked()
        && ui.input(|i| i.modifiers.command)
        && let Some(p) = hovered.filter(|p| matches!(p.kind, PieceKind::Chip(ChipKind::Reference, ..)))
    {
        let src = cx.text.get(p.range.clone()).unwrap_or_default();
        cx.follow = src.split_once('{').map(|(_, k)| k.trim_end_matches('}').trim().to_owned());
    }
    let r = match hovered.and_then(|p| piece_tip(&p.kind, cx.text, &p.range, &cx.refs.cited)) {
        Some(tip) => r.on_hover_text_at_pointer(tip),
        None => r,
    };
    let atoms: Vec<Atom> = pieces.iter().filter_map(atom_of).collect();
    // A click on a formula or a command shown as source edits it here.
    let clicked_atom = hovered.and_then(atom_of).filter(|_| (r.clicked() || r.double_clicked()) && !ui.input(|i| i.modifiers.command));
    if let Some(atom) = &clicked_atom {
        cx.open = Some(InPlace::of_atom(cx.path, cx.text, atom));
    } else if r.double_clicked() && ui.input(|i| i.modifiers.command) {
        // Ctrl+double-click (Cmd on a Mac) on a run goes to where that run's source starts; a
        // plain double-click selects the word (`formatted_edit`).
        cx.jump = Some(Jump(hovered.map_or(range.start, |p| p.range.start)));
    }
    let role = para.role;
    r.widget_info(|| egui::WidgetInfo::labeled(role, true, plain_text.trim()));
    let mut line = line_of(&pieces, para.empty_at.unwrap_or(range.start));
    if let Some(at) = para.empty_at.filter(|_| line.first_stop().is_none()) {
        // Only what the view adds ("Figure."): the caret stands after it, at `at`.
        let mut parts = line.parts().to_vec();
        parts.push(Part::Text(vec![at]));
        line = Line::new(parts);
    }
    if let Some(byte) = cx.reveal
        && line.span().is_some_and(|s| s.start <= byte && byte <= s.end)
    {
        let at = line.view_at(byte).unwrap_or(0);
        let caret = galley.pos_from_cursor(egui::text::CCursor::new(at)).translate(origin.to_vec2());
        ui.scroll_to_rect(caret.expand(para.size), None);
        cx.reveal = None;
    }
    let under = in_place::under(cx, &atoms);
    cx.lines.push(DrawnLine { line, galley, origin, slot, response: r.clone(), phantom: false, cell: para.cell, atoms });
    if under {
        ui.add_space(cx.t.unit * 2.0);
        in_place::draw(ui, cx);
    }
    r
}

/// The run a piece opens in the source editor: a formula, a command shown as source.
fn atom_of(p: &Piece) -> Option<Atom> {
    match p.kind {
        PieceKind::Formula(_) | PieceKind::Typeset(_) | PieceKind::FormulaError(_) => Some(Atom { range: p.range.clone(), math: p.math.clone() }),
        PieceKind::Source => Some(Atom { range: p.range.clone(), math: None }),
        _ => None,
    }
}

/// Where a block ends in the view: a heading after the label shown with it.
fn block_end(block: &Block) -> usize {
    match &block.kind {
        BlockKind::Heading { label: Some(label), .. } => block.range.end.max(label.range.end),
        _ => block.range.end,
    }
}

/// An empty paragraph between source offsets `prev` and `next` (another block or the end of the
/// text), when the caret stands alone between blank lines there: Enter just made it.
fn phantom(ui: &mut egui::Ui, cx: &mut Cx<'_>, (prev, next): (usize, usize), last: bool) {
    let Some(c) = cx.caret.filter(|c| prev < *c && *c <= next) else { return };
    let (Some(before), Some(after)) = (cx.text.get(prev..c), cx.text.get(c..next)) else { return };
    let alone = before.trim().is_empty() && before.matches('\n').count() >= 2 && (after.trim().is_empty() || c == next);
    if !alone || cx.lines.iter().any(|d| d.phantom) {
        return;
    }
    let t = cx.t;
    let size = t.reading_size;
    if last {
        ui.add_space(gap_before(&t, &BlockKind::Paragraph(Vec::new())));
    }
    let galley = ui.painter().layout_no_wrap(String::new(), t.reading(size, false, false), t.text);
    let height = (size * t.reading_leading).round();
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::CLICK | Sense::DRAG);
    let slot = ui.painter().add(egui::Shape::Noop);
    let origin = pos2(rect.left(), rect.center().y - galley.size().y / 2.0);
    cx.lines.push(DrawnLine { line: Line::empty(c), galley, origin, slot, response, phantom: true, cell: None, atoms: Vec::new() });
    if !last {
        ui.add_space(gap_before(&t, &BlockKind::Paragraph(Vec::new())));
    }
}

/// The focus ring of a part of the view; whether Enter was pressed on it.
fn focusable(ui: &egui::Ui, cx: &mut Cx<'_>, r: &egui::Response, rect: Rect) -> bool {
    if cx.first.is_none() {
        cx.first = Some(r.id);
    }
    if !r.has_focus() {
        return false;
    }
    cx.focused = true;
    crate::widgets::focus_ring(ui, &cx.t, r, rect.expand(cx.t.unit), cx.t.corner());
    ui.input(|i| i.key_pressed(egui::Key::Enter))
}

/// The focus ring, Enter and double-click of a part of the view: they show its source.
fn activate(ui: &egui::Ui, cx: &mut Cx<'_>, r: &egui::Response, rect: Rect, start: usize, target: Option<usize>) {
    if focusable(ui, cx, r, rect) {
        cx.jump = Some(Jump(start));
    }
    if let Some(at) = target {
        cx.jump = Some(Jump(at));
    } else if r.double_clicked() {
        cx.jump = Some(Jump(start));
    }
}

/// The advance of the reading face's no-break space at `size`, and how far its baseline is below
/// the middle of its line (the text and a typeset formula's space are centred in their row, so
/// they share that baseline).
fn probe(ui: &egui::Ui, t: &Tokens, size: f32) -> (f32, f32) {
    let galley = ui.painter().layout_no_wrap("\u{a0}".to_owned(), t.reading(size, false, false), t.text);
    galley.rows.first().and_then(|r| r.glyphs.first()).map_or((size * 0.25, size * 0.25), |g| (g.advance_width, g.font_ascent - g.font_height / 2.0))
}

/// The baseline and right edge of char `at` of `galley`, relative to it (a formula's space is
/// its letter spacing, laid before the glyph, and the glyph: it ends where the glyph does).
fn glyph_end(galley: &Galley, at: usize) -> Option<Pos2> {
    let mut row_start = 0;
    for row in &galley.rows {
        let n = row.char_count_including_newline().0;
        if at < row_start + n {
            let g = row.glyphs.get(at - row_start)?;
            return Some(pos2(row.pos.x + g.pos.x + g.advance_width, row.pos.y + g.pos.y));
        }
        row_start += n;
    }
    None
}

/// Typeset formulas over the spaces held for them, in `color`, `scale` screen points per TeX
/// point.
fn paint_typeset(ui: &egui::Ui, cx: &mut Cx<'_>, galley: &Galley, pieces: &[Piece], origin: Pos2, (scale, color): (f32, Color32)) {
    let ppp = ui.ctx().pixels_per_point();
    for p in pieces {
        let PieceKind::Typeset(b) = &p.kind else { continue };
        let Some(end) = glyph_end(galley, p.chars.start) else { continue };
        let end = origin + end.to_vec2();
        let rect = Rect::from_min_max(pos2(end.x - b.width_pt * scale, end.y - b.height_pt * scale), pos2(end.x, end.y + b.depth_pt * scale));
        if ui.is_rect_visible(rect) {
            ink(ui, cx, b, rect, scale * ppp, color);
        }
    }
}

/// Draws typeset formula `b` in `rect`, rasterised at `px` pixels per TeX point.
fn ink(ui: &egui::Ui, cx: &mut Cx<'_>, b: &FormulaBox, rect: Rect, px: f32, color: Color32) {
    match cx.ink.texture(ui.ctx(), b, px) {
        Inked::Ready(id) => {
            ui.painter().image(id, rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), color);
        }
        Inked::Later => {}
        Inked::Broken => {
            ui.painter().rect_stroke(rect, cx.t.corner(), Stroke::new(1.0, cx.t.text_muted), StrokeKind::Inside);
        }
    }
}

/// The soft pills behind formulas, chips and source runs.
fn paint_pieces(ui: &egui::Ui, t: &Tokens, galley: &Galley, pieces: &[Piece], origin: Pos2, size: f32) {
    for p in pieces {
        let fill = match p.kind {
            PieceKind::Chip(_, _, Chip::Missing(_)) => blend(t.surface, t.pressed),
            PieceKind::Chip(..) => blend(t.surface, t.accent_soft),
            PieceKind::Formula(_) | PieceKind::FormulaError(_) | PieceKind::Source => blend(t.surface, t.pressed),
            PieceKind::Text | PieceKind::Break | PieceKind::Link(_) | PieceKind::Footnote(_) | PieceKind::Typeset(_) => continue,
        };
        // Chips and formulas are outlined too, so they read as one unit on dark surfaces.
        let stroke = match p.kind {
            PieceKind::Chip(_, _, Chip::Missing(_)) => Stroke::new(1.0, t.text_muted.gamma_multiply(0.5)),
            PieceKind::Chip(..) => Stroke::new(1.0, t.accent.gamma_multiply(0.35)),
            PieceKind::Formula(_) => formula_outline(t),
            PieceKind::FormulaError(_) => Stroke::new(1.0, t.error),
            _ => Stroke::NONE,
        };
        for r in span_rects(galley, (p.chars.start, p.chars.end)) {
            let r = r.translate(origin.to_vec2());
            let pill = Rect::from_center_size(r.center(), vec2(r.width(), (size * 1.3).min(r.height())));
            ui.painter().rect(pill, t.corner(), fill, stroke, StrokeKind::Inside);
            if matches!(p.kind, PieceKind::FormulaError(_)) {
                error_underline(ui, t, pill);
            }
        }
    }
}

/// The wavy line under a formula with an error.
fn error_underline(ui: &egui::Ui, t: &Tokens, pill: Rect) {
    let y = pill.bottom() - 1.0;
    let step = 2.0;
    let points: Vec<Pos2> = (0..=((pill.width() - 2.0 * step) / step).max(1.0) as usize)
        .map(|i| pos2(pill.left() + step + i as f32 * step, y - if i % 2 == 0 { 0.0 } else { 1.5 }))
        .collect();
    ui.painter().add(egui::Shape::line(points, Stroke::new(1.0, t.error)));
}

/// The outline of a formula's pill.
fn formula_outline(t: &Tokens) -> Stroke {
    Stroke::new(1.0, t.syntax_math.gamma_multiply(0.3))
}

/// How many no-break spaces hold a chip's icon.
const CHIP_ICON_SLOT: usize = 5;

/// The icon in each chip's leading spaces.
fn paint_chip_icons(ui: &egui::Ui, t: &Tokens, galley: &Galley, pieces: &[Piece], origin: Pos2, size: f32) {
    for p in pieces {
        let PieceKind::Chip(kind, _, Chip::Label | Chip::File) = p.kind else { continue };
        if let Some(r) = span_rects(galley, (p.chars.start + 1, p.chars.start + 1 + CHIP_ICON_SLOT)).first() {
            let r = r.translate(origin.to_vec2());
            crate::icons::paint(ui, r, chip_icon(kind), (size * 0.72).round(), t.accent);
        }
    }
}

fn preamble(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>) {
    let t = cx.t;
    let height = t.row_height + 2.0 * t.unit;
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::click());
    let open = cx.preamble_open;
    let label = tl!("Preamble");
    r.widget_info(|| egui::WidgetInfo::selected(egui::Role::Button, true, open, label));
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_expanded(open));
    let fill = if r.hovered() { blend(t.frame, t.hover) } else { t.frame };
    let corner = if open { egui::CornerRadius { nw: t.radius_large, ne: t.radius_large, sw: 0, se: 0 } } else { t.corner_large() };
    ui.painter().rect_filled(rect, corner, fill);
    ui.painter().rect_stroke(rect, corner, t.hairline(), StrokeKind::Inside);
    let chevron = Rect::from_min_size(rect.min + vec2(2.0 * t.unit, 0.0), vec2(t.icon_size, height));
    crate::icons::paint(ui, chevron, if open { "chevron-down" } else { "chevron-right" }, t.icon_size - 2.0, t.text_muted);
    let x = chevron.right() + 2.0 * t.unit;
    let title = ui.painter().text(pos2(x, rect.center().y), Align2::LEFT_CENTER, label, t.strong(), t.text);
    if !cx.summary.is_empty() {
        let at = pos2(title.right() + 3.0 * t.unit, rect.center().y);
        ui.painter().text(at, Align2::LEFT_CENTER, cx.summary, t.small(), t.text_muted);
    }
    let hint = if open { tl!("Hide") } else { tl!("Show") };
    ui.painter().text(pos2(rect.right() - 3.0 * t.unit, rect.center().y), Align2::RIGHT_CENTER, hint, t.small(), t.text_muted);
    if r.clicked() {
        cx.toggle_preamble = true;
    }
    if cx.first.is_none() {
        cx.first = Some(r.id);
    }
    if r.has_focus() {
        cx.focused = true;
    }
    crate::widgets::focus_ring(ui, &t, &r, rect, t.corner_large());
    let _ = r.on_hover_text(tl!("The preamble: the document's class, packages and settings. Click its source to edit it here."));
    if open {
        let src = cx.text.get(range.clone()).unwrap_or_default().trim_end();
        let _ = code(ui, cx, src, range.start, Some(egui::CornerRadius { nw: 0, ne: 0, sw: t.radius_large, se: t.radius_large }), None);
    }
}

/// Source text in the code font with its syntax colours, in a soft frame under an optional
/// `header`; double-clicking a character jumps to it.
fn code(ui: &mut egui::Ui, cx: &mut Cx<'_>, src: &str, offset: usize, corner: Option<egui::CornerRadius>, header: Option<&str>) -> egui::Response {
    let t = cx.t;
    if in_place::at_block(cx, offset) {
        let top = ui.cursor().min;
        in_place::draw(ui, cx);
        let rect = Rect::from_min_max(top, pos2(top.x + ui.available_width(), ui.cursor().top()));
        return ui.interact(rect, egui::Id::unique(("in-place-block", offset)), Sense::hover());
    }
    let pad = 3.0 * t.unit;
    let head = header.map(|h| ui.painter().layout_no_wrap(h.to_owned(), t.small(), t.text_muted));
    let head_h = head.as_ref().map_or(0.0, |g| g.size().y + 2.0 * t.unit);
    let width = ui.available_width();
    let mut job = LayoutJob::default();
    let font = FontId::new(t.code_size * 0.95, FontFamily::Monospace);
    for token in texcraft_latex::lex(src) {
        let piece = src.get(token.range.clone()).unwrap_or_default();
        job.append(piece, 0.0, TextFormat::simple(font.clone(), t.syntax(token.kind)));
    }
    job.wrap.max_width = width - 2.0 * pad;
    let galley = ui.painter().layout_job(job);
    let (rect, r) = ui.allocate_exact_size(vec2(width, head_h + galley.size().y + 2.0 * pad), Sense::click());
    let corner = corner.unwrap_or_else(|| t.corner_large());
    ui.painter().rect_filled(rect, corner, blend(t.surface, t.current_line));
    ui.painter().rect_stroke(rect, corner, t.hairline(), StrokeKind::Inside);
    if let Some(head) = head {
        let strip = Rect::from_min_size(rect.min, vec2(width, head_h));
        ui.painter().galley(pos2(strip.left() + pad, strip.center().y - head.size().y / 2.0), head, t.text_muted);
        ui.painter().hline(strip.x_range().shrink(1.0), strip.bottom(), t.hairline());
    }
    let origin = rect.min + vec2(pad, pad + head_h);
    ui.painter().galley(origin, Arc::clone(&galley), t.text);
    let source = tl!("LaTeX source: {text}").replace("{text}", src);
    // Its header is visible, so the name starts with it.
    let label = match header {
        Some(h) => format!("{h}. {source}"),
        None => source,
    };
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Code, true, &label));
    // A click or Enter edits it here.
    if focusable(ui, cx, &r, rect) || r.clicked() {
        let header = header.map_or_else(|| tl!("LaTeX").to_owned(), str::to_owned);
        cx.open = Some(InPlace::new(cx.path, cx.text, offset..offset + src.len(), What::Source, Anchor::Block(offset), header));
    }
    r
}

fn title_block(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, [title, author, date]: [&Vec<Inline>; 3]) {
    let t = cx.t;
    let align = std::mem::replace(&mut cx.align, Align::Center);
    ui.add_space(t.reading_size);
    if title.is_empty() && author.is_empty() {
        let tip = [Inline { kind: InlineKind::Source, range: range.clone() }];
        let _ = paragraph(
            ui,
            cx,
            &tip,
            range,
            &Para { size: t.reading_size, bold: false, color: t.text_muted, role: egui::Role::Paragraph, empty_at: None, cell: None },
        );
    }
    if !title.is_empty() {
        let r = paragraph(
            ui,
            cx,
            title,
            range,
            &Para { size: t.heading_sizes[0], bold: true, color: t.text, role: egui::Role::Heading, empty_at: None, cell: None },
        );
        ui.ctx().accesskit_node_builder(r.id, |node| node.set_level(1));
    }
    if !author.is_empty() {
        ui.add_space(t.reading_size * 0.75);
        let _ = paragraph(
            ui,
            cx,
            author,
            range,
            &Para { size: t.reading_size * 1.1, bold: false, color: t.text_muted, role: egui::Role::Paragraph, empty_at: None, cell: None },
        );
    }
    if !date.is_empty() {
        ui.add_space(t.reading_size * 0.25);
        let _ = paragraph(
            ui,
            cx,
            date,
            range,
            &Para { size: t.reading_size, bold: false, color: t.text_muted, role: egui::Role::Paragraph, empty_at: None, cell: None },
        );
    }
    ui.add_space(t.reading_size * 0.75);
    cx.align = align;
}

fn heading(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, level: u8, content: &[Inline], label: Option<&Inline>) {
    let t = cx.t;
    let size = t.heading_sizes.get(usize::from(level)).copied().unwrap_or(t.reading_size);
    let mut inlines = Vec::new();
    // Its number, as the PDF sets it before the title: shown, not edited.
    if let Some(n) = cx.refs.numbered.numbers.one(cx.path.as_str(), range.start) {
        inlines.push(Inline { kind: InlineKind::Text { text: format!("{n}\u{2002}"), style: Style::default() }, range: range.start..range.start });
    }
    inlines.extend(content.iter().cloned());
    if let Some(label) = label {
        inlines.push(Inline {
            kind: InlineKind::Text { text: "\u{2002}".into(), style: Style::default() },
            range: label.range.start..label.range.start,
        });
        inlines.push(label.clone());
    }
    let r = paragraph(ui, cx, &inlines, range, &Para { size, bold: true, color: t.text, role: egui::Role::Heading, empty_at: None, cell: None });
    // Part and chapter are the first level, section the second, and so on.
    let aria = usize::from(level.max(1));
    ui.ctx().accesskit_node_builder(r.id, |node| node.set_level(aria));
    ui.add_space(t.reading_size * 0.25);
}

/// A list's marker at `depth` for item `n` (from 0).
fn marker(kind: ListKind, depth: usize, n: usize) -> String {
    match kind {
        ListKind::Bullets => ["•", "◦", "▪", "·"][depth % 4].to_owned(),
        ListKind::Numbers => match depth % 4 {
            0 => format!("{}.", n + 1),
            1 => format!("({})", letters(n)),
            2 => format!("{}.", roman(n + 1)),
            _ => format!("{}.", letters(n).to_uppercase()),
        },
        ListKind::Descriptions => String::new(),
    }
}

/// `a`, `b`, … `z`, `aa`, …
fn letters(n: usize) -> String {
    let mut n = n;
    let mut out = Vec::new();
    loop {
        out.push(char::from(b'a' + (n % 26) as u8));
        if n < 26 {
            break;
        }
        n = n / 26 - 1;
    }
    out.iter().rev().collect()
}

fn roman(n: usize) -> String {
    let mut n = n;
    let mut out = String::new();
    for (value, digits) in [
        (1000, "m"),
        (900, "cm"),
        (500, "d"),
        (400, "cd"),
        (100, "c"),
        (90, "xc"),
        (50, "l"),
        (40, "xl"),
        (10, "x"),
        (9, "ix"),
        (5, "v"),
        (4, "iv"),
        (1, "i"),
    ] {
        while n >= value {
            out.push_str(digits);
            n -= value;
        }
    }
    out
}

fn list(ui: &mut egui::Ui, cx: &mut Cx<'_>, kind: ListKind, items: &[Item], depth: usize) {
    let t = cx.t;
    let indent = if kind == ListKind::Descriptions { t.reading_size * 1.25 } else { t.reading_size * 1.75 };
    let line = (t.reading_size * t.reading_leading).round();
    for (n, item) in items.iter().enumerate() {
        if n > 0 {
            ui.add_space(t.reading_size * 0.35);
        }
        let top = ui.cursor().top();
        let left = ui.cursor().left();
        let mark = marker(kind, depth, n);
        if !mark.is_empty() {
            let at = pos2(left + indent - t.reading_size * 0.55, top + line / 2.0);
            let font =
                if kind == ListKind::Numbers { t.reading(t.reading_size, false, false) } else { t.reading(t.reading_size * 1.05, true, false) };
            ui.painter().text(at, Align2::RIGHT_CENTER, mark, font, t.text_muted);
        }
        let mut blocks = item.blocks.clone();
        if let Some(label) = &item.label {
            let bold: Vec<Inline> = label
                .iter()
                .cloned()
                .map(|mut i| {
                    if let InlineKind::Text { style, .. } = &mut i.kind {
                        style.bold = true;
                    }
                    i
                })
                .collect();
            let space =
                Inline { kind: InlineKind::Text { text: "\u{2002}".into(), style: Style::default() }, range: item.range.start..item.range.start };
            match blocks.first_mut() {
                Some(Block { kind: BlockKind::Paragraph(p), .. }) => {
                    let rest = std::mem::take(p);
                    *p = bold.into_iter().chain([space]).chain(rest).collect();
                }
                _ => blocks.insert(0, Block { kind: BlockKind::Paragraph(bold), range: item.range.clone() }),
            }
        }
        if blocks.is_empty() {
            // Its caret stands after `\item `.
            let start = item.range.start + "\\item".len();
            let at = if cx.text.get(start..start + 1) == Some(" ") { start + 1 } else { start };
            blocks.push(Block { kind: BlockKind::Paragraph(Vec::new()), range: at..at.max(item.range.end) });
        }
        ui.horizontal_top(|ui| {
            ui.add_space(indent);
            ui.vertical(|ui| {
                ui.set_width(ui.available_width());
                draw_blocks(ui, cx, &blocks, depth + 1);
            });
        });
    }
}

fn container(ui: &mut egui::Ui, cx: &mut Cx<'_>, name: &str, blocks: &[Block], depth: usize) {
    let t = cx.t;
    match name {
        "abstract" => {
            let title = egui::RichText::new(tl!("Abstract").to_uppercase())
                .font(FontId::new(t.small_size, crate::fonts::semibold()))
                .extra_letter_spacing(1.2)
                .color(t.text_muted);
            ui.vertical_centered(|ui| ui.label(title));
            ui.add_space(t.unit * 2.0);
            indented(ui, cx, blocks, depth, t.reading_size * 1.5, false);
        }
        "quote" | "quotation" | "verse" => indented(ui, cx, blocks, depth, t.reading_size * 1.25, true),
        _ => {
            let align = match name {
                "center" => Align::Center,
                "flushright" => Align::Max,
                _ => Align::Min,
            };
            let outer = std::mem::replace(&mut cx.align, align);
            draw_blocks(ui, cx, blocks, depth);
            cx.align = outer;
        }
    }
}

/// Blocks inset by `inset` on both sides, with a rule on the left when `rule`.
fn indented(ui: &mut egui::Ui, cx: &mut Cx<'_>, blocks: &[Block], depth: usize, inset: f32, rule: bool) {
    let t = cx.t;
    let top = ui.cursor().top();
    let left = ui.cursor().left();
    let width = ui.available_width();
    ui.horizontal_top(|ui| {
        ui.add_space(inset);
        ui.vertical(|ui| {
            ui.set_width(width - if rule { inset } else { 2.0 * inset });
            draw_blocks(ui, cx, blocks, depth);
        });
    });
    if rule {
        let x = left + t.unit;
        ui.painter().line_segment([pos2(x, top), pos2(x, ui.cursor().top())], Stroke::new(3.0, t.separator));
    }
}

fn formula(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, source: &Range<usize>, env: Option<&str>, closed: bool) {
    let t = cx.t;
    if in_place::at_block(cx, range.start) {
        return in_place::draw(ui, cx);
    }
    // A click or Enter edits its math here, with its preview.
    let edit_here = |cx: &mut Cx<'_>| {
        let what = What::Formula { display: true, env: env.map(str::to_owned) };
        let title = match env {
            Some(env) => tl!("Formula ({env}), in LaTeX").replace("{env}", env),
            None => tl!("Formula (LaTeX)").to_owned(),
        };
        cx.open = Some(InPlace::new(cx.path, cx.text, source.clone(), what, Anchor::Block(range.start), title));
    };
    let math = cx.text.get(source.clone()).unwrap_or_default();
    let lines: Vec<&str> = math.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
    let shown = lines.join("\n");
    let pad = vec2(4.0 * t.unit, 3.0 * t.unit);
    let width = ui.available_width();
    // Its numbers, row by row, at the right as the PDF sets them; or that it is not closed.
    let rows: Vec<Option<String>> = match (closed, cx.refs.numbered.numbers.of(cx.path.as_str(), range.start)) {
        (false, _) => vec![Some(tl!("not closed").to_owned())],
        (true, Some(Number::Rows(rows))) => rows.clone(),
        (true, _) => Vec::new(),
    };
    let color = if closed { t.text } else { t.error };
    let font = if closed { t.reading(t.reading_size, false, false) } else { t.small() };
    let numbers: Vec<Option<Arc<Galley>>> =
        rows.iter().map(|r| r.as_ref().map(|n| ui.painter().layout_no_wrap(n.clone(), font.clone(), color))).collect();
    let numbered: Vec<&str> = rows.iter().flatten().map(String::as_str).collect();
    let margin = numbers.iter().flatten().map(|g| g.size().x + 2.0 * t.unit).fold(0.0, f32::max);
    let room = width - 2.0 * margin;
    let state = cx.typeset.get(&range.start).cloned();
    if let Some(FormulaState::Typeset(b)) = &state {
        // At the size of the text, made smaller to fit the measure.
        let scale = t.reading_size / cx.body_pt;
        let natural = vec2(b.width_pt, b.height_pt + b.depth_pt) * scale;
        let fit = (room / natural.x.max(1.0)).min(1.0);
        let (rect, r) = ui.allocate_exact_size(vec2(width, natural.y * fit + 2.0 * pad.y), Sense::click());
        let image = Rect::from_center_size(rect.center(), natural * fit);
        if ui.is_rect_visible(rect) {
            ink(ui, cx, b, image, scale * fit * ui.ctx().pixels_per_point(), t.text);
        }
        paint_numbers(ui, &numbers, rect, image.y_range(), color);
        // Named by its source, as a picture of a formula is, then its numbers.
        let label = if numbered.is_empty() { collapse(math) } else { format!("{} {}", collapse(math), numbered.join(" ")) };
        r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, true, &label));
        let r = r.on_hover_text(tl!("A formula. Click, or press Enter, to edit it here."));
        if focusable(ui, cx, &r, image.expand(t.unit)) || r.clicked() {
            edit_here(cx);
        }
        return;
    }
    let font = FontId::new(t.code_size, FontFamily::Monospace);
    let galley = ui.painter().layout_job(LayoutJob::simple(shown.clone(), font, t.syntax_math, room - 2.0 * pad.x));
    let (rect, r) = ui.allocate_exact_size(vec2(width, galley.size().y + 2.0 * pad.y), Sense::click());
    let pill = Rect::from_center_size(rect.center(), vec2((galley.rect.width() + 2.0 * pad.x).min(room), rect.height()));
    ui.painter().rect(pill, t.corner_large(), blend(t.surface, t.pressed), formula_outline(&t), StrokeKind::Inside);
    // Lines of a multi-line formula start together, as the source aligns them.
    let origin = pos2(pill.left() + pad.x - galley.rect.left(), rect.top() + pad.y - galley.rect.top());
    ui.painter().galley(origin, galley, t.syntax_math);
    if let Some(FormulaState::Failed(_)) = &state {
        error_underline(ui, &t, pill.shrink2(vec2(pad.x / 2.0, 0.0)));
    }
    paint_numbers(ui, &numbers, rect, pill.shrink2(vec2(0.0, pad.y)).y_range(), color);
    let label = tl!("Formula: {text}").replace("{text}", &shown);
    let label = if numbered.is_empty() { label } else { format!("{label} {}", numbered.join(" ")) };
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Math, true, &label));
    let tip = match &state {
        // Not closed: its tag says so.
        Some(FormulaState::Failed(messages)) if closed => tl!("This formula has an error: {message}").replace("{message}", &messages.join("\n")),
        Some(FormulaState::Pending) => tl!("The formula's source, while it is typeset. Click to edit it here.").to_owned(),
        _ => tl!("The formula's source: this version typesets formulas in the desktop app only. Click to edit it here.").to_owned(),
    };
    let r = r.on_hover_text(tip);
    if focusable(ui, cx, &r, pill) || r.clicked() {
        edit_here(cx);
    }
}

/// A display formula's numbers at the right of `rect`, row `i` of `n` in the `i`-th of `n` equal
/// parts of `rows` (its rows' heights are not known here).
fn paint_numbers(ui: &egui::Ui, numbers: &[Option<Arc<Galley>>], rect: Rect, rows: egui::Rangef, color: Color32) {
    let n = numbers.len().max(1) as f32;
    for (i, g) in numbers.iter().enumerate() {
        let Some(g) = g else { continue };
        let y = rows.min + rows.span() * (i as f32 + 0.5) / n;
        ui.painter().galley(pos2(rect.right() - g.size().x, y - g.size().y / 2.0), Arc::clone(g), color);
    }
}

/// A theorem-like environment as the PDF sets it: "Theorem 2 (Title)." in bold (a proof's
/// "Proof." in italics), its body under it, and a proof's end mark.
fn theorem(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, kind: &TheoremKind, title: &[Inline], blocks: &[Block], depth: usize) {
    let t = cx.t;
    let proof = !kind.numbered && kind.env == "proof";
    let mut head = kind.name.clone();
    if kind.numbered
        && let Some(n) = cx.refs.numbered.numbers.one(cx.path.as_str(), range.start)
    {
        head = format!("{head} {n}");
    }
    let title = collapse(&plain(cx.text, title));
    let format = |bold: bool, italic: bool| TextFormat { font_id: t.reading(t.reading_size, bold, italic), color: t.text, ..TextFormat::default() };
    let mut job = LayoutJob::default();
    job.append(&head, 0.0, format(!proof, proof));
    if !title.is_empty() {
        job.append(&format!(" ({title})"), 0.0, format(false, false));
    }
    job.append(".", 0.0, format(!proof, proof));
    job.wrap.max_width = ui.available_width();
    let words = if title.is_empty() { format!("{head}.") } else { format!("{head} ({title}).") };
    let galley = ui.painter().layout_job(job);
    let (rect, r) = ui.allocate_exact_size(vec2(ui.available_width(), galley.size().y), Sense::click());
    ui.painter().galley(rect.min, galley, t.text);
    r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Label, true, &words));
    let r = r.on_hover_text(tl!("Double-click, or press Enter, to show its source."));
    activate(ui, cx, &r, rect, range.start, None);
    ui.add_space(t.reading_size * 0.35);
    draw_blocks(ui, cx, blocks, depth);
    if proof {
        let mark = ui.painter().layout_no_wrap("∎".to_owned(), t.reading(t.reading_size, false, false), t.text_muted);
        let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), mark.size().y), Sense::hover());
        ui.painter().galley(pos2(rect.right() - mark.size().x, rect.top()), mark, t.text_muted);
    }
}

/// An `\input` or `\include` shown in place: the file's text under a thin rule with its name and
/// Edit Here, read-only (its text is edited in its own view); or why it cannot be shown.
fn include(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>, name: &str, depth: usize) {
    let t = cx.t;
    let width = ui.available_width();
    let found = cx.includes.get(name).cloned();
    let shown = match &found {
        Some(Ok((file, ..))) if file == cx.path || cx.within.contains(file) => {
            Err(tl!("{name} includes itself, so it is not shown again here.").replace("{name}", file.as_str()))
        }
        Some(Ok(found)) if cx.within.len() < 8 => Ok(found.clone()),
        Some(Ok((file, ..))) => Err(tl!("{name} is included too deep to be shown here.").replace("{name}", file.as_str())),
        Some(Err(_)) | None => Err(tl!("{name} is not among the project's files: the build will stop here.").replace("{name}", name)),
    };
    // The rule and the file's name, with Edit Here at the right.
    let top = ui.cursor().top();
    ui.painter().hline(ui.max_rect().x_range(), top, Stroke::new(1.0, t.separator));
    ui.add_space(t.unit);
    let file_name = match &shown {
        Ok((file, ..)) => file.as_str().to_owned(),
        Err(_) => name.to_owned(),
    };
    ui.horizontal(|ui| {
        ui.set_width(width);
        let (icon, _) = ui.allocate_exact_size(Vec2::splat(t.small_size * 1.2), Sense::hover());
        crate::icons::paint(ui, icon, "file-text", t.small_size, t.text_muted);
        ui.add_space(t.unit);
        let label = ui.label(egui::RichText::new(&file_name).font(t.small()).color(t.text_muted));
        let _ = label.on_hover_text(tl!("Included here with \\input or \\include: its text is shown read-only."));
        if let Ok((file, ..)) = &shown {
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                let r = crate::widgets::text_button(ui, &t, tl!("Edit Here"), true);
                let r = r.on_hover_text(tl!("Opens {name} in the formatted view, to edit it.").replace("{name}", file.as_str()));
                if r.clicked() {
                    cx.edit_included = Some(file.clone());
                }
            });
        }
    });
    ui.add_space(t.unit * 2.0);
    match shown {
        Err(why) => {
            let r = ui.label(egui::RichText::new(why).font(t.reading(t.reading_size * 0.9, false, true)).color(t.text_muted));
            if r.double_clicked() {
                cx.jump = Some(Jump(range.start));
            }
        }
        Ok((file, text, blocks)) => {
            let none = HashMap::new();
            let typeset = cx.typeset_files.get(&file).unwrap_or(&none);
            let mut within = cx.within.clone();
            within.push(cx.path.clone());
            let mut sub = Cx {
                t,
                text: &text,
                project: cx.project,
                path: &file,
                images: &mut *cx.images,
                preamble_open: false,
                summary: "",
                scroll_to: None,
                jump: None,
                toggle_preamble: false,
                first: None,
                focused: false,
                align: cx.align,
                footnotes: cx.footnotes,
                typeset,
                typeset_files: cx.typeset_files,
                body_pt: cx.body_pt,
                ink: &mut *cx.ink,
                lines: Vec::new(),
                caret: None,
                reveal: None,
                edit_caret: None,
                edit_anchor: None,
                table_prompt: None,
                focus_toolbar: false,
                toolbar_focused: false,
                table_action: None,
                edit_figure: None,
                figure_action: None,
                figure_label: None,
                misspelled: &[],
                grammar: &[],
                refs: cx.refs,
                includes: cx.includes,
                within,
                follow: None,
                in_place: None,
                preview: None,
                close_in_place: false,
                open: None,
                edit_included: None,
            };
            // Without the preamble of a file that has one: it is the main file's that counts.
            let body = match blocks.first() {
                Some(Block { kind: BlockKind::Preamble, .. }) => blocks.get(1..).unwrap_or_default(),
                _ => &blocks[..],
            };
            draw_blocks(ui, &mut sub, body, depth);
            // What it asks for goes to the view: following a reference, editing a file.
            cx.footnotes = sub.footnotes;
            cx.follow = cx.follow.take().or(sub.follow);
            cx.edit_included = cx.edit_included.take().or(sub.edit_included);
            // Editing a part of it (a double-click, Enter, a formula) opens it in its own view.
            if sub.jump.is_some() || sub.open.is_some() || sub.edit_figure.is_some() {
                cx.edit_included = Some(file.clone());
            }
            cx.focused |= sub.focused;
            if cx.first.is_none() {
                cx.first = sub.first;
            }
        }
    }
    ui.add_space(t.unit * 2.0);
    ui.painter().hline(ui.max_rect().x_range(), ui.cursor().top(), Stroke::new(1.0, t.separator));
}

/// A project file `\includegraphics` names: as written or with a figure extension, from the
/// project's root or the file's folder.
fn find_image(project: &Project, file: &ProjectPath, name: &str) -> Option<(ProjectPath, Vec<u8>)> {
    let folder = file.as_str().rsplit_once('/').map(|(f, _)| f);
    let bases = [Some(name.to_owned()), folder.map(|f| format!("{f}/{name}"))];
    for base in bases.into_iter().flatten() {
        for ext in ["", ".pdf", ".png", ".jpg", ".jpeg", ".svg"] {
            let Ok(path) = ProjectPath::new(&format!("{base}{ext}")) else { continue };
            if figure::kind(path.as_str()).is_none() {
                continue;
            }
            match project.get(&path) {
                Some(Contents::Binary(b)) => return Some((path, b.clone())),
                Some(Contents::Text(s)) => return Some((path, s.clone().into_bytes())),
                None => {}
            }
        }
    }
    None
}

/// The picture of an image, decoded once per file and length.
fn picture(ui: &egui::Ui, cx: &mut Cx<'_>, name: &str) -> Picture {
    let Some((path, bytes)) = find_image(cx.project, cx.path, name) else {
        return Err(tl!("{name} is not among the project's files.").replace("{name}", name));
    };
    let key = (path.as_str().to_owned(), bytes.len());
    if let Some(p) = cx.images.get(&key) {
        return p.clone();
    }
    let project = cx.project;
    let lookup = |href: &str| -> Option<Vec<u8>> {
        let p = ProjectPath::new(href).ok()?;
        match project.get(&p)? {
            Contents::Binary(b) => Some(b.clone()),
            Contents::Text(t) => Some(t.clone().into_bytes()),
        }
    };
    let max_side = ui.ctx().input(|i| i.max_texture_side).min(4096) as u32;
    let scale = 2.0 * ui.ctx().pixels_per_point();
    let decoded = figure::decode(path.as_str(), &bytes, 0, scale, max_side, &lookup)
        .map(|f| {
            let pixels = egui::ColorImage::from_rgba_premultiplied([f.image.width as usize, f.image.height as usize], &f.image.rgba);
            let texture = ui.ctx().load_texture(format!("formatted:{}", path.as_str()), pixels, egui::TextureOptions::LINEAR);
            (texture, vec2(f.size[0].max(1.0), f.size[1].max(1.0)))
        })
        .map_err(|e| tl!("{name} cannot be shown: {e}").replace("{name}", name).replace("{e}", &crate::i18n::msg(&e.to_string())));
    cx.images.insert(key, decoded.clone());
    decoded
}

fn source_block(ui: &mut egui::Ui, cx: &mut Cx<'_>, range: &Range<usize>) {
    let src = cx.text.get(range.clone()).unwrap_or_default().trim_end();
    let (tag, tip) = source_reason(src);
    let r = code(ui, cx, src, range.start, None, Some(&tag));
    let _ = r.on_hover_text(format!("{tip} {}", tl!("Click, or press Enter, to edit it here.")));
}

/// The title of a block shown as its source (what it is) and why the view shows it so.
fn source_reason(src: &str) -> (String, String) {
    let env = src.strip_prefix("\\begin{").and_then(|r| r.split_once('}')).map(|(name, _)| name.trim());
    match env {
        Some(name) if name.starts_with("table") => (
            tl!("LaTeX: a table float").to_owned(),
            tl!("The table editor shows a table float holding one tabular with its caption and label; this one holds more, so it is shown as its source.")
                .to_owned(),
        ),
        Some(name) => (
            tl!("LaTeX: the {name} environment").replace("{name}", name),
            tl!("The formatted view does not show the {name} environment, so it shows its source.").replace("{name}", name),
        ),
        None => (tl!("LaTeX").to_owned(), tl!("The formatted view cannot show this faithfully yet, so it shows its source.").to_owned()),
    }
}
