//! The Settings dialog: appearance, the editor, building and the keyboard. Each change applies at
//! once and is kept in the preferences; engine settings change through engine commands.

use egui::{Align, Layout, RichText};
use serde_json::json;
use texcraft_engine::{Engine, Output, PdfX};

use crate::dialogs::Dialog;
use crate::prefs::ThemePref;
use crate::theme::Tokens;
use crate::{App, widgets};

const TABS: &[(&str, &str)] = &[
    (tn!("Appearance"), "settings"),
    (tn!("Editor"), "file-code"),
    (tn!("Building"), "play"),
    (tn!("Keyboard"), "text-cursor-input"),
    (tn!("Review"), "message-square-text"),
    (tn!("AI"), crate::ai::ICON),
    (tn!("Accounts"), "link"),
];

pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let narrow = crate::sheet::narrow(ctx, &t);
    // On a phone the sections are a list, each opening as a page of its own with Back to it.
    let page = narrow && app.view.settings_page;
    let leave = if page { crate::sheet::Leave::Back(tl!("Back to Settings")) } else { crate::sheet::Leave::Close };
    let mut close = false;
    let shown = crate::sheet::show(ctx, &t, "texcraft-settings", 720.0, leave, |ui| {
        if narrow {
            if page {
                let label = TABS.get(app.view.settings_tab).map_or("", |(l, _)| crate::i18n::t(l));
                crate::dialogs::title(ui, &t, label);
                ui.spacing_mut().item_spacing.y = t.unit * 3.0;
                section(app, ui, &t);
            } else {
                crate::dialogs::title(ui, &t, tl!("Settings"));
                sections(app, ui, &t, true);
            }
            return;
        }
        ui.horizontal(|ui| {
            ui.label(RichText::new(tl!("Settings")).font(t.title()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, &t, "x", tl!("Close"), true).clicked() {
                    close = true;
                }
            });
        });
        ui.add_space(t.unit * 3.0);
        // As tall as the window allows (the AI tab's Connect is not below the fold); a tab taller
        // than that (a long language, a small window) scrolls, so the title and Close stay.
        let room = (ui.ctx().content_rect().height() - 160.0).max(200.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(160.0);
                sections(app, ui, &t, false);
            });
            ui.add_space(t.unit * 4.0);
            egui::ScrollArea::vertical().id_salt("settings-tab").max_height(room).auto_shrink([false, true]).show(ui, |ui| {
                ui.vertical(|ui| {
                    ui.set_min_height(room.min(640.0));
                    ui.spacing_mut().item_spacing.y = t.unit * 3.0;
                    section(app, ui, &t);
                });
            });
        });
    });
    if shown.back {
        app.view.settings_page = false;
        app.view.settings_focus = true;
    }
    if close || shown.close {
        app.view.dialog = None;
        app.view.settings_page = false;
    }
}

/// Chooses the tab named `name` (its English name in lower case: `editor`, `ai`), if there is one.
pub(crate) fn choose_tab(app: &mut App, name: &str) {
    if let Some(i) = TABS.iter().position(|(label, _)| label.eq_ignore_ascii_case(name)) {
        app.view.settings_tab = i;
        app.view.settings_page = true;
    }
}

/// The sections: tabs beside the section shown, or on a phone a list whose rows open them.
fn sections(app: &mut App, ui: &mut egui::Ui, t: &Tokens, list: bool) {
    for (i, (label, icon)) in TABS.iter().enumerate() {
        let selected = !list && app.view.settings_tab == i;
        let row =
            widgets::Row { depth: 0, icon, label: crate::i18n::t(label), selected, disclosure: None, tag: None, icon_color: None, muted: false };
        let r = widgets::row(ui, t, &row);
        // A tab of a tab list, or a button opening its page; not a tree's item.
        let role = if list { egui::accesskit::Role::Button } else { egui::accesskit::Role::Tab };
        ui.ctx().accesskit_node_builder(r.id, |node| node.set_role(role));
        if list && app.view.settings_tab == i && std::mem::take(&mut app.view.settings_focus) {
            r.request_focus();
        }
        if r.clicked() {
            app.view.settings_tab = i;
            if list {
                app.view.settings_page = true;
                crate::sheet::focus_leave(ui.ctx(), "texcraft-settings");
            }
        }
    }
}

