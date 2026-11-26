//! Tables and figures in the formatted view: a cell is edited in place and changes only its own
//! source, the table's toolbar inserts a row, and the figure dialog changes a caption.

use egui::accesskit::Role;
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::project::ProjectPath;
use texcraft_ui_egui::App;
use texcraft_ui_egui::prefs::Prefs;

const TABLE: &str = "Before.\n\n\\begin{tabular}{lr}\n  alpha & beta \\\\\n  gamma & delta \\\\\n\\end{tabular}\n\nAfter.\n";

pub(crate) const FIGURE: &str = "Before.\n\n\\begin{figure}[h]\n  \\centering\n  \\includegraphics{plot}\n  \\caption{Old.}\n\\end{figure}\n";

fn text(harness: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| harness.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

pub(crate) fn formatted(text: &str) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": text})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut harness = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    harness.run();
    harness.get_by_label("Formatted").click();
    harness.run();
    harness
}

fn key(harness: &mut Harness<'_, App>, modifiers: egui::Modifiers, key: egui::Key) {
    harness.key_press_modifiers(modifiers, key);
    harness.run();
}

#[test]
fn a_cell_is_edited_in_place_and_tab_goes_to_the_next_cell() {
    let mut h = formatted(TABLE);
    assert!(h.query_by_role(Role::Table).is_some(), "the tabular is not shown as a table");
    h.get_by_role_and_label(Role::Cell, "beta").click();
    h.run();
    h.get_by_label("main.tex formatted").focus();
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::End);
    h.get_by_label("main.tex formatted").type_text("!");
    h.run();
    assert_eq!(text(&mut h), TABLE.replace("beta", "beta!"), "typing in a cell changed more than the cell");

    // Tab selects the next cell's text: typing replaces it.
    key(&mut h, egui::Modifiers::NONE, egui::Key::Tab);
    h.get_by_label("main.tex formatted").type_text("g&m");
    h.run();
    assert_eq!(text(&mut h), TABLE.replace("beta", "beta!").replace("gamma", "g\\&m"));
}

#[test]
fn the_toolbar_inserts_a_row_below_the_caret() {
    let mut h = formatted(TABLE);
    h.get_by_role_and_label(Role::Cell, "alpha").click();
    h.run();
    h.get_by_role(Role::Toolbar);
    h.get_by_role_and_label(Role::Button, "Row below").click();
    h.run();
    h.run();
    let now = text(&mut h);
    assert!(
        now.starts_with("Before.\n\n\\begin{tabular}{lr}\n  alpha & beta \\\\\n")
            && now.ends_with("  gamma & delta \\\\\n\\end{tabular}\n\nAfter.\n"),
        "{now}"
    );
    assert_eq!(now.lines().count(), TABLE.lines().count() + 1, "{now}");
    assert_eq!(h.query_all_by_role(Role::Row).count(), 3, "the new row is not shown");
}

#[test]
fn a_table_the_grid_cannot_show_says_why() {
    let h = formatted("\\begin{tabular}{ll}\n\\multirow{2}{*}{a} & b\n\\end{tabular}\n");
    assert!(h.query_by_role(Role::Table).is_none());
    assert!(h.query_by_label_contains("a cell spans rows").is_some(), "the reason is not given");
}

#[test]
fn the_figure_dialog_changes_the_caption_alone() {
    let mut h = formatted(FIGURE);
    h.get_by_role_and_label(Role::Button, "Edit Figure…").click();
    h.run();
    h.get_by_label("Caption (LaTeX)").click();
    h.run();
    key(&mut h, egui::Modifiers::COMMAND, egui::Key::End);
    h.get_by_label("Caption (LaTeX)").type_text(" New");
    h.run();
    h.get_by_role_and_label(Role::Button, "Apply").click();
    h.run();
    assert_eq!(text(&mut h), FIGURE.replace("Old.", "Old. New"));
}

fn press(h: &Harness<'_, App>, name: &str) -> bool {
    use egui_kittest::kittest::NodeT;
    h.get_by_role_and_label(Role::Button, name).accesskit_node().data().toggled() == Some(egui::accesskit::Toggled::True)
}

#[test]
fn the_toolbar_makes_a_header_row_and_merges_and_splits_cells() {
    let mut h = formatted(TABLE);
    h.get_by_role_and_label(Role::Cell, "alpha").click();
    h.run();
    assert!(!press(&h, "Header row"));
    h.get_by_role_and_label(Role::Button, "Header row").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), TABLE.replace("alpha & beta", "\\textbf{alpha} & \\textbf{beta}"), "more than the first row changed");
    assert!(press(&h, "Header row"), "Header row is not shown pressed");
    h.get_by_role_and_label(Role::Button, "Header row").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), TABLE);

    // Shift+Right from the first cell into the second selects across them: Merge cells.
    h.get_by_role_and_label(Role::Cell, "gamma").click();
    h.run();
    h.get_by_label("main.tex formatted").focus();
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Home);
    for _ in 0.."gamma".len() + 1 {
        key(&mut h, egui::Modifiers::SHIFT, egui::Key::ArrowRight);
    }
    h.run();
    h.get_by_role_and_label(Role::Button, "Merge cells").click();
    h.run();
    h.run();
    let merged = TABLE.replace("gamma & delta", "\\multicolumn{2}{l}{gamma delta}");
    assert_eq!(text(&mut h), merged);
    h.get_by_role_and_label(Role::Cell, "gamma delta").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Split cell").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), TABLE.replace("gamma & delta", "gamma delta &"));
}

