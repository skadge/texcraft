//! The tolerant syntax tree: structure of typical documents, half-typed input, and a hostile-input
//! sweep that checks the spans always tile the text.

use texcraft_latex::tree::{Kind, Node, parse, tiles};

const MAIN: &str = include_str!("../../../../examples/article/main.tex");
const INTRO: &str = include_str!("../../../../examples/article/sections/intro.tex");
const FIGURE: &str = "\\begin{figure}[ht]\n  \\centering\n  \\includegraphics[width=0.5\\linewidth]{img/bob.png}\n  \\caption{A bob, \\emph{seen} from above.}\n  \\label{fig:bob}\n\\end{figure}\n";

/// Every node of the tree, depth first, argument bodies included.
fn all(nodes: &[Node]) -> Vec<&Node> {
    let mut out = Vec::new();
    for n in nodes {
        out.push(n);
        match &n.kind {
            Kind::Group { body, .. } => out.extend(all(body)),
            Kind::Command { args, .. } => args.iter().for_each(|a| out.extend(all(&a.body))),
            Kind::Environment { args, body, .. } => {
                args.iter().for_each(|a| out.extend(all(&a.body)));
                out.extend(all(body));
            }
            _ => {}
        }
    }
    out
}

fn command<'a>(nodes: &'a [Node], name: &str) -> Option<&'a Node> {
    all(nodes).into_iter().find(|n| matches!(&n.kind, Kind::Command { name: m, .. } if m == name))
}

fn environment<'a>(nodes: &'a [Node], name: &str) -> Option<&'a Node> {
    all(nodes).into_iter().find(|n| matches!(&n.kind, Kind::Environment { name: m, .. } if m == name))
}

/// The text of the `i`th argument of the first `name` command.
fn arg<'a>(text: &'a str, nodes: &[Node], name: &str, i: usize) -> Option<&'a str> {
    let Kind::Command { args, .. } = &command(nodes, name)?.kind else { return None };
    text.get(args.get(i)?.inner.clone())
}

#[test]
fn a_document_has_its_commands_environments_and_formulas() {
    let tree = parse(MAIN);
    assert!(tiles(&tree, 0..MAIN.len()));
    let Some(Node { kind: Kind::Environment { body, end, .. }, .. }) = environment(&tree, "document") else { panic!("no document environment") };
    assert!(end.is_some());
    // The sections are inside the document, with their titles as arguments.
    assert_eq!(arg(MAIN, body, "section", 0), Some("Introduction"));
    assert_eq!(arg(MAIN, &tree, "title", 0), Some("Notes on a Small Pendulum"));
    assert!(command(&tree, "maketitle").is_some());
    let maths: Vec<_> = all(&tree)
        .into_iter()
        .filter_map(|n| if let Kind::Math { display, env, closed, .. } = &n.kind { Some((*display, env.clone(), *closed)) } else { None })
        .collect();
    assert!(maths.contains(&(false, None, true)), "inline math: {maths:?}");
    assert!(maths.contains(&(true, Some("align".into()), true)));
    assert!(maths.contains(&(true, Some("equation".into()), true)));
    // `\item` sits in the list's body; the table keeps its tabular and caption.
    let Some(Node { kind: Kind::Environment { body, .. }, .. }) = environment(&tree, "itemize") else { panic!("no itemize") };
    assert_eq!(body.iter().filter(|n| matches!(&n.kind, Kind::Command { name, .. } if name == "item")).count(), 3);
    let Some(Node { kind: Kind::Environment { args, body, .. }, .. }) = environment(&tree, "table") else { panic!("no table") };
    assert_eq!(args.first().map(|a| &MAIN[a.inner.clone()]), Some("h"));
    assert!(environment(body, "tabular").is_some() && command(body, "caption").is_some());
    assert!(all(&tree).iter().any(|n| n.kind == Kind::ParBreak));
    assert!(!all(&tree).iter().any(|n| n.kind == Kind::Error));
}

