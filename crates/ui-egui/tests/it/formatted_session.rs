//! A writer's session in the formatted view, end to end, on each template (docs/testing.md,
//! "Testing the formatted view"): a paragraph with a bold word and a citation picked from the
//! bibliography, a list nested once, a heading, a figure with its caption and label and a
//! cross-reference to it, a table with two cells filled, an equation, a footnote, undo and redo,
//! Find and Replace, then undo back to the template and redo to the end, in a window of a
//! laptop's size with the build's fonts. After each step the keyboard is in the text, the caret in
//! a line of the view inside the window, a typed word shows before it, and the source is still
//! sound: nothing left unclosed, and no empty style, item or heading left where the caret is not.

use egui::accesskit::{Role, Toggled};
use egui_kittest::Harness;
use egui_kittest::kittest::{NodeT, Queryable};
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_latex::tree::{self, Kind, Node};
use texcraft_ui_egui::{App, ui_cmd};

const SIZE: egui::Vec2 = egui::vec2(1280.0, 860.0);

const VIEW: &str = "main.tex formatted";

/// A template: its files (the main file first), and words of a body paragraph the session starts
/// after.
struct Template {
    name: &'static str,
    files: &'static [(&'static str, &'static str)],
    anchor: &'static str,
}

const TEMPLATES: &[Template] = &[
    Template {
        name: "article",
        files: &[("main.tex", include_str!("../../../../templates/builtin/article/main.tex"))],
        anchor: "Say why the question matters",
    },
    Template { name: "cv", files: &[("main.tex", include_str!("../../../../templates/builtin/cv/main.tex"))], anchor: "Your Title" },
    Template {
        name: "example",
        files: &[
            ("main.tex", include_str!("../../../../templates/builtin/example/main.tex")),
            ("sections/introduction.tex", include_str!("../../../../templates/builtin/example/sections/introduction.tex")),
            ("references.bib", include_str!("../../../../templates/builtin/example/references.bib")),
        ],
        anchor: "Office paper is about",
    },
    Template {
        name: "homework",
        files: &[("main.tex", include_str!("../../../../templates/builtin/homework/main.tex"))],
        anchor: "Answer each part.",
    },
    Template {
        name: "letter",
        files: &[("main.tex", include_str!("../../../../templates/builtin/letter/main.tex"))],
        anchor: "The first paragraph says why",
    },
    Template { name: "slides", files: &[("main.tex", include_str!("../../../../templates/builtin/slides/main.tex"))], anchor: "Text on the left" },
    Template {
        name: "thesis",
        files: &[
            ("main.tex", include_str!("../../../../templates/builtin/thesis/main.tex")),
            ("chapters/introduction.tex", include_str!("../../../../templates/builtin/thesis/chapters/introduction.tex")),
            ("chapters/background.tex", include_str!("../../../../templates/builtin/thesis/chapters/background.tex")),
        ],
        anchor: "Data and proofs that would break",
    },
];

fn text(h: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| h.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

/// A bibliography of the session's own, beside the template's files: the entry the writer picks.
const BIB: (&str, &str) = (
    "session.bib",
    "@book{kilnworth19,\n  author = {Ada Kilnworth},\n  title = {Firing at Home},\n  publisher = {Example Press},\n  year = {2019}\n}\n",
);

fn open(t: &Template) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    for (i, (path, body)) in t.files.iter().chain(std::iter::once(&BIB)).enumerate() {
        if i > 0 {
            let _ = s.execute("file.new", &json!({"path": path}));
        }
        let written = s.execute("file.write", &json!({"path": path, "text": body}));
        assert!(written.is_ok(), "{}: {written:?}", t.name);
    }
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    let app = crate::support::app(s);
    // A character a build without craft-fonts lacks is drawn as a box: this checks editing.
    let mut h = Harness::builder().with_size(SIZE).allow_missing_glyphs().build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    h.get_by_label(VIEW).focus();
    h.run();
    h
}

/// The view's lines on screen: their node, rect and text.
fn lines(h: &Harness<'_, App>) -> Vec<(egui::accesskit::NodeId, egui::Rect, String)> {
    crate::support::view_lines(h, VIEW)
}

/// The caret's line (rect and text) and where in it.
fn caret(h: &Harness<'_, App>) -> Option<(egui::Rect, String, usize)> {
    crate::support::view_caret(h, VIEW)
}

fn settle(h: &mut Harness<'_, App>) {
    h.run();
    h.run();
}

fn key(h: &mut Harness<'_, App>, modifiers: egui::Modifiers, key: egui::Key) {
    h.key_press_modifiers(modifiers, key);
    settle(h);
}

