//! The symbol palette (docs/ROADMAP.md, E11): a side panel section of mathematical symbols in
//! groups, with the recently used ones first, searchable by name or command. Choosing one inserts
//! its command at the caret: as it is inside a formula, in `$…$` outside one. A symbol from a
//! package the document does not load says so.
//!
//! The grouping and the glyphs are TeXCraft's own: each symbol is drawn as its Unicode character in
//! the interface's faces (with the installed fonts behind them on the desktop).

use egui::{Align, Align2, Layout, Sense, vec2};
use serde_json::Value;
use texcraft_buffer::{EditKind, Selection, Transaction};
use texcraft_engine::project::Contents;

use crate::{App, widgets};

/// Recently inserted symbols remembered.
const RECENT_MAX: usize = 16;

/// A symbol: the command it inserts, its glyph, its name, and the package it needs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Symbol {
    pub command: &'static str,
    pub glyph: &'static str,
    pub name: &'static str,
    pub package: Option<&'static str>,
    /// It takes an argument (`\hat{…}`): the caret goes between its braces.
    pub argument: bool,
}

const fn s(command: &'static str, glyph: &'static str, name: &'static str) -> Symbol {
    Symbol { command, glyph, name, package: None, argument: false }
}

const fn ams(command: &'static str, glyph: &'static str, name: &'static str) -> Symbol {
    Symbol { command, glyph, name, package: Some("amssymb"), argument: false }
}

const fn amsmath(command: &'static str, glyph: &'static str, name: &'static str) -> Symbol {
    Symbol { command, glyph, name, package: Some("amsmath"), argument: false }
}

const fn arg(command: &'static str, glyph: &'static str, name: &'static str, package: Option<&'static str>) -> Symbol {
    Symbol { command, glyph, name, package, argument: true }
}

