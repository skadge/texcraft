use texcraft_project::ProjectPath;

#[test]
fn a_path_that_could_leave_the_project_is_refused() {
    for hostile in ["../x.tex", "a/../../x", "/etc/passwd", "C:/x.tex", "a\\..\\x", "a//b", "a/./b", "", "a\0b"] {
        assert!(ProjectPath::new(hostile).is_err(), "{hostile:?} was accepted");
    }
}

#[test]
fn a_leading_dot_slash_names_the_same_file() {
    assert_eq!(ProjectPath::new("./chapters/intro.tex").ok(), ProjectPath::new("chapters/intro.tex").ok());
}