/// The section chosen.
fn section(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    match app.view.settings_tab {
        0 => appearance(app, ui, t),
        1 => editor(app, ui, t),
        2 => building(app, ui, t),
        3 => keyboard(app, ui, t),
        4 => review(app, ui, t),
        5 => crate::ai::settings(app, ui, t),
        _ => crate::forge::settings(app, ui, t),
    }
}

/// A setting: its name and explanation on the left, its control on the right. The control's
/// response, when it returns one, is named after the setting (AGENTS.md rule 21).
fn setting(ui: &mut egui::Ui, t: &Tokens, name: &str, help: &str, control: impl FnOnce(&mut egui::Ui) -> Option<egui::Response>) {
    // Too narrow for both side by side (a phone): the control goes under its explanation.
    if ui.available_width() < 480.0 {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(name).font(t.strong()));
            ui.label(RichText::new(help).font(t.small()).color(t.text_muted));
            ui.add_space(t.unit);
            ui.horizontal_wrapped(|ui| {
                if let Some(response) = control(ui) {
                    let _ = response.accessible_name(name);
                }
            });
        });
        return;
    }
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.set_width(300.0);
            ui.spacing_mut().item_spacing.y = 2.0;
            ui.label(RichText::new(name).font(t.strong()));
            ui.label(RichText::new(help).font(t.small()).color(t.text_muted));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if let Some(response) = control(ui) {
                let _ = response.accessible_name(name);
            }
        });
    });
}

fn appearance(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    setting(ui, t, tl!("Theme"), tl!("Light or dark, or as the system is set."), |ui| {
        let themes = [ThemePref::System, ThemePref::Light, ThemePref::Dark];
        let selected = themes.iter().position(|th| *th == app.prefs.theme).unwrap_or(0);
        let options = [(tl!("System"), None), (tl!("Light"), None), (tl!("Dark"), None)];
        if let Some(i) = widgets::segmented(ui, t, "settings-theme", &options, selected) {
            let ctx = ui.ctx().clone();
            app.set_theme(&ctx, themes.get(i).copied().unwrap_or_default());
        }
        None
    });
    if app.desktop_window {
        setting(
            ui,
            t,
            tl!("System title bar"),
            tl!("The system's title bar above the window, in place of the window's buttons in the top bar."),
            |ui| {
                let mut on = app.prefs.system_title_bar;
                let r = ui.checkbox(&mut on, "");
                if r.changed() {
                    app.prefs.system_title_bar = on;
                    app.prefs_changed = true;
                }
                Some(r)
            },
        );
    }
    setting(ui, t, tl!("Show tips at startup"), tl!("A tip of the day on the start page, about a feature that is easy to miss."), |ui| {
        let mut on = app.prefs.show_tips;
        let r = ui.checkbox(&mut on, "");
        if r.changed() {
            crate::tips::set_shown(app, on);
        }
        Some(r)
    });
    let languages = crate::i18n::languages();
    let help = if languages.len() > 1 {
        tl!("The language of menus and messages.")
    } else {
        tl!("This build has English only: translations come from the craft-i18n catalogs.")
    };
    setting(ui, t, tl!("Interface language"), help, |ui| {
        let current = if app.prefs.language == "auto" {
            tl!("System").to_owned()
        } else {
            languages.iter().find(|(c, _)| *c == app.prefs.language).map_or_else(|| app.prefs.language.clone(), |(_, n)| n.clone())
        };
        ui.add_enabled_ui(languages.len() > 1, |ui| {
            let combo =
                craft_ui::ComboBox::from_id_salt("settings-language").width(180.0).height(f32::INFINITY).selected_text(current).show_ui(ui, |ui| {
                    let mut choose = |ui: &mut egui::Ui, code: &str, name: &str| {
                        if ui.selectable_label(app.prefs.language == code, name).clicked() {
                            app.prefs.language = code.to_owned();
                            app.prefs_changed = true;
                            crate::i18n::set_language(code);
                            app.session.set_interface_language(&crate::i18n::spelling_fallback());
                        }
                    };
                    choose(ui, "auto", tl!("System"));
                    for (code, name) in &languages {
                        choose(ui, code, name);
                    }
                });
            let _ = combo.response.accessible_name(tl!("Interface language"));
        });
        None
    });
}