/// The palette's groups, in order: (name, symbols).
pub const GROUPS: &[(&str, &[Symbol])] = &[
    (
        tn!("Greek"),
        &[
            s("\\alpha", "α", tn!("alpha")),
            s("\\beta", "β", tn!("beta")),
            s("\\gamma", "γ", tn!("gamma")),
            s("\\delta", "δ", tn!("delta")),
            s("\\epsilon", "ϵ", tn!("epsilon")),
            s("\\varepsilon", "ε", tn!("epsilon, variant")),
            s("\\zeta", "ζ", tn!("zeta")),
            s("\\eta", "η", tn!("eta")),
            s("\\theta", "θ", tn!("theta")),
            s("\\vartheta", "ϑ", tn!("theta, variant")),
            s("\\iota", "ι", tn!("iota")),
            s("\\kappa", "κ", tn!("kappa")),
            s("\\lambda", "λ", tn!("lambda")),
            s("\\mu", "μ", tn!("mu")),
            s("\\nu", "ν", tn!("nu")),
            s("\\xi", "ξ", tn!("xi")),
            s("\\pi", "π", tn!("pi")),
            s("\\varpi", "ϖ", tn!("pi, variant")),
            s("\\rho", "ρ", tn!("rho")),
            s("\\varrho", "ϱ", tn!("rho, variant")),
            s("\\sigma", "σ", tn!("sigma")),
            s("\\varsigma", "ς", tn!("sigma, final")),
            s("\\tau", "τ", tn!("tau")),
            s("\\upsilon", "υ", tn!("upsilon")),
            s("\\phi", "ϕ", tn!("phi")),
            s("\\varphi", "φ", tn!("phi, variant")),
            s("\\chi", "χ", tn!("chi")),
            s("\\psi", "ψ", tn!("psi")),
            s("\\omega", "ω", tn!("omega")),
            s("\\Gamma", "Γ", tn!("Gamma, capital")),
            s("\\Delta", "Δ", tn!("Delta, capital")),
            s("\\Theta", "Θ", tn!("Theta, capital")),
            s("\\Lambda", "Λ", tn!("Lambda, capital")),
            s("\\Xi", "Ξ", tn!("Xi, capital")),
            s("\\Pi", "Π", tn!("Pi, capital")),
            s("\\Sigma", "Σ", tn!("Sigma, capital")),
            s("\\Upsilon", "Υ", tn!("Upsilon, capital")),
            s("\\Phi", "Φ", tn!("Phi, capital")),
            s("\\Psi", "Ψ", tn!("Psi, capital")),
            s("\\Omega", "Ω", tn!("Omega, capital")),
        ],
    ),
    (
        tn!("Relations"),
        &[
            s("\\leq", "≤", tn!("less than or equal")),
            s("\\geq", "≥", tn!("greater than or equal")),
            s("\\neq", "≠", tn!("not equal")),
            s("\\approx", "≈", tn!("approximately")),
            s("\\equiv", "≡", tn!("equivalent")),
            s("\\sim", "∼", tn!("similar")),
            s("\\simeq", "≃", tn!("similar or equal")),
            s("\\cong", "≅", tn!("congruent")),
            s("\\propto", "∝", tn!("proportional to")),
            s("\\ll", "≪", tn!("much less than")),
            s("\\gg", "≫", tn!("much greater than")),
            s("\\in", "∈", tn!("element of")),
            s("\\notin", "∉", tn!("not an element of")),
            s("\\ni", "∋", tn!("contains as member")),
            s("\\subset", "⊂", tn!("subset")),
            s("\\supset", "⊃", tn!("superset")),
            s("\\subseteq", "⊆", tn!("subset or equal")),
            s("\\supseteq", "⊇", tn!("superset or equal")),
            s("\\perp", "⊥", tn!("perpendicular")),
            s("\\parallel", "∥", tn!("parallel")),
            s("\\mid", "∣", tn!("divides")),
            s("\\models", "⊨", tn!("models")),
            s("\\vdash", "⊢", tn!("proves")),
            s("\\prec", "≺", tn!("precedes")),
            s("\\succ", "≻", tn!("succeeds")),
            ams("\\leqslant", "⩽", tn!("less than or slanted equal")),
            ams("\\geqslant", "⩾", tn!("greater than or slanted equal")),
            ams("\\lesssim", "≲", tn!("less than or similar")),
            ams("\\gtrsim", "≳", tn!("greater than or similar")),
            ams("\\triangleq", "≜", tn!("defined as")),
        ],
    ),
    (
        tn!("Operators"),
        &[
            s("\\pm", "±", tn!("plus or minus")),
            s("\\mp", "∓", tn!("minus or plus")),
            s("\\times", "×", tn!("times")),
            s("\\div", "÷", tn!("divided by")),
            s("\\cdot", "⋅", tn!("centred dot")),
            s("\\ast", "∗", tn!("asterisk")),
            s("\\circ", "∘", tn!("composition")),
            s("\\bullet", "∙", tn!("bullet")),
            s("\\oplus", "⊕", tn!("circled plus")),
            s("\\otimes", "⊗", tn!("circled times")),
            s("\\cup", "∪", tn!("union")),
            s("\\cap", "∩", tn!("intersection")),
            s("\\setminus", "∖", tn!("set minus")),
            s("\\wedge", "∧", tn!("logical and")),
            s("\\vee", "∨", tn!("logical or")),
            s("\\neg", "¬", tn!("not")),
            s("\\sum", "∑", tn!("sum")),
            s("\\prod", "∏", tn!("product")),
            s("\\coprod", "∐", tn!("coproduct")),
            s("\\int", "∫", tn!("integral")),
            s("\\iint", "∬", tn!("double integral")),
            s("\\oint", "∮", tn!("contour integral")),
            s("\\bigcup", "⋃", tn!("union, large")),
            s("\\bigcap", "⋂", tn!("intersection, large")),
            s("\\nabla", "∇", tn!("nabla")),
            s("\\partial", "∂", tn!("partial derivative")),
            s("\\sqrt{}", "√", tn!("square root")),
            arg("\\frac{}{}", "½", tn!("fraction"), None),
        ],
    ),
    (
        tn!("Arrows"),
        &[
            s("\\to", "→", tn!("to")),
            s("\\gets", "←", tn!("gets")),
            s("\\leftrightarrow", "↔", tn!("left and right arrow")),
            s("\\Rightarrow", "⇒", tn!("implies, double arrow")),
            s("\\Leftarrow", "⇐", tn!("left double arrow")),
            s("\\Leftrightarrow", "⇔", tn!("if and only if, double arrow")),
            s("\\mapsto", "↦", tn!("maps to")),
            s("\\uparrow", "↑", tn!("up arrow")),
            s("\\downarrow", "↓", tn!("down arrow")),
            s("\\updownarrow", "↕", tn!("up and down arrow")),
            s("\\longrightarrow", "⟶", tn!("long right arrow")),
            s("\\longleftarrow", "⟵", tn!("long left arrow")),
            s("\\Longrightarrow", "⟹", tn!("long double right arrow")),
            s("\\hookrightarrow", "↪", tn!("hook right arrow")),
            s("\\nearrow", "↗", tn!("north-east arrow")),
            s("\\searrow", "↘", tn!("south-east arrow")),
            s("\\rightleftharpoons", "⇌", tn!("equilibrium")),
            amsmath("\\implies", "⟹", tn!("implies")),
            amsmath("\\iff", "⟺", tn!("if and only if")),
            ams("\\twoheadrightarrow", "↠", tn!("onto arrow")),
        ],
    ),
    (
        tn!("Accents"),
        &[
            arg("\\hat{}", "x̂", tn!("hat"), None),
            arg("\\widehat{}", "x̂", tn!("wide hat"), None),
            arg("\\bar{}", "x̄", tn!("bar"), None),
            arg("\\overline{}", "x̅", tn!("overline"), None),
            arg("\\tilde{}", "x̃", tn!("tilde"), None),
            arg("\\widetilde{}", "x̃", tn!("wide tilde"), None),
            arg("\\vec{}", "x⃗", tn!("vector"), None),
            arg("\\dot{}", "ẋ", tn!("dot"), None),
            arg("\\ddot{}", "ẍ", tn!("double dot"), None),
            arg("\\acute{}", "x́", tn!("acute"), None),
            arg("\\grave{}", "x̀", tn!("grave"), None),
            arg("\\check{}", "x̌", tn!("check"), None),
            arg("\\breve{}", "x̆", tn!("breve"), None),
            arg("\\underline{}", "x̲", tn!("underline"), None),
            arg("\\mathbb{}", "ℝ", tn!("blackboard bold"), Some("amssymb")),
            arg("\\mathcal{}", "𝒞", tn!("calligraphic"), None),
            arg("\\mathfrak{}", "𝔉", tn!("fraktur"), Some("amssymb")),
            arg("\\mathbf{}", "𝐱", tn!("bold"), None),
        ],
    ),
    (
        tn!("Delimiters"),
        &[
            s("\\langle", "⟨", tn!("left angle bracket")),
            s("\\rangle", "⟩", tn!("right angle bracket")),
            s("\\lfloor", "⌊", tn!("left floor")),
            s("\\rfloor", "⌋", tn!("right floor")),
            s("\\lceil", "⌈", tn!("left ceiling")),
            s("\\rceil", "⌉", tn!("right ceiling")),
            s("\\{", "{", tn!("left brace")),
            s("\\}", "}", tn!("right brace")),
            s("\\|", "‖", tn!("double bar")),
            amsmath("\\lvert", "|", tn!("left bar")),
            amsmath("\\rvert", "|", tn!("right bar")),
            amsmath("\\lVert", "‖", tn!("left double bar")),
            amsmath("\\rVert", "‖", tn!("right double bar")),
        ],
    ),
    (
        tn!("Miscellaneous"),
        &[
            s("\\infty", "∞", tn!("infinity")),
            s("\\forall", "∀", tn!("for all")),
            s("\\exists", "∃", tn!("there exists")),
            ams("\\nexists", "∄", tn!("there does not exist")),
            s("\\emptyset", "∅", tn!("empty set")),
            ams("\\varnothing", "⌀", tn!("empty set, variant")),
            s("\\aleph", "ℵ", tn!("aleph")),
            s("\\hbar", "ℏ", tn!("h-bar")),
            s("\\ell", "ℓ", tn!("script l")),
            s("\\Re", "ℜ", tn!("real part")),
            s("\\Im", "ℑ", tn!("imaginary part")),
            s("\\wp", "℘", tn!("Weierstrass p")),
            s("\\angle", "∠", tn!("angle")),
            s("\\triangle", "△", tn!("triangle")),
            s("\\prime", "′", tn!("prime")),
            s("\\ldots", "…", tn!("dots on the line")),
            s("\\cdots", "⋯", tn!("centred dots")),
            s("\\vdots", "⋮", tn!("vertical dots")),
            s("\\ddots", "⋱", tn!("diagonal dots")),
            s("\\top", "⊤", tn!("top")),
            s("\\bot", "⊥", tn!("bottom")),
            ams("\\therefore", "∴", tn!("therefore")),
            ams("\\because", "∵", tn!("because")),
            ams("\\square", "□", tn!("square")),
            ams("\\blacksquare", "■", tn!("filled square")),
            s("\\dagger", "†", tn!("dagger")),
            s("\\star", "⋆", tn!("star")),
            s("\\degree", "°", tn!("degree")),
        ],
    ),
];

