//! Keystroke tables: text before (`|` marks the caret), the keys, and the text after. Written from
//! Vim's documentation of each command (`:help`), not from its code.

use texcraft_buffer::{Buffer, Selection};
use texcraft_vim::{Ctx, Effect, Key, Mode, Outcome, Vim};

/// `<Esc>`, `<CR>`, `<BS>`, `<Del>`, `<Tab>`, `<C-x>`; any other character is typed.
fn parse_keys(s: &str) -> Vec<Key> {
    let mut keys = Vec::new();
    let mut rest = s;
    while let Some(c) = rest.chars().next() {
        if c == '<'
            && let Some(end) = rest.find('>')
        {
            let name = &rest[1..end];
            let key = match name {
                "Esc" => Some(Key::Esc),
                "CR" => Some(Key::Enter),
                "BS" => Some(Key::Backspace),
                "Del" => Some(Key::Delete),
                "Tab" => Some(Key::Tab),
                _ => name.strip_prefix("C-").and_then(|c| c.chars().next()).map(Key::Ctrl),
            };
            if let Some(key) = key {
                keys.push(key);
                rest = &rest[end + 1..];
                continue;
            }
        }
        keys.push(Key::Char(c));
        rest = &rest[c.len_utf8()..];
    }
    keys
}

struct Run {
    text: String,
    mode: Mode,
    effects: Vec<Effect>,
    handled: Vec<bool>,
}

fn run(before: &str, keys: &str) -> Run {
    let caret = before.chars().position(|c| c == '|').unwrap_or(0);
    let mut buffer = Buffer::new(&before.replacen('|', "", 1));
    let mut selection = Selection::point(caret);
    let mut vim = Vim::new();
    let mut effects = Vec::new();
    let mut handled = Vec::new();
    for (i, key) in parse_keys(keys).into_iter().enumerate() {
        let mut ctx =
            Ctx { buffer: &mut buffer, selection: &mut selection, now: i as f64 * 0.1, view_top: 0, view_rows: 20, clipboard: None, indent: "  " };
        let Outcome { handled: h, effects: e } = vim.key(key, &mut ctx);
        handled.push(h);
        effects.extend(e);
    }
    let mut text = buffer.text();
    let at = buffer.char_to_byte(vim.cursor());
    text.insert(at, '|');
    Run { text, mode: vim.mode(), effects, handled }
}