fn editor(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    setting(
        ui,
        t,
        tl!("Font size"),
        tl!("The size of the source text, in points; the formatted view's text follows it. Ctrl with the mouse wheel over the text changes it too."),
        |ui| {
            let mut size = app.prefs.editor_font_size;
            ui.label(crate::i18n::measure(f64::from(size), 1, "{n} pt"));
            let r = ui.add(egui::Slider::new(&mut size, crate::prefs::TEXT_SIZE_MIN..=crate::prefs::TEXT_SIZE_MAX).step_by(0.5).show_value(false));
            if r.changed() {
                app.set_text_size(size);
            }
            Some(r)
        },
    );
    let mut autosave = app.session.settings.autosave;
    setting(
        ui,
        t,
        tl!("Save automatically"),
        tl!(
            "Write changes shortly after typing stops, and keep versions of the project by themselves. Off, File › Save saves, and closing asks about changes not saved."
        ),
        |ui| {
            let r = ui.add(egui::Checkbox::without_text(&mut autosave));
            if r.changed() {
                app.run("project.autosave", json!({"on": autosave}));
            }
            Some(r)
        },
    );
    key_bindings(app, ui, t, "settings-keys-editor");
    setting(
        ui,
        t,
        tl!("Close brackets and environments"),
        tl!("Typing an opening bracket, parenthesis or brace adds the closing one, and \\begin{…} adds its \\end{…}."),
        |ui| {
            let mut on = app.prefs.auto_close;
            let r = ui.checkbox(&mut on, "");
            if r.changed() {
                app.prefs.auto_close = on;
                app.prefs_changed = true;
            }
            Some(r)
        },
    );
    spelling(app, ui, t);
    grammar(app, ui, t);
    let preview =
        egui::text::LayoutJob::simple_singleline("\\section{Introduction} % E = mc^2".to_owned(), app.tokens.code_font(), app.tokens.syntax_command);
    egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(preview);
    });
}

/// The spelling languages offered, by tag; others can be set by agents (`language.spelling_options`).
pub(crate) const SPELLING_LANGUAGES: &[(&str, &str)] = &[
    ("en-US", tn!("English (United States)")),
    ("en-GB", tn!("English (United Kingdom)")),
    ("en-CA", tn!("English (Canada)")),
    ("en-AU", tn!("English (Australia)")),
    ("fr", tn!("French")),
    ("de", tn!("German")),
    ("es", tn!("Spanish")),
    ("it", tn!("Italian")),
    ("pt", tn!("Portuguese")),
    ("pt-BR", tn!("Portuguese (Brazil)")),
    ("nl", tn!("Dutch")),
    ("pl", tn!("Polish")),
    ("cs", tn!("Czech")),
    ("ru", tn!("Russian")),
    ("tr", tn!("Turkish")),
    ("hr", tn!("Croatian")),
    ("id", tn!("Indonesian")),
    ("sv", tn!("Swedish")),
    ("da", tn!("Danish")),
    ("nb", tn!("Norwegian")),
    ("fi", tn!("Finnish")),
];

/// A spelling language's name by its tag (`en-GB`: English (United Kingdom)); the tag itself for
/// one not offered.
pub(crate) fn spelling_language_name(tag: &str) -> String {
    SPELLING_LANGUAGES.iter().find(|(c, _)| c.eq_ignore_ascii_case(tag)).map_or_else(|| tag.to_owned(), |(_, n)| crate::i18n::t(n).to_owned())
}

/// What spelling checks now and where its language comes from, or why nothing is checked.
pub(crate) fn spelling_help(app: &App) -> String {
    let (language, from) = app.session.spelling_language();
    let name = spelling_language_name(&language);
    match (app.session.spelling_ready(), from) {
        (Err(why), _) if app.session.settings.spelling.on => crate::i18n::msg(&why),
        (_, texcraft_engine::spelling::LanguageFrom::Document) => {
            tl!("Checking {language}, as the document's babel or polyglossia sets it.").replace("{language}", &name)
        }
        (_, texcraft_engine::spelling::LanguageFrom::Interface) => {
            tl!("Checking {language}, from the interface's language and the system's region: the document names none.").replace("{language}", &name)
        }
        _ => tl!("Checking {language}.").replace("{language}", &name),
    }
}