/// Every symbol, by its command.
pub fn find(command: &str) -> Option<Symbol> {
    GROUPS.iter().flat_map(|(_, list)| list.iter()).find(|s| s.command == command).copied()
}

/// The symbol's accessible name and tooltip: its name, then its command.
fn label(s: &Symbol) -> String {
    format!("{} {}", crate::i18n::t(s.name), s.command)
}

/// Whether `s` matches the search `query` (its name or command).
fn matches(s: &Symbol, query: &str) -> bool {
    let q = query.trim().trim_start_matches('\\').to_lowercase();
    q.is_empty() || s.command.to_lowercase().contains(&q) || s.name.contains(&q) || crate::i18n::t(s.name).to_lowercase().contains(&q)
}

/// The palette's search and where the keyboard goes.
#[derive(Default)]
pub(crate) struct Palette {
    query: String,
    /// Give the search field the keyboard next frame.
    pub focus: bool,
}

/// Opens the palette in the side panel, the keyboard in its search (`edit.symbols`); closes it
/// when it is open.
pub(crate) fn toggle(app: &mut App) {
    if app.view.side == Some(crate::Side::Symbols) {
        app.view.side = None;
        app.editor.focus = true;
    } else {
        app.view.side = Some(crate::Side::Symbols);
        app.symbols.focus = true;
    }
}

