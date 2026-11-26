//! The figure dialog's edits: each part changes alone, and missing parts are added in place.

use texcraft_latex::edit::apply;
use texcraft_latex::figure::{self, FigureChanges};

const FIG: &str = "Text.\n\n\\begin{figure}[h]\n  \\centering\n  \\includegraphics[width=0.5\\linewidth, angle=90]{plot}\n  \\caption{Old $x$.}\n  \\label{fig:plot}\n\\end{figure}\n";

fn edit(text: &str, c: &FigureChanges) -> Result<String, String> {
    let at = text.find("\\begin{figure}").ok_or("no figure")?;
    let fig = figure::find(text, at + 3).ok_or("figure not found")?;
    let changes = fig.edit(text, c);
    assert!(changes.windows(2).all(|w| w[0].range.end <= w[1].range.start), "overlapping: {changes:?}");
    Ok(apply(text, &changes))
}

#[test]
fn each_part_changes_alone() -> Result<(), String> {
    let caption = edit(FIG, &FigureChanges { caption: Some("New.".into()), ..Default::default() })?;
    assert_eq!(caption, FIG.replace("Old $x$.", "New."));
    let width = edit(FIG, &FigureChanges { width: Some(0.8), ..Default::default() })?;
    assert_eq!(width, FIG.replace("width=0.5\\linewidth", "width=0.8\\linewidth"));
    let place = edit(FIG, &FigureChanges { placement: Some("tb".into()), label: Some("fig:new".into()), ..Default::default() })?;
    assert_eq!(place, FIG.replace("[h]", "[tb]").replace("fig:plot", "fig:new"));
    let fig = figure::find(FIG, FIG.find("plot").unwrap_or_default()).ok_or("no figure")?;
    let options = fig.image.and_then(|g| g.options).map(|o| FIG[o].to_owned()).unwrap_or_default();
    assert_eq!(figure::width_fraction(&options), Some(0.5));
    Ok(())
}

#[test]
fn missing_parts_are_added_on_lines_of_their_own() -> Result<(), String> {
    let bare = "\\begin{figure}\n  \\includegraphics{a}\n\\end{figure}";
    let out = edit(
        bare,
        &FigureChanges {
            width: Some(1.0),
            placement: Some("htbp".into()),
            caption: Some("C".into()),
            label: Some("fig:a".into()),
            ..Default::default()
        },
    )?;
    assert_eq!(out, "\\begin{figure}[htbp]\n  \\includegraphics[width=\\linewidth]{a}\n  \\caption{C}\n  \\label{fig:a}\n\\end{figure}");
    // A new figure reads back as the parts it was made with.
    let made = figure::new_figure(&FigureChanges {
        image: Some("img/x".into()),
        caption: Some("X".into()),
        label: Some("fig:x".into()),
        ..Default::default()
    });
    let fig = figure::find(&made, 2).ok_or("the new figure does not read")?;
    assert_eq!(fig.caption.map(|c| made[c.0].to_owned()).as_deref(), Some("X"));
    assert_eq!(fig.image.map(|g| made[g.path].to_owned()).as_deref(), Some("img/x"));
    Ok(())
}

#[test]
fn a_table_float_gets_its_caption_and_label_like_a_figure() -> Result<(), String> {
    let text = "\\begin{table}[h]\n  \\centering\n  \\begin{tabular}{l}\n  a\n  \\end{tabular}\n\\end{table}\n";
    let fig = figure::find_table_float(text, text.find("tabular").ok_or("no tabular")?).ok_or("no float")?;
    let out = apply(text, &fig.edit(text, &FigureChanges { caption: Some("Results.".into()), label: Some("tab:r".into()), ..Default::default() }));
    assert_eq!(
        out,
        "\\begin{table}[h]\n  \\caption{Results.}\n  \\label{tab:r}\n  \\centering\n  \\begin{tabular}{l}\n  a\n  \\end{tabular}\n\\end{table}\n"
    );
    assert!(figure::find_table_float("\\begin{figure}\\end{figure}", 3).is_none());
    Ok(())
}

#[test]
fn an_empty_caption_is_added_as_a_place_to_type_one_after_the_placement() -> Result<(), String> {
    let text = "\\begin{figure}[h]\n\\end{figure}";
    let out = edit(text, &FigureChanges { caption: Some(String::new()), ..Default::default() })?;
    assert_eq!(out, "\\begin{figure}[h]\n  \\caption{}\n\\end{figure}");
    Ok(())
}