/// Spell checking: on, the language (with what is checked now, or why nothing is), comments.
fn spelling(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    let options = app.session.settings.spelling.clone();
    setting(ui, t, tl!("Check spelling"), tl!("Misspelled words in the text are underlined; commands, math and verbatim text are skipped."), |ui| {
        let mut on = options.on;
        let r = ui.add(egui::Checkbox::without_text(&mut on));
        if r.changed() {
            app.run("language.spelling_options", json!({"on": on}));
        }
        Some(r)
    });
    let name = spelling_language_name;
    let help = spelling_help(app);
    setting(ui, t, tl!("Spelling language"), &help, |ui| {
        let current = if options.language == "auto" { tl!("From the document").to_owned() } else { name(&options.language) };
        let mut chosen = None;
        let combo = craft_ui::ComboBox::from_id_salt("settings-spelling-language").width(200.0).selected_text(current).show_ui(ui, |ui| {
            if ui.selectable_label(options.language == "auto", tl!("From the document")).clicked() {
                chosen = Some("auto");
            }
            for (tag, n) in SPELLING_LANGUAGES {
                if ui.selectable_label(options.language.eq_ignore_ascii_case(tag), crate::i18n::t(n)).clicked() {
                    chosen = Some(tag);
                }
            }
        });
        let _ = combo.response.accessible_name(tl!("Spelling language"));
        if let Some(tag) = chosen {
            app.run("language.spelling_options", json!({"language": tag}));
        }
        None
    });
    setting(ui, t, tl!("Check comments"), tl!("Also check the text after % in the source."), |ui| {
        let mut on = options.comments;
        let r = ui.add_enabled(options.on, egui::Checkbox::without_text(&mut on));
        if r.changed() {
            app.run("language.spelling_options", json!({"comments": on}));
        }
        Some(r)
    });
}

/// The name of a group of grammar rules (`grammar.rule`'s `group`).
fn grammar_group(id: &str) -> &'static str {
    match id {
        "agreement" => tl!("Agreement"),
        "grammar" => tl!("Grammar and word order"),
        "punctuation" => tl!("Punctuation and capitals"),
        "repetition" => tl!("Repeated and redundant words"),
        "usage" => tl!("Word choice and usage"),
        _ => tl!("Style and readability"),
    }
}

/// Grammar checking: on, its groups of rules, and the rules turned off from the text's menu.
fn grammar(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    // A build without Harper (the browser's) says so where the switch would be (AGENTS.md rule 15).
    if !texcraft_engine::grammar::OFFLINE {
        let help = if texcraft_engine::ai::available() && app.session.ai_enabled() {
            tl!("Grammar checking is not in this build; Check Grammar with AI is available.")
        } else {
            tl!("Grammar checking is not in this build.")
        };
        setting(ui, t, tl!("Check grammar (English)"), help, |ui| {
            let mut off = false;
            Some(ui.add_enabled(false, egui::Checkbox::without_text(&mut off)))
        });
        return;
    }
    let options = app.session.settings.grammar.clone();
    let help = match app.session.grammar_ready() {
        Err(why) if options.on => crate::i18n::msg(&why),
        _ => tl!("Grammar problems in English text are underlined with a double line, checked on this computer by Harper; spelling stays with the spell checker.").to_owned(),
    };
    setting(ui, t, tl!("Check grammar (English)"), &help, |ui| {
        let mut on = options.on;
        let r = ui.add(egui::Checkbox::without_text(&mut on));
        if r.changed() {
            app.run("grammar.options", json!({"on": on}));
            app.prefs_changed = true;
        }
        Some(r)
    });
    for (id, _) in texcraft_engine::grammar::GROUPS {
        let name = grammar_group(id);
        setting(ui, t, name, tl!("Rules of this kind are checked."), |ui| {
            let mut on = !options.groups_off.contains(id);
            let r = ui.add_enabled(options.on, egui::Checkbox::without_text(&mut on));
            if r.changed() {
                app.run("grammar.rule", json!({"group": id, "on": on}));
                app.prefs_changed = true;
            }
            Some(r)
        });
    }
    for rule in &options.rules_off {
        setting(ui, t, rule, tl!("Turned off from the text's menu (Turn Off This Rule)."), |ui| {
            let r = ui.button(tl!("Turn On"));
            if r.clicked() {
                app.run("grammar.rule", json!({"rule": rule, "on": true}));
                app.prefs_changed = true;
            }
            Some(r)
        });
    }
}