/// `edit.insert_symbol {"command": "\\alpha"}`.
pub(crate) fn run(app: &mut App, params: &Value) {
    let Some(symbol) = params.get("command").and_then(Value::as_str).and_then(find) else {
        app.view.status = Some((tl!("Name a symbol of the palette by its command, such as \\alpha.").to_owned(), true));
        return;
    };
    insert(app, &symbol);
}

/// Whether the project's main file (or the shown one) loads `package`.
fn loads(app: &App, package: &str) -> bool {
    let project = app.session.project();
    let files = [project.main(), app.session.current()];
    files.into_iter().flatten().filter_map(|p| project.text(p).ok()).any(|text| {
        text.lines().filter(|l| l.trim_start().starts_with("\\usepackage") || l.trim_start().starts_with("\\RequirePackage")).any(|l| {
            let names = l.rsplit_once('{').map_or("", |(_, n)| n.trim_end_matches(|c: char| c == '}' || c.is_whitespace()));
            // mathtools loads amsmath.
            names.split(',').map(str::trim).any(|n| n == package || (package == "amsmath" && n == "mathtools"))
        })
    })
}

/// Inserts `symbol` at the caret of the shown source: its command in a formula, `$…$` around it
/// elsewhere.
pub(crate) fn insert(app: &mut App, symbol: &Symbol) {
    let Some(path) = app.session.current().cloned().filter(|p| matches!(app.session.project().get(p), Some(Contents::Text(_)))) else {
        app.view.status = Some((tl!("Open a text file to insert a symbol in it.").to_owned(), true));
        return;
    };
    app.editor.formatted.set(&path, false);
    let source = &mut app.editor.source;
    let doc = &mut source.doc;
    let r = doc.selection.primary();
    let (a, b) = (r.start(), r.end());
    let text = doc.buffer.text();
    let byte = text.char_indices().nth(a).map_or(text.len(), |(i, _)| i);
    let in_math = texcraft_latex::tree::in_math(&texcraft_latex::tree::parse(&text), byte);
    let selected = doc.buffer.slice(a, b);
    let mut command = symbol.command.to_owned();
    // Inside the braces: the selection, and the caret.
    let caret_in = if symbol.argument || command.ends_with("{}") {
        let open = command.find("{}").map_or(command.len(), |i| i + 1);
        command.insert_str(open, &selected);
        Some(command[..open].chars().count() + selected.chars().count())
    } else {
        None
    };
    // A command word followed by a letter needs a space between them.
    let next = text.get(byte..).and_then(|t| t.chars().nth(b - a));
    if in_math && caret_in.is_none() && command.ends_with(|c: char| c.is_ascii_alphabetic()) && next.is_some_and(|c| c.is_ascii_alphabetic()) {
        command.push(' ');
    }
    let (insertion, caret) = if in_math {
        let len = command.chars().count();
        (command, caret_in.unwrap_or(len))
    } else {
        let len = command.chars().count();
        (format!("${command}$"), 1 + caret_in.unwrap_or(len))
    };
    let at = a + caret;
    let before = doc.selection.clone();
    if doc.buffer.edit(Transaction::replace(a, b, &insertion), &before, &Selection::point(at), EditKind::Other).is_err() {
        return;
    }
    doc.buffer.seal();
    source.select(at, at);
    source.reveal = true;
    remember(app, symbol);
    let said = match symbol.package.filter(|p| !loads(app, p)) {
        Some(package) => tl!("Inserted {command}; it needs \\usepackage{{package}} in the preamble.")
            .replace("{command}", symbol.command.trim_end_matches("{}"))
            .replace("{package}", package),
        None => tl!("Inserted {command}.").replace("{command}", symbol.command.trim_end_matches("{}")),
    };
    app.view.status = Some((said, false));
}

