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
        "Greek",
        &[
            s("\\alpha", "α", "alpha"),
            s("\\beta", "β", "beta"),
            s("\\gamma", "γ", "gamma"),
            s("\\delta", "δ", "delta"),
            s("\\epsilon", "ϵ", "epsilon"),
            s("\\varepsilon", "ε", "epsilon, variant"),
            s("\\zeta", "ζ", "zeta"),
            s("\\eta", "η", "eta"),
            s("\\theta", "θ", "theta"),
            s("\\vartheta", "ϑ", "theta, variant"),
            s("\\iota", "ι", "iota"),
            s("\\kappa", "κ", "kappa"),
            s("\\lambda", "λ", "lambda"),
            s("\\mu", "μ", "mu"),
            s("\\nu", "ν", "nu"),
            s("\\xi", "ξ", "xi"),
            s("\\pi", "π", "pi"),
            s("\\varpi", "ϖ", "pi, variant"),
            s("\\rho", "ρ", "rho"),
            s("\\varrho", "ϱ", "rho, variant"),
            s("\\sigma", "σ", "sigma"),
            s("\\varsigma", "ς", "sigma, final"),
            s("\\tau", "τ", "tau"),
            s("\\upsilon", "υ", "upsilon"),
            s("\\phi", "ϕ", "phi"),
            s("\\varphi", "φ", "phi, variant"),
            s("\\chi", "χ", "chi"),
            s("\\psi", "ψ", "psi"),
            s("\\omega", "ω", "omega"),
            s("\\Gamma", "Γ", "Gamma, capital"),
            s("\\Delta", "Δ", "Delta, capital"),
            s("\\Theta", "Θ", "Theta, capital"),
            s("\\Lambda", "Λ", "Lambda, capital"),
            s("\\Xi", "Ξ", "Xi, capital"),
            s("\\Pi", "Π", "Pi, capital"),
            s("\\Sigma", "Σ", "Sigma, capital"),
            s("\\Upsilon", "Υ", "Upsilon, capital"),
            s("\\Phi", "Φ", "Phi, capital"),
            s("\\Psi", "Ψ", "Psi, capital"),
            s("\\Omega", "Ω", "Omega, capital"),
        ],
    ),
    (
        "Relations",
        &[
            s("\\leq", "≤", "less than or equal"),
            s("\\geq", "≥", "greater than or equal"),
            s("\\neq", "≠", "not equal"),
            s("\\approx", "≈", "approximately"),
            s("\\equiv", "≡", "equivalent"),
            s("\\sim", "∼", "similar"),
            s("\\simeq", "≃", "similar or equal"),
            s("\\cong", "≅", "congruent"),
            s("\\propto", "∝", "proportional to"),
            s("\\ll", "≪", "much less than"),
            s("\\gg", "≫", "much greater than"),
            s("\\in", "∈", "element of"),
            s("\\notin", "∉", "not an element of"),
            s("\\ni", "∋", "contains as member"),
            s("\\subset", "⊂", "subset"),
            s("\\supset", "⊃", "superset"),
            s("\\subseteq", "⊆", "subset or equal"),
            s("\\supseteq", "⊇", "superset or equal"),
            s("\\perp", "⊥", "perpendicular"),
            s("\\parallel", "∥", "parallel"),
            s("\\mid", "∣", "divides"),
            s("\\models", "⊨", "models"),
            s("\\vdash", "⊢", "proves"),
            s("\\prec", "≺", "precedes"),
            s("\\succ", "≻", "succeeds"),
            ams("\\leqslant", "⩽", "less than or slanted equal"),
            ams("\\geqslant", "⩾", "greater than or slanted equal"),
            ams("\\lesssim", "≲", "less than or similar"),
            ams("\\gtrsim", "≳", "greater than or similar"),
            ams("\\triangleq", "≜", "defined as"),
        ],
    ),
    (
        "Operators",
        &[
            s("\\pm", "±", "plus or minus"),
            s("\\mp", "∓", "minus or plus"),
            s("\\times", "×", "times"),
            s("\\div", "÷", "divided by"),
            s("\\cdot", "⋅", "centred dot"),
            s("\\ast", "∗", "asterisk"),
            s("\\circ", "∘", "composition"),
            s("\\bullet", "∙", "bullet"),
            s("\\oplus", "⊕", "circled plus"),
            s("\\otimes", "⊗", "circled times"),
            s("\\cup", "∪", "union"),
            s("\\cap", "∩", "intersection"),
            s("\\setminus", "∖", "set minus"),
            s("\\wedge", "∧", "logical and"),
            s("\\vee", "∨", "logical or"),
            s("\\neg", "¬", "not"),
            s("\\sum", "∑", "sum"),
            s("\\prod", "∏", "product"),
            s("\\coprod", "∐", "coproduct"),
            s("\\int", "∫", "integral"),
            s("\\iint", "∬", "double integral"),
            s("\\oint", "∮", "contour integral"),
            s("\\bigcup", "⋃", "union, large"),
            s("\\bigcap", "⋂", "intersection, large"),
            s("\\nabla", "∇", "nabla"),
            s("\\partial", "∂", "partial derivative"),
            s("\\sqrt{}", "√", "square root"),
            arg("\\frac{}{}", "½", "fraction", None),
        ],
    ),
    (
        "Arrows",
        &[
            s("\\to", "→", "to"),
            s("\\gets", "←", "gets"),
            s("\\leftrightarrow", "↔", "left and right arrow"),
            s("\\Rightarrow", "⇒", "implies, double arrow"),
            s("\\Leftarrow", "⇐", "left double arrow"),
            s("\\Leftrightarrow", "⇔", "if and only if, double arrow"),
            s("\\mapsto", "↦", "maps to"),
            s("\\uparrow", "↑", "up arrow"),
            s("\\downarrow", "↓", "down arrow"),
            s("\\updownarrow", "↕", "up and down arrow"),
            s("\\longrightarrow", "⟶", "long right arrow"),
            s("\\longleftarrow", "⟵", "long left arrow"),
            s("\\Longrightarrow", "⟹", "long double right arrow"),
            s("\\hookrightarrow", "↪", "hook right arrow"),
            s("\\nearrow", "↗", "north-east arrow"),
            s("\\searrow", "↘", "south-east arrow"),
            s("\\rightleftharpoons", "⇌", "equilibrium"),
            amsmath("\\implies", "⟹", "implies"),
            amsmath("\\iff", "⟺", "if and only if"),
            ams("\\twoheadrightarrow", "↠", "onto arrow"),
        ],
    ),
    (
        "Accents",
        &[
            arg("\\hat{}", "x̂", "hat", None),
            arg("\\widehat{}", "x̂", "wide hat", None),
            arg("\\bar{}", "x̄", "bar", None),
            arg("\\overline{}", "x̅", "overline", None),
            arg("\\tilde{}", "x̃", "tilde", None),
            arg("\\widetilde{}", "x̃", "wide tilde", None),
            arg("\\vec{}", "x⃗", "vector", None),
            arg("\\dot{}", "ẋ", "dot", None),
            arg("\\ddot{}", "ẍ", "double dot", None),
            arg("\\acute{}", "x́", "acute", None),
            arg("\\grave{}", "x̀", "grave", None),
            arg("\\check{}", "x̌", "check", None),
            arg("\\breve{}", "x̆", "breve", None),
            arg("\\underline{}", "x̲", "underline", None),
            arg("\\mathbb{}", "ℝ", "blackboard bold", Some("amssymb")),
            arg("\\mathcal{}", "𝒞", "calligraphic", None),
            arg("\\mathfrak{}", "𝔉", "fraktur", Some("amssymb")),
            arg("\\mathbf{}", "𝐱", "bold", None),
        ],
    ),
    (
        "Delimiters",
        &[
            s("\\langle", "⟨", "left angle bracket"),
            s("\\rangle", "⟩", "right angle bracket"),
            s("\\lfloor", "⌊", "left floor"),
            s("\\rfloor", "⌋", "right floor"),
            s("\\lceil", "⌈", "left ceiling"),
            s("\\rceil", "⌉", "right ceiling"),
            s("\\{", "{", "left brace"),
            s("\\}", "}", "right brace"),
            s("\\|", "‖", "double bar"),
            amsmath("\\lvert", "|", "left bar"),
            amsmath("\\rvert", "|", "right bar"),
            amsmath("\\lVert", "‖", "left double bar"),
            amsmath("\\rVert", "‖", "right double bar"),
        ],
    ),
    (
        "Miscellaneous",
        &[
            s("\\infty", "∞", "infinity"),
            s("\\forall", "∀", "for all"),
            s("\\exists", "∃", "there exists"),
            ams("\\nexists", "∄", "there does not exist"),
            s("\\emptyset", "∅", "empty set"),
            ams("\\varnothing", "⌀", "empty set, variant"),
            s("\\aleph", "ℵ", "aleph"),
            s("\\hbar", "ℏ", "h-bar"),
            s("\\ell", "ℓ", "script l"),
            s("\\Re", "ℜ", "real part"),
            s("\\Im", "ℑ", "imaginary part"),
            s("\\wp", "℘", "Weierstrass p"),
            s("\\angle", "∠", "angle"),
            s("\\triangle", "△", "triangle"),
            s("\\prime", "′", "prime"),
            s("\\ldots", "…", "dots on the line"),
            s("\\cdots", "⋯", "centred dots"),
            s("\\vdots", "⋮", "vertical dots"),
            s("\\ddots", "⋱", "diagonal dots"),
            s("\\top", "⊤", "top"),
            s("\\bot", "⊥", "bottom"),
            ams("\\therefore", "∴", "therefore"),
            ams("\\because", "∵", "because"),
            ams("\\square", "□", "square"),
            ams("\\blacksquare", "■", "filled square"),
            s("\\dagger", "†", "dagger"),
            s("\\star", "⋆", "star"),
            s("\\degree", "°", "degree"),
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
