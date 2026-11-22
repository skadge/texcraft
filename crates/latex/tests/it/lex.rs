use texcraft_latex::{Kind, lex};

fn kind_at(text: &str, needle: &str) -> Option<Kind> {
    let at = text.find(needle)?;
    lex(text).into_iter().find(|t| t.range.contains(&at)).map(|t| t.kind)
}

#[test]
fn tokens_cover_the_text_without_gaps_or_overlaps() {
    let text = "\\section{Intro} % note\nSee $a^2$ and \\[ x \\] é\\\\ \\begin{align*} y &= 1 \\end{align*}\\";
    let mut at = 0;
    for t in lex(text) {
        assert_eq!(t.range.start, at);
        at = t.range.end;
    }
    assert_eq!(at, text.len());
}

#[test]
fn math_ends_where_its_delimiter_closes() {
    let text = "one $two$ three \\[ four \\] five \\begin{equation} six \\end{equation} seven";
    assert_eq!(kind_at(text, "two"), Some(Kind::Math));
    assert_eq!(kind_at(text, "four"), Some(Kind::Math));
    assert_eq!(kind_at(text, "six"), Some(Kind::Math));
    for outside in ["three", "five", "seven"] {
        assert_eq!(kind_at(text, outside), Some(Kind::Text), "{outside}");
    }
}

#[test]
fn an_escaped_percent_does_not_start_a_comment() {
    assert_eq!(kind_at("50\\% off", "off"), Some(Kind::Text));
}