fn building(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    let mut auto = app.session.settings.auto_compile;
    let delay = app.session.settings.auto_compile_delay_ms;
    setting(ui, t, tl!("Compile automatically"), tl!("Build the PDF when typing pauses."), |ui| {
        let r = ui.add(egui::Checkbox::without_text(&mut auto));
        if r.changed() {
            app.run("build.auto", json!({"on": auto, "delay_ms": delay}));
        }
        Some(r)
    });
    setting(ui, t, tl!("Pause before building"), tl!("How long typing must stop before an automatic build."), |ui| {
        let mut ms = delay as f64;
        ui.label(crate::i18n::measure(ms, 0, "{n} ms"));
        let r = ui.add_enabled(auto, egui::Slider::new(&mut ms, 300.0..=3000.0).step_by(100.0).show_value(false));
        if r.changed() {
            app.run("build.auto", json!({"on": auto, "delay_ms": ms as u64}));
        }
        Some(r)
    });
    let options = app.session.settings.build;
    setting(ui, t, tl!("Engine"), tl!("How the document is typeset: as LuaLaTeX, as pdfLaTeX, or chosen from the document."), |ui| {
        let engines = [(Engine::Auto, tl!("Chosen from the document")), (Engine::LuaLatex, "LuaLaTeX"), (Engine::PdfLatex, "pdfLaTeX")];
        let current = engines.iter().find(|(e, _)| *e == options.engine).map_or("", |(_, n)| *n);
        let mut chosen = None;
        let combo = craft_ui::ComboBox::from_id_salt("settings-engine").width(200.0).selected_text(current).show_ui(ui, |ui| {
            for (engine, name) in engines {
                if ui.selectable_label(options.engine == engine, name).clicked() {
                    chosen = Some(engine);
                }
            }
        });
        let _ = combo.response.accessible_name(tl!("Engine"));
        if let Some(engine) = chosen {
            let id = match engine {
                Engine::Auto => "auto",
                Engine::LuaLatex => "lualatex",
                Engine::PdfLatex => "pdflatex",
            };
            app.run("build.options", json!({"engine": id}));
        }
        None
    });
    setting(ui, t, tl!("Stop at the first error"), tl!("Otherwise the build goes on and reports every error."), |ui| {
        let mut on = options.halt_on_error;
        let r = ui.add(egui::Checkbox::without_text(&mut on));
        if r.changed() {
            app.run("build.options", json!({"halt_on_error": on}));
        }
        Some(r)
    });
    setting(ui, t, tl!("Draft images"), tl!("Draw images as framed boxes with their names, for faster builds."), |ui| {
        let mut on = options.draft_images;
        let r = ui.add(egui::Checkbox::without_text(&mut on));
        if r.changed() {
            app.run("build.options", json!({"draft_images": on}));
        }
        Some(r)
    });
    output(app, ui, t, options);
    // Only the browser's engine downloads packages through TeXCraft; the desktop's retex has its
    // own settings.
    if cfg!(target_arch = "wasm32") {
        package_server(app, ui, t);
    }
    hidden_messages(app, ui, t);
}