#[test]
fn the_keystroke_table() {
    let table: &[(&str, &str, &str)] = &[
        // Operators with motions.
        ("|hello world", "dw", "|world"),
        ("hello |world", "dw", "hello| "),
        ("|one two three", "2dw", "|three"),
        ("|one two three", "d2w", "|three"),
        ("|one two", "cwONE<Esc>", "ON|E two"),
        ("fo|o bar", "cwx<Esc>", "fo|x bar"),
        ("|abc", "dtc", "|c"),
        ("ab|cd", "D", "a|b"),
        ("|a\nb\nc", "dj", "|c"),
        ("a\n|b\nc\nd", "dG", "|a"),
        ("|a\nb\n\nc", "d}", "|\nc"),
        ("|foo bar", "gUiw", "|FOO bar"),
        ("|Foo", "g~~", "|fOO"),
        ("  |foo", "ccbar<Esc>", "  ba|r"),
        // Lines.
        ("|line1\nline2\nline3", "dd", "|line2\nline3"),
        ("line1\n|line2\nline3", "ddp", "line1\nline3\n|line2"),
        ("|a\nb\nc", "yyjp", "a\nb\n|a\nc"),
        ("|one\ntwo", "\"ayyj\"ap", "one\ntwo\n|one"),
        ("|a\nb\nc", "2>>", "  |a\n  b\nc"),
        // Simple commands.
        ("|hello", "x", "|ello"),
        ("|hello", "3x", "|lo"),
        ("hel|lo", "X", "he|lo"),
        ("|ab", "xp", "b|a"),
        ("|abc", "~~~", "AB|C"),
        ("|abc", "rx", "|xbc"),
        ("|a\n  b", "J", "a| b"),
        ("x |9 y", "<C-a>", "x 1|0 y"),
        ("x |5 y", "3<C-x>", "x |2 y"),
        // Motions.
        ("|(a [b] c)", "%", "(a [b] c|)"),
        ("|a\nb\n\nc", "}", "a\nb\n|\nc"),
        ("|a\n\nb", "w", "a\n|\nb"),
        ("|foo.bar baz", "e", "fo|o.bar baz"),
        ("foo.bar |baz", "B", "|foo.bar baz"),
        ("foo bar|", "ge", "fo|o bar"),
        ("|a,b,c", "f,;", "a,b|,c"),
        ("|a,b,c", "t,;", "a,|b,c"),
        ("|abc\nde", "$j", "abc\nd|e"),
        ("|a\nb\nc", "2G", "a\n|b\nc"),
        ("|a\nb\nc", "majj'a", "|a\nb\nc"),
        // Text objects.
        ("a|bc def", "ciwX<Esc>", "|X def"),
        ("one t|wo three", "daw", "one |three"),
        ("|foo(bar, baz)", "f(di(", "foo(|)"),
        ("x = {a, {|b}, c}", "di{", "x = {a, {|}, c}"),
        ("x = {a, {|b}, c}", "2di{", "x = {|}"),
        ("say \"hel|lo there\" now", "ci\"bye<Esc>", "say \"by|e\" now"),
        ("|a\nb\n\nc", "dip", "|\nc"),
        // LaTeX's text objects.
        ("\\begin{itemize}\n  \\item |a\n\\end{itemize}", "die", "\\begin{itemize}\n|\\end{itemize}"),
        ("x\n\\begin{a}\n|y\n\\end{a}", "dae", "x\n|"),
        ("where $|x+1$ ok", "ci$y<Esc>", "where $|y$ ok"),
        ("\\textbf{b|old} x", "dac", "| x"),
        ("\\frac{|a}{b}", "cicz<Esc>", "\\frac{|z}{b}"),
        // Insert mode.
        ("|abc", "ix<Esc>", "|xabc"),
        ("|abc", "3ix<Esc>", "xx|xabc"),
        ("|abc", "Ax<Esc>", "abc|x"),
        ("|abc", "ox<Esc>", "abc\n|x"),
        ("  |abc", "ox<Esc>", "  abc\n  |x"),
        ("|abc", "Ox<Esc>", "|x\nabc"),
        ("|foo bar", "A<C-w><Esc>", "foo| "),
        ("|abc", "Rxy<Esc>", "x|yc"),
        ("|abc", "Rxy<BS><Esc>", "|xbc"),
        // Undo, redo and repeat.
        ("|abc", "xu", "|abc"),
        ("|abc", "xxu", "|bc"),
        ("|abc", "xu<C-r>", "|bc"),
        ("|abc", "ixyz<Esc>u", "|abc"),
        ("|a b c d", "dw..", "|d"),
        ("|x\nx\nx", "Ay<Esc>j.", "xy\nx|y\nx"),
        ("|abcdef", "x3.", "|ef"),
        // Visual modes.
        ("|hello world", "vlld", "|lo world"),
        ("|a\nb\nc", "Vjd", "|c"),
        ("|one\ntwo", "Vj>", "  |one\n  two"),
        ("|foo bar", "yiwwviwp", "foo |foo"),
        ("|abc", "vlU", "|ABc"),
        // Search.
        ("|foo bar foo", "/foo<CR>", "foo bar |foo"),
        ("|foo bar foo", "/foo<CR>n", "|foo bar foo"),
        ("|foo bar foo", "*", "foo bar |foo"),
        ("|a(b) a(c)", "/a\\(.\\)<CR>", "a(b) |a(c)"),
        // The command line.
        ("|a a\na", ":s/a/b/<CR>", "|b a\na"),
        ("|a a\na", ":%s/a/b/g<CR>", "b b\n|b"),
        ("|x1 x2", ":s/x\\(\\d\\)/<\\1>/g<CR>", "|<1> <2>"),
        ("|a\nb\nc", ":2<CR>", "a\n|b\nc"),
        ("|a\nb\nc", ":2,3d<CR>", "|a"),
        ("|a\nb", ":%norm Ax<CR>", "ax\nb|x"),
        // Macros.
        ("|1\n2\n3", "qaA.<Esc>jq2@a", "1.\n2.\n3|."),
    ];
    let mut failures = Vec::new();
    for (before, keys, after) in table {
        let got = run(before, keys).text;
        if got != *after {
            failures.push(format!("{before:?} + {keys:?}: expected {after:?}, got {got:?}"));
        }
    }
    assert!(failures.is_empty(), "{} of {} failed:\n{}", failures.len(), table.len(), failures.join("\n"));
}

#[test]
fn modes_change_with_their_keys() {
    assert_eq!(run("|abc", "i").mode, Mode::Insert);
    assert_eq!(run("|abc", "i<Esc>").mode, Mode::Normal);
    assert_eq!(run("|abc", "v").mode, Mode::Visual);
    assert_eq!(run("|abc", "V").mode, Mode::VisualLine);
    assert_eq!(run("|abc", "vv").mode, Mode::Normal);
    assert_eq!(run("|abc", "R").mode, Mode::Replace);
    assert_eq!(run("|abc", "i<C-o>").mode, Mode::Normal);
    assert_eq!(run("|abc", "i<C-o>l").mode, Mode::Insert, "Ctrl-O runs one command and goes back to Insert");
}

#[test]
fn window_shortcuts_are_left_to_the_window_and_unsupported_commands_say_so() {
    let r = run("|abc", "<C-s>");
    assert_eq!(r.handled, [false], "Ctrl+S was kept from the window's Save");
    let r = run("|abc", "<C-v>");
    assert!(r.effects.iter().any(|e| matches!(e, Effect::Error(_))), "Visual block mode was not reported as unsupported");
    let r = run("|abc", "==");
    assert!(r.effects.iter().any(|e| matches!(e, Effect::Error(_))));
    assert_eq!(r.text, "|abc");
}

#[test]
fn file_commands_become_effects() {
    assert_eq!(run("|a", ":w<CR>").effects, [Effect::Save]);
    assert_eq!(run("|a", ":wq<CR>").effects, [Effect::Save, Effect::Close { force: false }]);
    assert_eq!(run("|a", ":q!<CR>").effects, [Effect::Close { force: true }]);
    assert_eq!(run("|a", ":e intro.tex<CR>").effects, [Effect::Open("intro.tex".to_owned())]);
    assert_eq!(run("|abc", "\"+yiw").effects, [Effect::Copy("abc".to_owned())]);
}