fn type_text(h: &mut Harness<'_, App>, s: &str) {
    h.event(egui::Event::Text(s.to_owned()));
    settle(h);
}

fn button(h: &mut Harness<'_, App>, label: &str) {
    h.get_by_role_and_label(Role::Button, label).click();
    settle(h);
}

fn command(h: &mut Harness<'_, App>, id: &str, params: serde_json::Value) {
    ui_cmd::run_with(h.state_mut(), id, &params);
    settle(h);
}

fn pressed(h: &Harness<'_, App>, name: &str) -> bool {
    h.get_by_role_and_label(Role::Button, name).accesskit_node().data().toggled() == Some(Toggled::True)
}

/// The caret at the end of the line of the view that reads `words`: a click there when it is on
/// screen, else Down from the text's start until the caret's line reads them (the view scrolls to
/// it); then End.
fn click_after(h: &mut Harness<'_, App>, words: &str) -> Result<(), String> {
    if let Some((_, rect, _)) = lines(h).into_iter().find(|(_, r, v)| v.contains(words) && r.max.y < SIZE.y && r.min.y > 0.0) {
        let at = egui::pos2(rect.right() - 2.0, rect.center().y);
        for pressed in [true, false] {
            h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE });
            h.step();
        }
        settle(h);
    } else {
        key(h, egui::Modifiers::COMMAND, egui::Key::Home);
        let mut found = false;
        for _ in 0..200 {
            if caret(h).is_some_and(|(_, value, _)| value.contains(words)) {
                found = true;
                break;
            }
            key(h, egui::Modifiers::NONE, egui::Key::ArrowDown);
        }
        if !found {
            return Err(format!("no line of the view reads {words:?}"));
        }
    }
    // End goes to the end of a row: to the paragraph's end, row by row, until Right would leave
    // the line (then Left comes back to its end).
    for _ in 0..40 {
        key(h, egui::Modifiers::NONE, egui::Key::End);
        let before = caret(h).map(|(_, value, at)| (value, at));
        key(h, egui::Modifiers::NONE, egui::Key::ArrowRight);
        let after = caret(h).map(|(_, value, at)| (value, at));
        if after == before {
            return Ok(());
        }
        if after.as_ref().map(|a| &a.0) != before.as_ref().map(|b| &b.0) {
            key(h, egui::Modifiers::NONE, egui::Key::ArrowLeft);
            return Ok(());
        }
    }
    Err(format!("the end of the paragraph reading {words:?} was not reached"))
}

/// Whether nothing in `nodes` is left unclosed.
fn closed(nodes: &[Node]) -> bool {
    nodes.iter().all(|n| match &n.kind {
        Kind::Error => false,
        Kind::Group { body, closed } => *closed && closed_all(body),
        Kind::Environment { body, end, args, .. } => end.is_some() && args.iter().all(|a| a.closed && closed_all(&a.body)) && closed_all(body),
        Kind::Command { args, .. } => args.iter().all(|a| a.closed && closed_all(&a.body)),
        Kind::Math { closed, .. } | Kind::Verbatim { closed, .. } => *closed,
        _ => true,
    })
}

fn closed_all(nodes: &[Node]) -> bool {
    closed(nodes)
}

