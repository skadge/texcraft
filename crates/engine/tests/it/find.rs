use serde_json::json;
use texcraft_compile::Unavailable;
use texcraft_engine::Session;
use texcraft_engine::find::Query;

#[test]
fn whole_words_case_and_regular_expressions_narrow_the_matches() {
    let text = "Cat cat category cat.";
    let q = |pattern: &str, regex, case, word| Query { pattern: pattern.to_owned(), regex, case, word };
    assert_eq!(q("cat", false, false, false).matches(text).unwrap().len(), 4);
    assert_eq!(q("cat", false, true, false).matches(text).unwrap().len(), 3);
    assert_eq!(q("cat", false, false, true).matches(text).unwrap().len(), 3);
    assert_eq!(q("c.t", false, false, false).matches(text).unwrap().len(), 0, "a plain pattern's dot is a dot");
    assert_eq!(q("c.t", true, false, false).matches(text).unwrap().len(), 4);
    assert!(q("(", true, false, false).matches(text).is_err());
}

#[test]
fn replacing_counts_and_expands_groups_only_for_regular_expressions() {
    let q = Query { pattern: r"x(\d)".to_owned(), regex: true, ..Query::default() };
    assert_eq!(q.replace_all("x1 x2", "<$1>", None).unwrap(), ("<1> <2>".to_owned(), 2));
    assert_eq!(q.replace_all("x1 x2", "<$1>", Some(2..5)).unwrap(), ("x1 <2>".to_owned(), 1), "outside the range was replaced");
    let plain = Query { pattern: "a".to_owned(), ..Query::default() };
    assert_eq!(plain.replace_all("a", "$0", None).unwrap().0, "$0", "a plain replacement was expanded");
}

#[test]
fn file_find_and_replace_work_without_the_window() {
    let mut s = Session::with_backend(Box::new(Unavailable("tests".into())));
    s.execute("file.write", &json!({"path": "main.tex", "text": "one\ntwo one"})).unwrap();
    let found = s.execute("file.find", &json!({"path": "main.tex", "pattern": "one"})).unwrap();
    assert_eq!(found["count"], 2);
    assert_eq!((found["matches"][1]["line"].as_u64(), found["matches"][1]["column"].as_u64()), (Some(2), Some(5)));
    let r = s.execute("file.replace", &json!({"path": "main.tex", "pattern": "one", "replacement": "1"})).unwrap();
    assert_eq!(r["replaced"], 2);
    let text = s.project().text(&texcraft_engine::project::ProjectPath::new("main.tex").unwrap()).unwrap().to_owned();
    assert_eq!(text, "1\ntwo 1");
}