#[test]
fn figures_nested_emphasis_verbatim_and_comments() {
    let tree = parse(FIGURE);
    assert!(tiles(&tree, 0..FIGURE.len()));
    assert_eq!(arg(FIGURE, &tree, "includegraphics", 1), Some("img/bob.png"));
    assert_eq!(arg(FIGURE, &tree, "label", 0), Some("fig:bob"));
    assert_eq!(arg(FIGURE, &tree, "emph", 0), Some("seen"));
    let Some(Node { kind: Kind::Command { args, .. }, .. }) = command(&tree, "caption") else { panic!("no caption") };
    assert!(args.first().and_then(|a| command(&a.body, "emph")).is_some(), "emph inside the caption");

    let text = "\\textbf{bold \\textit{both}} % a note\n\\verb|\\x{| and\n\\begin{verbatim}\n\\section{no}\n\\end{verbatim}\n";
    let tree = parse(text);
    assert!(tiles(&tree, 0..text.len()));
    let Some(Node { kind: Kind::Command { args, .. }, .. }) = command(&tree, "textbf") else { panic!("no textbf") };
    assert!(args.first().and_then(|a| command(&a.body, "textit")).is_some());
    assert!(tree.iter().any(|n| n.kind == Kind::Comment && text[n.range.clone()] == *"% a note"));
    let verbatim: Vec<_> =
        tree.iter().filter_map(|n| if let Kind::Verbatim { body, .. } = &n.kind { Some(&text[body.clone()]) } else { None }).collect();
    assert_eq!(verbatim, ["\\x{", "\n\\section{no}\n"]);
    assert!(command(&tree, "section").is_none(), "verbatim text is not parsed");
}

#[test]
fn half_typed_input_is_marked_unclosed() {
    let text = "\\section{Intro\n\\begin{itemize}\\item one \\emph{a $x^2 and";
    let tree = parse(text);
    assert!(tiles(&tree, 0..text.len()));
    let Some(Node { kind: Kind::Command { args, .. }, .. }) = command(&tree, "section") else { panic!("no section") };
    assert!(!args.iter().any(|a| a.closed));
    assert!(all(&tree).iter().any(|n| matches!(n.kind, Kind::Math { closed: false, .. })));
    assert!(matches!(environment(&tree, "itemize").map(|n| &n.kind), Some(Kind::Environment { end: None, .. })));

    // A formula left open in a group ends at the group's `}`, not at the end of the paragraph.
    let text = "\\textbf{see $x} after";
    let tree = parse(text);
    assert!(matches!(&tree[0].kind, Kind::Command { args, .. } if args[0].closed));
    assert_eq!(&text[tree[1].range.clone()], " after");
}

#[test]
fn stray_closers_are_errors_over_their_bytes() {
    let text = "a } b \\end{itemize} c";
    let tree = parse(text);
    let errors: Vec<_> = tree.iter().filter(|n| n.kind == Kind::Error).map(|n| &text[n.range.clone()]).collect();
    assert_eq!(errors, ["}", "\\end{itemize}"]);

    // `}` closes the group even across an optional argument left open inside it.
    let text = "{\\item[label} after";
    let tree = parse(text);
    assert!(tiles(&tree, 0..text.len()));
    assert!(matches!(&tree[0].kind, Kind::Group { closed: true, .. }));
    // An outer `\end` closes an inner environment left open.
    let text = "\\begin{a}\\begin{b} x\\end{a} y";
    let tree = parse(text);
    assert!(matches!(&tree[0].kind, Kind::Environment { end: Some(_), .. }));
    assert!(matches!(environment(&tree, "b").map(|n| &n.kind), Some(Kind::Environment { end: None, .. })));
}

/// Characters whose insertion or deletion changes the structure.
const STRUCTURAL: &[char] = &['{', '}', '[', ']', '\\', '$', '%', '\n'];

fn check(text: &str) {
    let tree = parse(text);
    assert!(tiles(&tree, 0..text.len()), "does not tile: {text:?}");
}

#[test]
fn hostile_input_never_breaks_the_tiling() {
    let samples = [MAIN, INTRO, FIGURE, "é$\\(ü\\[ö{\\verb\u{2603}x\u{2603}\\begin{ä}%ß\n\n}"];
    for sample in samples {
        let boundaries: Vec<usize> = (0..=sample.len()).filter(|i| sample.is_char_boundary(*i)).collect();
        for &i in &boundaries {
            check(&sample[..i]);
            check(&sample[i..]);
            for c in STRUCTURAL {
                let mut s = sample.to_owned();
                s.insert(i, *c);
                check(&s);
            }
        }
        for w in boundaries.windows(2) {
            if sample[w[0]..w[1]].contains(STRUCTURAL) {
                check(&format!("{}{}", &sample[..w[0]], &sample[w[1]..]));
            }
        }
    }
    for deep in ["{", "\\textbf{", "\\item[", "\\begin{a}", "$\\(", "\\section[{"] {
        check(&deep.repeat(10_000));
        check(&format!("{}x{}", deep.repeat(10_000), "}".repeat(10_000)));
    }
}