/// The empty styles, headings and items of `text`.
fn empties(text: &str) -> Vec<String> {
    fn walk(text: &str, nodes: &[Node], out: &mut Vec<String>) {
        for (i, n) in nodes.iter().enumerate() {
            let src = text.get(n.range.clone()).unwrap_or_default();
            match &n.kind {
                Kind::Command { name, args, .. } => {
                    let empty = args.iter().rev().find(|a| !a.optional).filter(|a| text.get(a.inner.clone()).is_some_and(|s| s.trim().is_empty()));
                    let styled = matches!(name.as_str(), "textbf" | "textit" | "texttt" | "emph");
                    let heading = texcraft_latex::view::heading_level(name).is_some();
                    if empty.is_some() && (styled || heading) {
                        out.push(src.to_owned());
                    }
                    if name == "item" {
                        // What is up to the next item or the list's end.
                        let rest = nodes.get(i + 1..).unwrap_or_default();
                        let body: Vec<&Node> = rest.iter().take_while(|m| !matches!(&m.kind, Kind::Command { name, .. } if name == "item")).collect();
                        let blank = body.iter().all(|m| {
                            matches!(m.kind, Kind::Text | Kind::ParBreak | Kind::Comment)
                                && text.get(m.range.clone()).is_some_and(|s| s.trim().is_empty())
                        });
                        let labelled = args.iter().any(|a| a.optional);
                        if blank && !labelled {
                            out.push("\\item".to_owned());
                        }
                    }
                    args.iter().for_each(|a| walk(text, &a.body, out));
                }
                Kind::Group { body, .. } | Kind::Environment { body, .. } => walk(text, body, out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(text, &tree::parse(text), &mut out);
    out
}

/// What must hold after each step; `typed` is a word just typed.
fn check(h: &mut Harness<'_, App>, (template, step): (&str, &str), typed: Option<&str>, baseline: &[String]) -> Result<(), String> {
    let at = format!("{template}, after {step}");
    if !h.query_by_label(VIEW).is_some_and(|n| n.accesskit_node().is_focused()) {
        return Err(format!("{at}: the keyboard left the text"));
    }
    let Some((rect, value, index)) = caret(h) else { return Err(format!("{at}: the caret is in no line of the view")) };
    if rect.max.y < 0.0 || rect.min.y > SIZE.y {
        return Err(format!("{at}: the caret's line is at {rect:?}, outside the window"));
    }
    if let Some(word) = typed {
        let before: String = value.chars().take(index).collect();
        // (A space typed last shows before the caret where the paragraph goes on after it.)
        if !before.trim_end().ends_with(word) {
            return Err(format!("{at}: typed {word:?}, the caret's line reads {before:?} before the caret"));
        }
    }
    let source = text(h);
    if !closed(&tree::parse(&source)) {
        return Err(format!("{at}: something is left unclosed in {source:?}"));
    }
    let mut left: Vec<String> = empties(&source);
    for b in baseline {
        if let Some(i) = left.iter().position(|e| e == b) {
            left.remove(i);
        }
    }
    // The caret may stand in an empty item or heading (Enter in a list, a list just started): its
    // line is empty.
    if value.trim().is_empty()
        && let Some(i) = left.iter().position(|e| e == "\\item" || e.starts_with("\\section") || e.starts_with("\\sub") || e.starts_with("\\chapter"))
    {
        left.remove(i);
    }
    if !left.is_empty() {
        return Err(format!("{at}: empty structure left behind: {left:?} in {source:?}"));
    }
    Ok(())
}

/// The session on one template; the first step that went wrong.
fn session(t: &Template) -> Result<(), String> {
    let mut h = open(t);
    let original = text(&mut h);
    let baseline = empties(&original);
    let n = t.name;
    let step = |h: &mut Harness<'_, App>, name: &str, typed: Option<&str>| check(h, (n, name), typed, &baseline);

    click_after(&mut h, t.anchor)?;
    step(&mut h, "a click after a paragraph", None)?;
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    step(&mut h, "Enter", None)?;
    type_text(&mut h, "Kilns fire ");
    step(&mut h, "typing", Some("fire"))?;
    // A style pressed with nothing selected waits for the word typed.
    let before_bold = text(&mut h);
    button(&mut h, "Bold");
    if text(&mut h) != before_bold || !pressed(&h, "Bold") {
        return Err(format!("{n}: Bold pressed with nothing selected changed the source or does not show pressed"));
    }
    type_text(&mut h, "glazed");
    step(&mut h, "a bold word", Some("glazed"))?;
    if !text(&mut h).contains("\\textbf{glazed}") {
        return Err(format!("{n}: the word typed after Bold is not bold: {:?}", text(&mut h)));
    }
    button(&mut h, "Bold");
    type_text(&mut h, " pots");
    step(&mut h, "Bold pressed off and typing", Some("pots"))?;
    if text(&mut h).contains("pots}") {
        let source = text(&mut h);
        let at = source.find("Kilns").unwrap_or(0);
        return Err(format!("{n}: the text typed after Bold was pressed off is still bold: {:?}", source.get(at..(at + 80).min(source.len()))));
    }
    // A citation picked from the bibliography by its author's name.
    command(&mut h, "edit.cite", json!({}));
    pick(&mut h, "Type part of a key, an author or a title", "Kilnworth")?;
    step(&mut h, "a citation", None)?;
    type_text(&mut h, ".");
    step(&mut h, "a full stop", None)?;
    if !text(&mut h).contains("\\cite{kilnworth19}.") {
        return Err(format!("{n}: the citation is not where it was inserted: {:?}", text(&mut h)));
    }

    // A list, nested once, then left.
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    // (Its button may be in the More menu when the editor is narrow: the command it runs.)
    command(&mut h, "edit.itemize", json!({}));
    step(&mut h, "Bulleted List", None)?;
    type_text(&mut h, "Clay");
    step(&mut h, "an item", Some("Clay"))?;
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    type_text(&mut h, "Glaze");
    step(&mut h, "a second item", Some("Glaze"))?;
    key(&mut h, egui::Modifiers::NONE, egui::Key::Tab);
    step(&mut h, "Tab", None)?;
    if !text(&mut h).contains("\\begin{itemize}\n")
        || text(&mut h).matches("\\begin{itemize}").count() < original.matches("\\begin{itemize}").count() + 2
    {
        return Err(format!("{n}: Tab did not nest the item: {:?}", text(&mut h)));
    }
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    type_text(&mut h, "Ash");
    step(&mut h, "a nested item", Some("Ash"))?;
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    key(&mut h, egui::Modifiers::SHIFT, egui::Key::Tab);
    key(&mut h, egui::Modifiers::SHIFT, egui::Key::Tab);
    step(&mut h, "Enter and Shift+Tab twice", None)?;

    // A heading, and a paragraph after it.
    type_text(&mut h, "Firing");
    command(&mut h, "edit.heading_2", json!({}));
    step(&mut h, "Heading 2", None)?;
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    key(&mut h, egui::Modifiers::NONE, egui::Key::Enter);
    type_text(&mut h, "Slowly.");
    step(&mut h, "a paragraph after the heading", Some("Slowly."))?;

    // A figure, its caption typed.
    command(&mut h, "edit.figure", json!({"image": "kiln.png"}));
    step(&mut h, "a figure", None)?;
    type_text(&mut h, "A kiln");
    step(&mut h, "its caption", Some("A kiln"))?;
    if !text(&mut h).contains("\\caption{A kiln") {
        return Err(format!("{n}: the caption typed is not the figure's: {:?}", text(&mut h)));
    }
    // Its label, from the image's menu.
    label_figure(&mut h, "fig:firing")?;
    step(&mut h, "the figure's label", None)?;
    if !text(&mut h).contains("\\label{fig:firing}") {
        return Err(format!("{n}: the figure has no label: {:?}", text(&mut h)));
    }
    // A cross-reference to it, picked by its label, in the paragraph before it.
    click_after(&mut h, "Slowly.")?;
    type_text(&mut h, " See ");
    command(&mut h, "edit.ref", json!({}));
    pick(&mut h, "Type part of a label or of what it labels", "fig:firing")?;
    step(&mut h, "a cross-reference", None)?;
    if !text(&mut h).contains("See \\ref{fig:firing}") && !text(&mut h).contains("See \\cref{fig:firing}") {
        return Err(format!("{n}: the cross-reference is not where it was inserted: {:?}", text(&mut h)));
    }
    type_text(&mut h, " Then");
    step(&mut h, "typing after the cross-reference", Some("Then"))?;

    // A table, two cells filled.
    command(&mut h, "edit.table", json!({}));
    step(&mut h, "a table", None)?;
    type_text(&mut h, "Cone");
    step(&mut h, "a cell", Some("Cone"))?;
    key(&mut h, egui::Modifiers::NONE, egui::Key::Tab);
    type_text(&mut h, "Heat");
    step(&mut h, "another cell", Some("Heat"))?;
    if !text(&mut h).contains("Cone & Heat") {
        return Err(format!("{n}: the cells typed are not the table's first row: {:?}", text(&mut h)));
    }

    // An equation.
    command(&mut h, "edit.equation", json!({"latex": "E = mc^2"}));
    step(&mut h, "an equation", None)?;

    // A footnote at the end of the paragraph written first.
    click_after(&mut h, "Kilns fire")?;
    let before_note = text(&mut h);
    command(&mut h, "edit.footnote", json!({"text": "At 1200 degrees."}));
    step(&mut h, "a footnote", None)?;
    let with_note = text(&mut h);
    if !with_note.contains("\\footnote{At 1200 degrees.}") {
        return Err(format!("{n}: no footnote: {with_note:?}"));
    }

    // Undo and redo give the texts back.
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
    if text(&mut h) != before_note {
        return Err(format!("{n}: undo did not take the footnote back"));
    }
    key(&mut h, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
    if text(&mut h) != with_note {
        return Err(format!("{n}: redo did not put the footnote back"));
    }
    step(&mut h, "undo and redo", None)?;

    // Find and Replace over the view.
    command(&mut h, "edit.replace", json!({}));
    h.get_by_label("Find").type_text("Clay");
    settle(&mut h);
    h.get_by_label("Replace with").click();
    settle(&mut h);
    h.get_by_label("Replace with").type_text("Stoneware");
    settle(&mut h);
    h.get_by_label("Replace All").click();
    settle(&mut h);
    let replaced = text(&mut h);
    if !replaced.contains("\\item Stoneware") || replaced.contains("Clay") && !original.contains("Clay") {
        return Err(format!("{n}: Replace All did not replace the item's word: {replaced:?}"));
    }
    key(&mut h, egui::Modifiers::NONE, egui::Key::Escape);
    h.get_by_label(VIEW).focus();
    settle(&mut h);
    step(&mut h, "Find and Replace", None)?;

    // Undo goes back through every text the session made, to the template as it was; redo
    // comes forward through them to the last.
    let last = text(&mut h);
    let mut seen = vec![last.clone()];
    for _ in 0..400 {
        if text(&mut h) == original {
            break;
        }
        key(&mut h, egui::Modifiers::COMMAND, egui::Key::Z);
        seen.push(text(&mut h));
        if seen[seen.len() - 1] == seen[seen.len() - 2] {
            return Err(format!("{n}: undo stopped before the template's text: {:?}", text(&mut h)));
        }
    }
    if text(&mut h) != original {
        return Err(format!("{n}: undo did not reach the template's text: {:?}", text(&mut h)));
    }
    step(&mut h, "undo back to the start", None)?;
    for want in seen.iter().rev().skip(1) {
        key(&mut h, egui::Modifiers::COMMAND | egui::Modifiers::SHIFT, egui::Key::Z);
        if text(&mut h) != *want {
            return Err(format!("{n}: redo gave {:?}, undo had gone through {want:?}", text(&mut h)));
        }
    }
    step(&mut h, "redo to the end", None)
}

/// In the picker the command opened (its field labelled `field`), `query` typed and Enter.
fn pick(h: &mut Harness<'_, App>, field: &str, query: &str) -> Result<(), String> {
    let Some(f) = h.query_by_label(field) else { return Err(format!("no picker with the field {field:?}")) };
    f.type_text(query);
    settle(h);
    key(h, egui::Modifiers::NONE, egui::Key::Enter);
    if h.query_by_label(field).is_some() {
        return Err(format!("the picker stayed open after Enter on {query:?}"));
    }
    Ok(())
}

/// The figure's label set from its image's menu (right-click, Label…).
fn label_figure(h: &mut Harness<'_, App>, label: &str) -> Result<(), String> {
    let images: Vec<egui::Rect> = h
        .query_all(egui_kittest::kittest::by().role(Role::Image))
        .filter(|n| n.accesskit_node().label().is_some_and(|l| l.contains("kiln")))
        .map(|n| n.rect())
        .collect();
    let Some(rect) = images.into_iter().find(|r| r.max.y > 0.0 && r.min.y < SIZE.y) else {
        return Err("the figure's image is not on screen".to_owned());
    };
    let at = rect.center();
    h.event(egui::Event::PointerMoved(at));
    h.step();
    for pressed in [true, false] {
        h.event(egui::Event::PointerButton { pos: at, button: egui::PointerButton::Secondary, pressed, modifiers: egui::Modifiers::NONE });
        h.step();
    }
    settle(h);
    let Some(item) = h.query_by_label("Label…") else { return Err("the image's menu has no Label…".to_owned()) };
    item.click();
    settle(h);
    let field = "The label references use, such as fig:plot";
    if h.query_by_label(field).is_none() {
        return Err("Label… asked for nothing".to_owned());
    }
    // In place of the label it has.
    key(h, egui::Modifiers::COMMAND, egui::Key::A);
    h.get_by_label(field).type_text(label);
    settle(h);
    h.get_by_label("Apply").click();
    settle(h);
    h.get_by_label(VIEW).focus();
    settle(h);
    Ok(())
}

/// The templates a run takes: `TEXCRAFT_SESSION_TEMPLATES` (`all`, or names with commas between
/// them); by default the article alone, as a session takes a few seconds on each.
fn chosen() -> Vec<&'static Template> {
    let wanted = std::env::var("TEXCRAFT_SESSION_TEMPLATES").unwrap_or_else(|_| "article".to_owned());
    TEMPLATES.iter().filter(|t| wanted == "all" || wanted.split(',').any(|w| w.trim() == t.name)).collect()
}

#[test]
fn a_writers_session_on_each_template_keeps_the_view_and_the_source_sound() {
    let failed: Vec<String> = chosen().into_iter().filter_map(|t| session(t).err()).collect();
    assert!(failed.is_empty(), "{} of {} sessions went wrong:\n\n{}", failed.len(), chosen().len(), failed.join("\n\n"));
}