/// Puts `symbol` first among the recent ones.
fn remember(app: &mut App, symbol: &Symbol) {
    let recent = &mut app.prefs.recent_symbols;
    recent.retain(|c| c != symbol.command);
    recent.insert(0, symbol.command.to_owned());
    recent.truncate(RECENT_MAX);
    app.prefs_changed = true;
}

/// The palette, in the side panel.
pub(crate) fn panel(app: &mut App, ui: &mut egui::Ui) {
    let t = app.tokens;
    ui.allocate_ui_with_layout(vec2(ui.available_width(), t.toolbar_height), Layout::left_to_right(Align::Center), |ui| {
        ui.add_space(t.unit * 2.0);
        widgets::section_title(ui, &t, tl!("Symbols"));
    });
    let id = egui::Id::unique("texcraft-symbols-search");
    let mut chosen: Option<Symbol> = None;
    let query = app.symbols.query.clone();
    let shown: Vec<Symbol> = GROUPS.iter().flat_map(|(_, l)| l.iter()).filter(|s| matches(s, &query)).copied().collect();
    ui.horizontal(|ui| {
        ui.add_space(t.unit);
        let output = egui::TextEdit::singleline(&mut app.symbols.query)
            .id(id)
            .hint_text(tl!("Search by name or command"))
            .desired_width(ui.available_width() - 2.0 * t.unit)
            .show(ui);
        let r = output.response.response.clone();
        app.region(crate::Region::Side, r.id);
        let _ = r.clone().accessible_name(tl!("Search symbols"));
        if std::mem::take(&mut app.symbols.focus) {
            r.request_focus();
        }
        if r.lost_focus() {
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) && !query.trim().is_empty() {
                chosen = shown.first().copied();
            } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                app.editor.focus = true;
            }
        }
    });
    ui.add_space(t.unit);
    egui::ScrollArea::vertical().id_salt("symbols").auto_shrink([false, false]).show(ui, |ui| {
        if !query.trim().is_empty() {
            if shown.is_empty() {
                ui.label(egui::RichText::new(tl!("No symbol has that name or command.")).font(t.small()).color(t.text_muted));
            }
            grid(ui, app, &shown, &mut chosen);
            return;
        }
        let recent: Vec<Symbol> = app.prefs.recent_symbols.iter().filter_map(|c| find(c)).collect();
        if !recent.is_empty() {
            heading(ui, &t, tl!("Recent"));
            grid(ui, app, &recent, &mut chosen);
        }
        for (name, list) in GROUPS {
            heading(ui, &t, crate::i18n::t(name));
            grid(ui, app, list, &mut chosen);
        }
    });
    if let Some(symbol) = chosen {
        insert(app, &symbol);
        app.editor.focus = true;
    }
}