#[test]
fn a_caption_set_from_the_toolbar_puts_a_bare_table_in_a_float() {
    let mut h = formatted(TABLE);
    h.get_by_role_and_label(Role::Cell, "alpha").click();
    h.run();
    h.get_by_role_and_label(Role::Button, "Caption…").click();
    h.run();
    h.get_by_label("The caption, in LaTeX").type_text("Results.");
    h.run();
    h.get_by_role_and_label(Role::Button, "Apply").click();
    h.run();
    h.run();
    let now = text(&mut h);
    let body = &TABLE["Before.\n\n".len()..TABLE.len() - "\n\nAfter.\n".len()];
    assert_eq!(now, format!("Before.\n\n\\begin{{table}}[htbp]\n\\centering\n{body}\n\\caption{{Results.}}\n\\end{{table}}\n\nAfter.\n"));
    assert!(h.query_by_label_contains("Results.").is_some(), "the caption is not shown");
}

/// A grey image of 40 × 20 pixels, made for these tests.
const PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAACgAAAAUCAIAAABwJOjsAAAAIklEQVR4nGOoGCDAMGrxqMWjFo9aPGrxqMWjFo9aPHIsBgCjDmU95l8pogAAAABJRU5ErkJggg==";

const SIZED: &str =
    "Before.\n\n\\begin{figure}[h]\n  \\centering\n  \\includegraphics[width=0.5\\linewidth]{plot}\n  \\caption{Old.}\n\\end{figure}\n";

fn with_image(text: &str) -> Harness<'static, App> {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    assert!(s.execute("file.write", &json!({"path": "main.tex", "text": text})).is_ok());
    assert!(s.execute("file.import", &json!({"path": "plot.png", "data": PNG})).is_ok());
    assert!(s.execute("file.open", &json!({"path": "main.tex"})).is_ok());
    let app = App::with_prefs(s, Prefs { auto_compile: false, ..Prefs::default() });
    let mut h = Harness::builder().with_size(egui::vec2(1200.0, 800.0)).build_ui_state(|ui, app: &mut App| app.frame(ui), app);
    h.run();
    h.get_by_label("Formatted").click();
    h.run();
    h
}

#[test]
fn the_image_is_resized_from_the_keyboard_and_by_its_handle() {
    let mut h = with_image(SIZED);
    let image = "Image plot, 50 % of the line";
    h.get_by_role_and_label(Role::Image, image).focus();
    h.run();
    key(&mut h, egui::Modifiers::NONE, egui::Key::Plus);
    h.run();
    assert_eq!(text(&mut h), SIZED.replace("0.5\\linewidth", "0.75\\linewidth"), "+ did not take the next width");
    // The handle: dragged towards the middle, it snaps to a quarter of the line.
    let knob = h.get_by_label("Image width").rect().center();
    let wide = h.get_by_role_and_label(Role::Image, "Image plot, 75 % of the line").rect();
    // The image is centred in its row: a third of the way from the middle to its edge.
    let to = egui::pos2(wide.center().x + (knob.x - wide.center().x) * 0.35, knob.y);
    let button = |pos, pressed| egui::Event::PointerButton { pos, button: egui::PointerButton::Primary, pressed, modifiers: egui::Modifiers::NONE };
    h.event(egui::Event::PointerMoved(knob));
    h.run();
    h.event(button(knob, true));
    h.run();
    for i in 1..=8 {
        h.event(egui::Event::PointerMoved(knob + (to - knob) * (i as f32 / 8.0)));
        h.run();
    }
    h.event(button(to, false));
    h.run();
    h.run();
    assert_eq!(text(&mut h), SIZED.replace("0.5\\linewidth", "0.25\\linewidth"), "the handle did not snap to a quarter");
}

#[test]
fn the_images_menu_sets_the_placement_from_the_keyboard() {
    let mut h = with_image(SIZED);
    h.get_by_role_and_label(Role::Image, "Image plot, 50 % of the line").focus();
    h.run();
    key(&mut h, egui::Modifiers::SHIFT, egui::Key::F10);
    h.run();
    h.get_by_role_and_label(Role::MenuItem, "At the top of a page").click();
    h.run();
    h.run();
    assert_eq!(text(&mut h), SIZED.replace("[h]", "[t]"));
}

#[test]
fn add_caption_puts_the_caret_in_a_new_caption() {
    let bare = "Before.\n\n\\begin{figure}[h]\n  \\centering\n  \\includegraphics{plot}\n\\end{figure}\n";
    let mut h = with_image(bare);
    h.get_by_role_and_label(Role::Button, "Add Caption").click();
    h.run();
    h.run();
    h.run();
    h.get_by_label("main.tex formatted").type_text("A plot.");
    h.run();
    assert_eq!(text(&mut h), bare.replace("{plot}\n", "{plot}\n  \\caption{A plot.}\n"));
}
