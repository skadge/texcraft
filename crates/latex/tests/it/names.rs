//! The names a document prints that the formatted view shows, in its class's and language's words.

use texcraft_latex::names::{self, Language};

#[test]
fn the_bibliography_is_headed_as_the_class_and_renewcommand_say() {
    assert_eq!(names::names("\\documentclass{article}\\begin{document}\\end{document}").bibliography, "References");
    assert_eq!(names::names("\\documentclass[11pt]{report}\\begin{document}\\end{document}").bibliography, "Bibliography");
    let renamed = "\\documentclass{article}\\renewcommand{\\refname}{Sources}\\renewcommand\\bibname{Unused}\\begin{document}\\end{document}";
    assert_eq!(names::names(renamed).bibliography, "Sources");
    let book = "\\documentclass{book}\\addto\\captionsenglish{\\renewcommand\\bibname{Readings}}\\begin{document}\\end{document}";
    assert_eq!(names::names(book).bibliography, "Readings");
}

#[test]
fn the_language_is_babels_main_one_or_polyglossias() {
    let lang = |preamble: &str| names::language(&format!("{preamble}\\begin{{document}}\\end{{document}}"));
    assert_eq!(lang("\\documentclass{article}\\usepackage[french]{babel}"), Language::French);
    assert_eq!(lang("\\documentclass{article}\\usepackage[english,ngerman]{babel}"), Language::German, "babel's last language is its main one");
    assert_eq!(lang("\\documentclass{article}\\usepackage[main=spanish,english]{babel}"), Language::Spanish);
    assert_eq!(lang("\\documentclass[dutch]{article}\\usepackage{babel}"), Language::Dutch, "the class's options count");
    assert_eq!(lang("\\documentclass{article}\\usepackage{polyglossia}\\setdefaultlanguage{italian}"), Language::Italian);
    assert_eq!(lang("\\documentclass{article}\\usepackage[finnish]{babel}"), Language::English, "a language the view does not know");
    assert_eq!(lang("\\documentclass{article}"), Language::English);
    let french = names::names("\\documentclass{article}\\usepackage[french]{babel}\\begin{document}\\end{document}");
    assert_ne!(french.contents, names::names("\\documentclass{article}").contents);
}

#[test]
fn today_is_written_as_each_language_writes_dates() {
    let english = names::today(Language::English, 2026, 3, 1);
    assert!(english.starts_with("March") && english.ends_with("2026"), "{english}");
    // The day before the month, in the other languages; the month named in the language.
    for language in
        [Language::British, Language::French, Language::German, Language::Spanish, Language::Italian, Language::Dutch, Language::Portuguese]
    {
        let date = names::today(language, 2026, 3, 1);
        assert!(date.starts_with('1'), "{language:?}: {date}");
        assert!(!date.contains("March") || language == Language::British, "{language:?}: {date}");
    }
}
