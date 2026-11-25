//! Normal and Visual mode commands, read from the keys typed so far:
//! `["x][count]command`, where a command is a motion, an operator with a motion, a text object
//! or its own key doubled (`dd`), or one of the other commands.

use crate::Key;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Motion {
    Left,
    Right,
    /// Backspace and Space: left and right, across lines.
    BackWrap,
    ForwardWrap,
    Down,
    Up,
    /// Enter, `+` and `-`: lines down or up, to the first non-blank.
    DownFirst,
    UpFirst,
    WordStart(bool),
    WordEnd(bool),
    WordBack(bool),
    WordEndBack(bool),
    LineStart,
    FirstNonBlank,
    LineEnd,
    LastNonBlank,
    Column,
    GotoLast,
    GotoFirst,
    Find {
        target: char,
        forward: bool,
        till: bool,
    },
    RepeatFind {
        reverse: bool,
    },
    Percent,
    ParagraphForward,
    ParagraphBack,
    ViewTop,
    ViewMiddle,
    ViewBottom,
    Mark {
        name: char,
        exact: bool,
    },
    SearchNext {
        reverse: bool,
    },
    SearchWord {
        forward: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Op {
    Delete,
    Change,
    Yank,
    Indent,
    Outdent,
    Lower,
    Upper,
    Toggle,
    Reindent,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Target {
    Motion(Motion),
    Object {
        around: bool,
        kind: char,
    },
    /// The operator's key doubled: whole lines.
    Lines,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Simple {
    DeleteChar,
    DeleteCharBack,
    Substitute,
    SubstituteLine,
    DeleteToEnd,
    ChangeToEnd,
    YankLine,
    Replace(char),
    Join {
        spaces: bool,
    },
    Put {
        before: bool,
    },
    Undo,
    Redo,
    Repeat,
    ToggleCase,
    Insert,
    Append,
    InsertFirst,
    InsertColumnZero,
    AppendEnd,
    OpenBelow,
    OpenAbove,
    ReplaceMode,
    Visual,
    VisualLine,
    VisualBlock,
    Reselect,
    SearchForward,
    SearchBackward,
    CommandLine,
    SetMark(char),
    Record(Option<char>),
    Play(char),
    SaveQuit,
    QuitForce,
    ScrollCenter,
    ScrollTop,
    ScrollBottom,
    /// `zc`, `zo`, `za`, `zM`, `zR`.
    Fold(crate::Fold),
    HalfDown,
    HalfUp,
    PageDown,
    PageUp,
    LineDown,
    LineUp,
    JumpOlder,
    JumpNewer,
    Increment(bool),
    Cancel,
    // Visual mode only.
    SwapEnds,
    VisualLines(Op),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Cmd {
    Move(Motion),
    Operate(Op, Target),
    Simple(Simple),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Parsed {
    pub register: Option<char>,
    /// 0 when none was typed.
    pub count: usize,
    pub cmd: Cmd,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Parse<T> {
    /// More keys are needed.
    More,
    /// The keys are not a command.
    Invalid,
    Done(T),
}

fn ch(keys: &[Key], i: usize) -> Option<Option<char>> {
    keys.get(i).map(|k| if let Key::Char(c) = k { Some(*c) } else { None })
}

fn count_at(keys: &[Key], i: &mut usize) -> usize {
    let mut n = 0usize;
    if let Some(Some(c @ '1'..='9')) = ch(keys, *i) {
        n = c as usize - '0' as usize;
        *i += 1;
        while let Some(Some(c @ '0'..='9')) = ch(keys, *i) {
            n = n.saturating_mul(10).saturating_add(c as usize - '0' as usize);
            *i += 1;
        }
    }
    n
}

/// A motion at `keys[i..]`, and how many keys it took.
fn motion(keys: &[Key], i: usize) -> Parse<(Motion, usize)> {
    let Some(key) = keys.get(i) else { return Parse::More };
    let one = |m| Parse::Done((m, 1));
    let with_char = |make: &dyn Fn(char) -> Motion| match ch(keys, i + 1) {
        None => Parse::More,
        Some(Some(c)) => Parse::Done((make(c), 2)),
        Some(None) => Parse::Invalid,
    };
    match key {
        Key::Left => one(Motion::Left),
        Key::Right => one(Motion::Right),
        Key::CtrlRight => one(Motion::WordStart(true)),
        Key::CtrlLeft => one(Motion::WordBack(true)),
        Key::ShiftRight => one(Motion::WordStart(false)),
        Key::ShiftLeft => one(Motion::WordBack(false)),
        Key::Up => one(Motion::Up),
        Key::Down => one(Motion::Down),
        Key::Home => one(Motion::LineStart),
        Key::End => one(Motion::LineEnd),
        Key::Backspace => one(Motion::BackWrap),
        Key::Enter => one(Motion::DownFirst),
        Key::Ctrl('n' | 'j') => one(Motion::Down),
        Key::Ctrl('p') => one(Motion::Up),
        Key::Ctrl('h') => one(Motion::BackWrap),
        Key::Char(c) => match c {
            'h' => one(Motion::Left),
            'l' => one(Motion::Right),
            ' ' => one(Motion::ForwardWrap),
            'j' => one(Motion::Down),
            'k' => one(Motion::Up),
            '+' => one(Motion::DownFirst),
            '-' => one(Motion::UpFirst),
            'w' => one(Motion::WordStart(false)),
            'W' => one(Motion::WordStart(true)),
            'e' => one(Motion::WordEnd(false)),
            'E' => one(Motion::WordEnd(true)),
            'b' => one(Motion::WordBack(false)),
            'B' => one(Motion::WordBack(true)),
            '0' => one(Motion::LineStart),
            '^' => one(Motion::FirstNonBlank),
            '$' => one(Motion::LineEnd),
            '|' => one(Motion::Column),
            'G' => one(Motion::GotoLast),
            ';' => one(Motion::RepeatFind { reverse: false }),
            ',' => one(Motion::RepeatFind { reverse: true }),
            '%' => one(Motion::Percent),
            '}' => one(Motion::ParagraphForward),
            '{' => one(Motion::ParagraphBack),
            'H' => one(Motion::ViewTop),
            'M' => one(Motion::ViewMiddle),
            'L' => one(Motion::ViewBottom),
            'n' => one(Motion::SearchNext { reverse: false }),
            'N' => one(Motion::SearchNext { reverse: true }),
            '*' => one(Motion::SearchWord { forward: true }),
            '#' => one(Motion::SearchWord { forward: false }),
            'f' => with_char(&|c| Motion::Find { target: c, forward: true, till: false }),
            'F' => with_char(&|c| Motion::Find { target: c, forward: false, till: false }),
            't' => with_char(&|c| Motion::Find { target: c, forward: true, till: true }),
            'T' => with_char(&|c| Motion::Find { target: c, forward: false, till: true }),
            '`' => with_char(&|c| Motion::Mark { name: c, exact: true }),
            '\'' => with_char(&|c| Motion::Mark { name: c, exact: false }),
            'g' => match ch(keys, i + 1) {
                None => Parse::More,
                Some(Some('g')) => Parse::Done((Motion::GotoFirst, 2)),
                Some(Some('e')) => Parse::Done((Motion::WordEndBack(false), 2)),
                Some(Some('E')) => Parse::Done((Motion::WordEndBack(true), 2)),
                Some(Some('_')) => Parse::Done((Motion::LastNonBlank, 2)),
                Some(Some('j')) => Parse::Done((Motion::Down, 2)),
                Some(Some('k')) => Parse::Done((Motion::Up, 2)),
                Some(Some('0')) => Parse::Done((Motion::LineStart, 2)),
                Some(Some('$')) => Parse::Done((Motion::LineEnd, 2)),
                _ => Parse::Invalid,
            },
            _ => Parse::Invalid,
        },
        _ => Parse::Invalid,
    }
}

fn operator(keys: &[Key], i: usize) -> Parse<(Op, usize)> {
    match ch(keys, i) {
        Some(Some('d')) => Parse::Done((Op::Delete, 1)),
        Some(Some('c')) => Parse::Done((Op::Change, 1)),
        Some(Some('y')) => Parse::Done((Op::Yank, 1)),
        Some(Some('>')) => Parse::Done((Op::Indent, 1)),
        Some(Some('<')) => Parse::Done((Op::Outdent, 1)),
        Some(Some('=')) => Parse::Done((Op::Reindent, 1)),
        Some(Some('g')) => match ch(keys, i + 1) {
            None => Parse::More,
            Some(Some('u')) => Parse::Done((Op::Lower, 2)),
            Some(Some('U')) => Parse::Done((Op::Upper, 2)),
            Some(Some('~')) => Parse::Done((Op::Toggle, 2)),
            _ => Parse::Invalid,
        },
        _ => Parse::Invalid,
    }
}

/// The keys of an operator, to recognise it doubled (`dd`, `gUU`, `gUgU`).
fn doubled(op: Op, keys: &[Key], i: usize) -> Parse<usize> {
    let want: &[&[char]] = match op {
        Op::Delete => &[&['d']],
        Op::Change => &[&['c']],
        Op::Yank => &[&['y']],
        Op::Indent => &[&['>']],
        Op::Outdent => &[&['<']],
        Op::Reindent => &[&['=']],
        Op::Lower => &[&['u'], &['g', 'u']],
        Op::Upper => &[&['U'], &['g', 'U']],
        Op::Toggle => &[&['~'], &['g', '~']],
    };
    for seq in want {
        let got: Vec<Option<Option<char>>> = (0..seq.len()).map(|k| ch(keys, i + k)).collect();
        if got.iter().zip(seq.iter()).all(|(g, w)| *g == Some(Some(*w))) {
            return Parse::Done(seq.len());
        }
        if got.iter().zip(seq.iter()).all(|(g, w)| g.is_none() || *g == Some(Some(*w))) && got.iter().any(Option::is_none) {
            return Parse::More;
        }
    }
    Parse::Invalid
}

fn simple(keys: &[Key], i: usize, visual: bool) -> Parse<(Simple, usize)> {
    use Simple as S;
    let Some(key) = keys.get(i) else { return Parse::More };
    let one = |s| Parse::Done((s, 1));
    let next = ch(keys, i + 1);
    let with_char = |make: &dyn Fn(char) -> S| match next {
        None => Parse::More,
        Some(Some(c)) => Parse::Done((make(c), 2)),
        Some(None) if matches!(keys.get(i + 1), Some(Key::Enter)) => Parse::Done((make('\n'), 2)),
        Some(None) => Parse::Invalid,
    };
    match key {
        Key::Esc | Key::Ctrl('[' | 'c') => one(S::Cancel),
        Key::Delete => one(S::DeleteChar),
        Key::Ctrl('r') => one(S::Redo),
        Key::Ctrl('v') => one(S::VisualBlock),
        Key::Ctrl('d') => one(S::HalfDown),
        Key::Ctrl('u') => one(S::HalfUp),
        Key::Ctrl('f') => one(S::PageDown),
        Key::Ctrl('b') => one(S::PageUp),
        Key::PageDown => one(S::PageDown),
        Key::PageUp => one(S::PageUp),
        Key::Ctrl('e') => one(S::LineDown),
        Key::Ctrl('y') => one(S::LineUp),
        Key::Ctrl('o') => one(S::JumpOlder),
        Key::Ctrl('i') | Key::Tab => one(S::JumpNewer),
        Key::Ctrl('a') => one(S::Increment(true)),
        Key::Ctrl('x') => one(S::Increment(false)),
        Key::Char(c) => match (c, visual) {
            ('x', _) => one(S::DeleteChar),
            ('X', false) => one(S::DeleteCharBack),
            ('s', _) => one(S::Substitute),
            ('S', false) => one(S::SubstituteLine),
            ('D', false) => one(S::DeleteToEnd),
            ('C', false) => one(S::ChangeToEnd),
            ('Y', false) => one(S::YankLine),
            ('X' | 'D', true) => one(S::VisualLines(crate::parse::Op::Delete)),
            ('S' | 'C' | 'R', true) => one(S::VisualLines(crate::parse::Op::Change)),
            ('Y', true) => one(S::VisualLines(crate::parse::Op::Yank)),
            ('r', _) => with_char(&S::Replace),
            ('J', _) => one(S::Join { spaces: true }),
            ('p', _) => one(S::Put { before: false }),
            ('P', _) => one(S::Put { before: true }),
            ('u', false) => one(S::Undo),
            ('.', false) => one(S::Repeat),
            ('~', _) => one(S::ToggleCase),
            ('i', false) => one(S::Insert),
            ('a', false) => one(S::Append),
            ('I', _) => one(S::InsertFirst),
            ('A', _) => one(S::AppendEnd),
            ('o', false) => one(S::OpenBelow),
            ('O', false) => one(S::OpenAbove),
            ('o' | 'O', true) => one(S::SwapEnds),
            ('R', false) => one(S::ReplaceMode),
            ('v', _) => one(S::Visual),
            ('V', _) => one(S::VisualLine),
            ('/', _) => one(S::SearchForward),
            ('?', _) => one(S::SearchBackward),
            (':', _) => one(S::CommandLine),
            ('m', false) => with_char(&S::SetMark),
            ('q', false) => match next {
                None => Parse::More,
                Some(Some(c)) => Parse::Done((S::Record(Some(c)), 2)),
                Some(None) => Parse::Invalid,
            },
            ('@', false) => with_char(&S::Play),
            ('Z', false) => match next {
                None => Parse::More,
                Some(Some('Z')) => Parse::Done((S::SaveQuit, 2)),
                Some(Some('Q')) => Parse::Done((S::QuitForce, 2)),
                _ => Parse::Invalid,
            },
            ('z', _) => match (next, keys.get(i + 1)) {
                (None, _) => Parse::More,
                (Some(Some('z' | '.')), _) => Parse::Done((S::ScrollCenter, 2)),
                (Some(Some('t')), _) | (_, Some(Key::Enter)) => Parse::Done((S::ScrollTop, 2)),
                (Some(Some('b' | '-')), _) => Parse::Done((S::ScrollBottom, 2)),
                (Some(Some('c')), _) => Parse::Done((S::Fold(crate::Fold::Close), 2)),
                (Some(Some('o')), _) => Parse::Done((S::Fold(crate::Fold::Open), 2)),
                (Some(Some('a')), _) => Parse::Done((S::Fold(crate::Fold::Toggle), 2)),
                (Some(Some('M')), _) => Parse::Done((S::Fold(crate::Fold::CloseAll), 2)),
                (Some(Some('R')), _) => Parse::Done((S::Fold(crate::Fold::OpenAll), 2)),
                _ => Parse::Invalid,
            },
            ('g', _) => match next {
                None => Parse::More,
                Some(Some('J')) => Parse::Done((S::Join { spaces: false }, 2)),
                Some(Some('v')) => Parse::Done((S::Reselect, 2)),
                Some(Some('I')) if !visual => Parse::Done((S::InsertColumnZero, 2)),
                _ => Parse::Invalid,
            },
            _ => Parse::Invalid,
        },
        _ => Parse::Invalid,
    }
}

/// Reads a Normal mode command (`visual`: a Visual mode one, where operators apply to the
/// selection at once and `i`/`a` start text objects).
pub(crate) fn command(keys: &[Key], visual: bool, recording: bool) -> Parse<Parsed> {
    let mut i = 0;
    let mut register = None;
    if let Some(Some('"')) = ch(keys, 0) {
        match ch(keys, 1) {
            None => return Parse::More,
            Some(Some(c)) if c.is_ascii_alphanumeric() || "\"+*_-".contains(c) => register = Some(c),
            _ => return Parse::Invalid,
        }
        i = 2;
    }
    let count = count_at(keys, &mut i);
    if i >= keys.len() {
        return Parse::More;
    }
    let done = |cmd| Parse::Done(Parsed { register, count, cmd });
    // `q` ends a recording at once.
    if recording && ch(keys, i) == Some(Some('q')) {
        return done(Cmd::Simple(Simple::Record(None)));
    }
    if visual {
        if let Some(Some(c @ ('i' | 'a'))) = ch(keys, i) {
            return match ch(keys, i + 1) {
                None => Parse::More,
                Some(Some(kind)) => done(Cmd::Operate(Op::Yank, Target::Object { around: c == 'a', kind })),
                Some(None) => Parse::Invalid,
            };
        }
        // In Visual mode an operator applies to the selection (`Target::Lines` stands for it).
        let on_selection = match keys.get(i) {
            Some(Key::Char('x') | Key::Delete) => Some(Op::Delete),
            Some(Key::Char('s')) => Some(Op::Change),
            Some(Key::Char('~')) => Some(Op::Toggle),
            Some(Key::Char('u')) => Some(Op::Lower),
            Some(Key::Char('U')) => Some(Op::Upper),
            _ => None,
        };
        if let Some(op) = on_selection {
            return done(Cmd::Operate(op, Target::Lines));
        }
        match operator(keys, i) {
            Parse::Done((op, _)) => return done(Cmd::Operate(op, Target::Lines)),
            Parse::More => return Parse::More,
            Parse::Invalid => {}
        }
    } else {
        match operator(keys, i) {
            Parse::More => return Parse::More,
            Parse::Done((op, n)) => {
                let mut j = i + n;
                let second = count_at(keys, &mut j);
                let count = match (count, second) {
                    (0, s) => s,
                    (c, 0) => c,
                    (c, s) => c.saturating_mul(s),
                };
                if j >= keys.len() {
                    return Parse::More;
                }
                let done = |cmd| Parse::Done(Parsed { register, count, cmd });
                match doubled(op, keys, j) {
                    Parse::Done(_) => return done(Cmd::Operate(op, Target::Lines)),
                    Parse::More => return Parse::More,
                    Parse::Invalid => {}
                }
                if let Some(Some(c @ ('i' | 'a'))) = ch(keys, j) {
                    return match ch(keys, j + 1) {
                        None => Parse::More,
                        Some(Some(kind)) => done(Cmd::Operate(op, Target::Object { around: c == 'a', kind })),
                        Some(None) => Parse::Invalid,
                    };
                }
                return match motion(keys, j) {
                    Parse::Done((m, _)) => done(Cmd::Operate(op, Target::Motion(m))),
                    Parse::More => Parse::More,
                    Parse::Invalid => Parse::Invalid,
                };
            }
            Parse::Invalid => {}
        }
    }
    // `g` starts motions and other commands: ask both.
    let m = motion(keys, i);
    if let Parse::Done((m, _)) = m {
        return done(Cmd::Move(m));
    }
    match simple(keys, i, visual) {
        Parse::Done((s, _)) => done(Cmd::Simple(s)),
        Parse::More => Parse::More,
        Parse::Invalid if m == Parse::More => Parse::More,
        Parse::Invalid => Parse::Invalid,
    }
}
