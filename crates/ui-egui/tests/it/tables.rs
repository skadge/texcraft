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

const FIGURE: &str = "Before.\n\n\\begin{figure}[h]\n  \\centering\n  \\includegraphics{plot}\n  \\caption{Old.}\n\\end{figure}\n";

fn text(harness: &mut Harness<'_, App>) -> String {
    let path = ProjectPath::new("main.tex").ok();
    path.and_then(|p| harness.state_mut().session().project().text(&p).ok().map(str::to_owned)).unwrap_or_default()
}

fn formatted(text: &str) -> Harness<'static, App> {
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
