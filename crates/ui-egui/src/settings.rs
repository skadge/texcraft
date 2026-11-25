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
    ("Appearance", "settings"),
    ("Editor", "file-code"),
    ("Building", "play"),
    ("Keyboard", "text-cursor-input"),
    ("Review", "message-square-text"),
    ("AI", "circle-question-mark"),
    ("Accounts", "link"),
];

pub(crate) fn show(app: &mut App, ctx: &egui::Context) {
    let t = app.tokens;
    let mut close = false;
    let modal = egui::Modal::new(egui::Id::unique("texcraft-settings")).frame(widgets::dialog_frame(ctx)).show(ctx, |ui| {
        ui.set_width(720.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new(tl!("Settings")).font(t.title()));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if widgets::icon_button(ui, &t, "x", tl!("Close"), true).clicked() {
                    close = true;
                }
            });
        });
        ui.add_space(t.unit * 3.0);
        ui.horizontal_top(|ui| {
            ui.vertical(|ui| {
                ui.set_width(160.0);
                for (i, (label, icon)) in TABS.iter().enumerate() {
                    let row = widgets::Row {
                        depth: 0,
                        icon,
                        label: crate::i18n::t(label),
                        selected: app.view.settings_tab == i,
                        disclosure: None,
                        tag: None,
                        icon_color: None,
                    };
                    // A tab of a tab list, not a tree's item.
                    let r = widgets::row(ui, &t, &row);
                    ui.ctx().accesskit_node_builder(r.id, |node| node.set_role(egui::accesskit::Role::Tab));
                    if r.clicked() {
                        app.view.settings_tab = i;
                    }
                }
            });
            ui.add_space(t.unit * 4.0);
            ui.vertical(|ui| {
                ui.set_min_height(320.0);
                ui.spacing_mut().item_spacing.y = t.unit * 3.0;
                match app.view.settings_tab {
                    0 => appearance(app, ui, &t),
                    1 => editor(app, ui, &t),
                    2 => building(app, ui, &t),
                    3 => keyboard(app, ui, &t),
                    4 => review(app, ui, &t),
                    5 => crate::ai::settings(app, ui, &t),
                    _ => crate::forge::settings(app, ui, &t),
                }
            });
        });
    });
    if close || modal.should_close() {
        app.view.dialog = None;
    }
}

/// A setting: its name and explanation on the left, its control on the right. The control's
/// response, when it returns one, is named after the setting (AGENTS.md rule 21).
fn setting(ui: &mut egui::Ui, t: &Tokens, name: &str, help: &str, control: impl FnOnce(&mut egui::Ui) -> Option<egui::Response>) {
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
    let languages = crate::i18n::languages();
    let help = if languages.len() > 1 {
        tl!("The language of menus and messages.")
    } else {
        tl!("This build has English only: translations come from the craft-i18n catalogs.")
    };
    setting(ui, t, tl!("Interface language"), help, |ui| {
        let current = if app.prefs.language == "auto" {
            tl!("As the system").to_owned()
        } else {
            languages.iter().find(|(c, _)| *c == app.prefs.language).map_or_else(|| app.prefs.language.clone(), |(_, n)| n.clone())
        };
        ui.add_enabled_ui(languages.len() > 1, |ui| {
            let combo = craft_ui::ComboBox::from_id_salt("settings-language").width(180.0).selected_text(current).show_ui(ui, |ui| {
                let mut choose = |ui: &mut egui::Ui, code: &str, name: &str| {
                    if ui.selectable_label(app.prefs.language == code, name).clicked() {
                        app.prefs.language = code.to_owned();
                        app.prefs_changed = true;
                        crate::i18n::set_language(code);
                        app.session.set_interface_language(crate::i18n::current_code());
                    }
                };
                choose(ui, "auto", tl!("As the system"));
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
    setting(ui, t, tl!("Font size"), tl!("The size of the source text, in points."), |ui| {
        let mut size = app.prefs.editor_font_size;
        ui.label(format!("{size:.1} pt"));
        let r = ui.add(egui::Slider::new(&mut size, 10.0..=22.0).step_by(0.5).show_value(false));
        if r.changed() {
            app.prefs.editor_font_size = size;
            app.prefs_changed = true;
            app.apply_editor_font();
        }
        Some(r)
    });
    key_bindings(app, ui, t, "settings-keys-editor");
    setting(ui, t, tl!("Close brackets and environments"), tl!("Typing ( [ { adds the closing bracket, and \\begin{…} its \\end{…}."), |ui| {
        let mut on = app.prefs.auto_close;
        let r = ui.checkbox(&mut on, "");
        if r.changed() {
            app.prefs.auto_close = on;
            app.prefs_changed = true;
        }
        Some(r)
    });
    spelling(app, ui, t);
    let preview =
        egui::text::LayoutJob::simple_singleline("\\section{Introduction} % E = mc^2".to_owned(), app.tokens.code_font(), app.tokens.syntax_command);
    egui::Frame::new().fill(t.surface).stroke(t.hairline()).corner_radius(t.corner()).inner_margin(egui::Margin::same(10)).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(preview);
    });
}

/// The spelling languages offered, by tag; others can be set by agents (`language.spelling_options`).
const SPELLING_LANGUAGES: &[(&str, &str)] = &[
    ("en-US", "English (United States)"),
    ("en-GB", "English (United Kingdom)"),
    ("en-CA", "English (Canada)"),
    ("en-AU", "English (Australia)"),
    ("fr", "French"),
    ("de", "German"),
    ("es", "Spanish"),
    ("it", "Italian"),
    ("pt", "Portuguese"),
    ("pt-BR", "Portuguese (Brazil)"),
    ("nl", "Dutch"),
    ("pl", "Polish"),
    ("cs", "Czech"),
    ("ru", "Russian"),
    ("tr", "Turkish"),
    ("hr", "Croatian"),
    ("id", "Indonesian"),
    ("sv", "Swedish"),
    ("da", "Danish"),
    ("nb", "Norwegian"),
    ("fi", "Finnish"),
];

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
    let (language, from) = app.session.spelling_language();
    let name = |tag: &str| {
        SPELLING_LANGUAGES.iter().find(|(c, _)| c.eq_ignore_ascii_case(tag)).map_or_else(|| tag.to_owned(), |(_, n)| crate::i18n::t(n).to_owned())
    };
    let help = match (app.session.spelling_ready(), from) {
        (Err(why), _) if options.on => why,
        (_, texcraft_engine::spelling::LanguageFrom::Document) => {
            tl!("Checking {language}, as the document's babel or polyglossia sets it.").replace("{language}", &name(&language))
        }
        (_, texcraft_engine::spelling::LanguageFrom::Interface) => {
            tl!("Checking {language}, the interface's language: the document names none.").replace("{language}", &name(&language))
        }
        _ => tl!("Checking {language}.").replace("{language}", &name(&language)),
    };
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
        ui.label(format!("{ms:.0} ms"));
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
}

/// The kind of PDF a build makes, and for print its standard, bleed and crop marks.
fn output(app: &mut App, ui: &mut egui::Ui, t: &Tokens, options: texcraft_engine::BuildOptions) {
    let help = tl!(
        "For reading on a screen; accessible (tagged, PDF/UA, checked for screen readers); or for print (a PDF/X, checked as printers check it)."
    );
    setting(ui, t, tl!("PDF for"), help, |ui| {
        let kinds = [Output::Screen, Output::Accessible, Output::Print];
        let labels = [(tl!("Screen"), None), (tl!("Accessible"), None), (tl!("Print"), None)];
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
        let options = [(tl!("Standard"), None), (tl!("Vim"), None)];
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