/// Where the browser build downloads the TeX Live packages its own TeX files lack: TeXCraft's
/// server, another one (its address typed, applied with Enter) or none (`build.package_server`).
fn package_server(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    use texcraft_engine::packages::PackageServer;
    let server = app.session.settings.package_server.clone();
    let help = tl!(
        "The LaTeX packages a document needs and the browser's own TeX files lack are downloaded from this server, once. Off: the build goes on without them."
    );
    let choices = [tl!("TeXCraft's server"), tl!("Another server"), tl!("Off")];
    let custom = matches!(server, PackageServer::Custom(_)) || app.view.package_server_draft.is_some();
    let shown = match server {
        PackageServer::Off if !custom => 2,
        _ if custom => 1,
        _ => 0,
    };
    let mut chosen = None;
    setting(ui, t, tl!("Package server"), help, |ui| {
        let combo = craft_ui::ComboBox::from_id_salt("settings-package-server").width(200.0).selected_text(choices[shown]).show_ui(ui, |ui| {
            for (i, name) in choices.iter().enumerate() {
                if ui.selectable_label(i == shown, *name).clicked() {
                    chosen = Some(i);
                }
            }
        });
        let _ = combo.response.accessible_name(tl!("Package server"));
        None
    });
    match chosen {
        Some(0) => {
            app.view.package_server_draft = None;
            app.run("build.package_server", json!({"server": "default"}));
        }
        Some(1) if !custom => app.view.package_server_draft = Some(String::new()),
        Some(2) => {
            app.view.package_server_draft = None;
            app.run("build.package_server", json!({"server": "off"}));
        }
        _ => {}
    }
    if !(custom || chosen == Some(1)) {
        return;
    }
    let mut text = app.view.package_server_draft.clone().unwrap_or_else(|| server.base().unwrap_or_default().to_owned());
    let mut apply = false;
    let help =
        tl!("The https:// address of a TeX Live mirror laid out by year, then snapshot, whose server lets this site read it. Press Enter to use it.");
    setting(ui, t, tl!("Server address"), help, |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut text).hint_text("https://…/tlnet").desired_width(260.0));
        apply = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        Some(r)
    });
    if apply && app.run("build.package_server", json!({"server": text.trim()})).is_some() {
        app.view.package_server_draft = None;
    } else {
        app.view.package_server_draft = Some(text);
    }
}

/// The kinds of build message hidden from the log (Hide Like This), this project's then every
/// project's, each with Show Again.
fn hidden_messages(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    let project = app.session.hidden_for_project();
    let user = app.session.hidden_for_user().to_vec();
    ui.add_space(t.unit * 3.0);
    ui.label(RichText::new(tl!("Hidden messages")).font(t.strong()));
    if project.is_empty() && user.is_empty() {
        let none = tl!("No kind of message is hidden. In the build log, Hide Like This hides a warning's kind, here or in all projects.");
        ui.label(RichText::new(none).font(t.small()).color(t.text_muted));
        return;
    }
    let mut show = None;
    for (kinds, scope, help) in [(&project, "project", tl!("Hidden in this project.")), (&user, "user", tl!("Hidden in all projects."))] {
        for kind in kinds {
            setting(ui, t, kind, help, |ui| {
                // Named after its label, then the kind it shows again.
                let r = ui.button(tl!("Show Again"));
                if r.clicked() {
                    show = Some((kind.clone(), scope));
                }
                let _ = r.accessible_name(format!("{}: {kind}", tl!("Show Again")));
                None
            });
        }
    }
    if let Some((kind, scope)) = show {
        app.run("build.show_messages", json!({"kind": kind, "scope": scope}));
        app.prefs_changed = true;
    }
}

/// The kind of PDF a build makes, and for print its standard, bleed and crop marks.
fn output(app: &mut App, ui: &mut egui::Ui, t: &Tokens, options: texcraft_engine::BuildOptions) {
    let help = tl!(
        "For reading on a screen; accessible (tagged, PDF/UA, checked for screen readers); or for print (a PDF/X, checked as printers check it)."
    );
    setting(ui, t, tl!("PDF for"), help, |ui| {
        let kinds = [Output::Screen, Output::Accessible, Output::Print];
        let labels = [(tl!("Screen"), None), (tl!("Accessible"), None), (crate::i18n::tc("pdf", "Print"), None)];
        let selected = kinds.iter().position(|k| *k == options.output).unwrap_or(0);
        if let Some(i) = widgets::segmented(ui, t, "settings-output", &labels, selected) {
            let kind = kinds.get(i).copied().unwrap_or_default();
            app.run("build.options", json!({"output": kind}));
        }
        None
    });
    if options.output != Output::Print {
        return;
    }
    setting(
        ui,
        t,
        tl!("PDF/X standard"),
        tl!("PDF/X-4 keeps colour profiles and transparency; PDF/X-1a is CMYK only, for older print workflows."),
        |ui| {
            let standards = [PdfX::X4, PdfX::X1a];
            let mut chosen = None;
            let combo = craft_ui::ComboBox::from_id_salt("settings-pdfx").width(200.0).selected_text(options.pdfx.name()).show_ui(ui, |ui| {
                for standard in standards {
                    if ui.selectable_label(options.pdfx == standard, standard.name()).clicked() {
                        chosen = Some(standard);
                    }
                }
            });
            let _ = combo.response.accessible_name(tl!("PDF/X standard"));
            if let Some(standard) = chosen {
                app.run("build.options", json!({"pdfx": standard}));
            }
            None
        },
    );
    setting(ui, t, tl!("Bleed"), tl!("How far the art may run past the page's edges, to be trimmed off; printers often ask for 3 mm."), |ui| {
        let mut mm = options.bleed_mm;
        let r = ui.add(egui::Slider::new(&mut mm, 0.0..=texcraft_engine::MAX_BLEED_MM).step_by(0.5).suffix(" mm"));
        if r.changed() {
            app.run("build.options", json!({"bleed_mm": mm}));
        }
        Some(r)
    });
    setting(ui, t, tl!("Crop marks"), tl!("Marks outside the bleed that show where to trim."), |ui| {
        let mut on = options.crop_marks;
        let r = ui.add(egui::Checkbox::without_text(&mut on));
        if r.changed() {
            app.run("build.options", json!({"crop_marks": on}));
        }
        Some(r)
    });
}

