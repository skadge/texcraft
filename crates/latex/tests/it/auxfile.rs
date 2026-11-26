//! What cross-references and citations show, from an `.aux`.

use texcraft_latex::auxfile::{Aux, CiteStyle, Missing, parse};

#[test]
fn natbib_citations_are_set_as_its_styles_set_them() {
    // As retex writes them with natbib and plainnat; the forms are those of its PDF.
    let aux = parse("\\bibcite{knuth}{{1}{1984}{{Knuth}}{{}}}\n\\bibcite{lamport}{{2}{1994}{{Lamport}}{{}}}\n");
    let cite = |style, command| Aux::citation(Some(&aux), style, command, "knuth, lamport");
    assert_eq!(cite(CiteStyle::NatbibAuthorYear, "citep").as_deref(), Ok("[Knuth, 1984, Lamport, 1994]"));
    assert_eq!(cite(CiteStyle::NatbibAuthorYear, "citet").as_deref(), Ok("Knuth [1984], Lamport [1994]"));
    assert_eq!(cite(CiteStyle::NatbibNumbers, "citep").as_deref(), Ok("[1, 2]"));
    assert_eq!(cite(CiteStyle::NatbibNumbers, "citet").as_deref(), Ok("Knuth [1], Lamport [2]"));
    assert_eq!(Aux::citation(Some(&aux), CiteStyle::Plain, "cite", "knuth, nobody"), Err(Missing::Unknown("nobody".into())));
    assert_eq!(CiteStyle::of("\\usepackage[numbers,sort]{natbib}"), CiteStyle::NatbibNumbers);
}

#[test]
fn cleveref_names_a_run_of_one_kind_once_in_the_plural() {
    let aux = parse(
        "\\newlabel{a}{{1}{1}{}{equation.1}{}}\n\\newlabel{a@cref}{{[equation][1][]1}{[1][1][]1}{}{}{}}\n\\newlabel{b}{{2}{1}{}{equation.2}{}}\n\\newlabel{f}{{3}{2}{Cap}{figure.3}{}}\n",
    );
    assert_eq!(Aux::reference(Some(&aux), "cref", "a,b,f").as_deref(), Ok("eqs.\u{a0}(1) and (2) and fig.\u{a0}3"));
    assert_eq!(Aux::reference(Some(&aux), "Cref", "f").as_deref(), Ok("Figure\u{a0}3"));
    assert_eq!(Aux::reference(Some(&aux), "pageref", "f").as_deref(), Ok("2"));
    assert_eq!(Aux::reference(Some(&aux), "nameref", "f").as_deref(), Ok("Cap"));
    assert_eq!(Aux::reference(None, "ref", "a"), Err(Missing::NotBuilt));
}