fn heading(ui: &mut egui::Ui, t: &crate::theme::Tokens, name: &str) {
    ui.add_space(t.unit * 2.0);
    ui.horizontal(|ui| {
        ui.add_space(t.unit * 2.0);
        ui.label(egui::RichText::new(name).font(t.small()).color(t.text_muted));
    });
}

/// The symbols as square buttons, wrapping; each named by its name and command.
fn grid(ui: &mut egui::Ui, app: &App, list: &[Symbol], chosen: &mut Option<Symbol>) {
    let t = app.tokens;
    let side = t.toolbar_height;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = vec2(t.unit / 2.0, t.unit / 2.0);
        ui.add_space(t.unit);
        for s in list {
            let (rect, r) = ui.allocate_exact_size(vec2(side, side), Sense::click());
            let name = label(s);
            r.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, true, &name));
            if ui.is_rect_visible(rect) {
                let fill = widgets::state_fill(&t, &r, t.frame);
                if fill != t.frame {
                    ui.painter().rect_filled(rect, t.corner(), fill);
                }
                let font = egui::FontId::proportional(t.font_size * 1.35);
                // Without a face that has the glyph, the command stands for it.
                if ui.fonts_mut(|f| f.has_glyphs(&font, s.glyph)) {
                    ui.painter().text(rect.center(), Align2::CENTER_CENTER, s.glyph, font, t.text);
                } else {
                    let short: String = s.command.trim_start_matches('\\').trim_end_matches("{}").chars().take(4).collect();
                    ui.painter().with_clip_rect(rect).text(rect.center(), Align2::CENTER_CENTER, short, t.small(), t.text_muted);
                }
                widgets::focus_ring(ui, &t, &r, rect, t.corner());
            }
            let tip = match s.package {
                Some(p) => format!("{name} ({})", tl!("needs {package}").replace("{package}", p)),
                None => name,
            };
            if r.on_hover_text(tip).clicked() {
                *chosen = Some(*s);
            }
        }
    });
}