/// Standard or Vim key bindings in the editor; shown in the Editor and the Keyboard tabs, where
/// people look for it.
fn key_bindings(app: &mut App, ui: &mut egui::Ui, t: &Tokens, salt: &str) {
    setting(ui, t, tl!("Editor key bindings"), tl!("Standard editing keys, or Vim's modes and commands."), |ui| {
        let options = [(crate::i18n::tc("keys", "Standard"), None), (tl!("Vim"), None)];
        if let Some(i) = widgets::segmented(ui, t, salt, &options, usize::from(app.prefs.vim)) {
            app.prefs.vim = i == 1;
            app.prefs_changed = true;
        }
        None
    });
}

/// The name comments carry, and the key that tells this person's from others of the same name.
fn review(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    let current = app.prefs.identity.clone();
    let mut name = current.as_ref().map_or_else(|| app.view.identity_draft.clone(), |i| i.name.clone());
    setting(ui, t, tl!("Your name"), tl!("Shown with your comments and replies."), |ui| {
        let r = ui.add(egui::TextEdit::singleline(&mut name).desired_width(200.0));
        Some(r)
    });
    match current {
        Some(mut identity) => {
            if name.trim() != identity.name && !name.trim().is_empty() {
                identity.name = name.trim().to_owned();
                app.session.set_author(Some(identity.author()));
                app.prefs.identity = Some(identity.clone());
                app.prefs_changed = true;
            }
            let fingerprint = identity.author().fingerprint();
            setting(ui, t, tl!("Your key"), tl!("Made on this computer for your comments; others see this short form of it."), |ui| {
                Some(ui.label(RichText::new(fingerprint).font(t.code_font()).color(t.text)))
            });
            ui.label(
                RichText::new(tl!(
                    "The key's secret half is kept in the preferences file. There are no accounts: the name and key are all a comment carries."
                ))
                .font(t.small())
                .color(t.text_muted),
            );
        }
        None => {
            app.view.identity_draft = name.clone();
            setting(ui, t, tl!("Your key"), tl!("None yet: it is made with your first comment, or now."), |ui| {
                let r = ui.add_enabled(!name.trim().is_empty(), egui::Button::new(tl!("Make Key")));
                if r.clicked() {
                    match crate::prefs::Identity::new(&name) {
                        Ok(identity) => {
                            app.session.set_author(Some(identity.author()));
                            app.prefs.identity = Some(identity);
                            app.prefs_changed = true;
                        }
                        Err(e) => app.view.status = Some((e, true)),
                    }
                }
                Some(r)
            });
        }
    }
}

fn keyboard(app: &mut App, ui: &mut egui::Ui, t: &Tokens) {
    key_bindings(app, ui, t, "settings-keys-keyboard");
    setting(ui, t, tl!("Keyboard shortcuts"), tl!("See and change the shortcut of every command."), |ui| {
        if ui.button(tl!("Change Shortcuts…")).clicked() {
            app.view.dialog = Some(Dialog::Shortcuts);
        }
        None
    });
}
