//! The formatted view's editing as `texcraft-ui-egui`'s `formatted_edit.rs` does it, without a
//! window: the selection is a pair of source bytes found in the lines of the view each step, and
//! each key or toolbar button makes the source edit the window makes.

use texcraft_latex::edit::{self, Change, Doc, Pos, StyledBy};
use texcraft_latex::table::{self, Align, Borders};
use texcraft_latex::view::{self, ListKind};

use super::lines::{self, Shown, ViewLine};

/// A user's action. Numbers pick a line, a stop or a cell among those there are when it is made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Op {
    Click(u32, u32),
    /// Shift+click or a drag: the selection's head moves there.
    Extend(u32, u32),
    DoubleClick(u32, u32),
    TripleClick(u32),
    SelectAll,
    Left {
        shift: bool,
        word: bool,
    },
    Right {
        shift: bool,
        word: bool,
    },
    Up {
        shift: bool,
    },
    Down {
        shift: bool,
    },
    Home {
        shift: bool,
    },
    End {
        shift: bool,
    },
    Type(String),
    Backspace,
    Delete,
    Enter,
    Tab,
    ShiftTab,
    Paste(String),
    Cut,
    Bar(&'static str),
    Cite(String),
    Ref(String),
    Footnote(String),
    Link(String, String),
    Math(&'static str, String),
    Table(&'static str, u32),
    /// An image chosen in the picker or dropped on the view: a new figure (`figure.insert`).
    Figure,
}

/// What a step changed and how: what the checks need to know.
#[derive(Debug, Clone, Default)]
pub struct Done {
    /// The changes made, of the text before.
    pub changes: Vec<Change>,
    /// The text typed or pasted where the caret was, the selection deleted first.
    pub typed: Option<String>,
    /// A character deleted next to the caret: its line and place, and how many characters of the
    /// view it was (a command shown as a word, `\LaTeX`, is deleted whole).
    pub char_deleted: Option<(Pos, usize)>,
    /// The selection deleted: from, to.
    pub selection_deleted: Option<(Pos, Pos)>,
    /// A change of structure that keeps the words.
    pub keeps_words: bool,
    /// An edit made by more than one change of the text (a selection deleted, then typing).
    pub steps: usize,
}

/// Why a step went wrong in the simulation itself (the source edit could not be made).
#[derive(Debug, Clone)]
pub struct Broken(pub String);

#[derive(Debug, Clone)]
pub struct Editor {
    pub text: String,
    pub anchor: usize,
    pub head: usize,
    pub hint: Option<usize>,
}

impl Editor {
    pub fn new(text: &str) -> Self {
        let mut e = Editor { text: text.to_owned(), anchor: 0, head: 0, hint: None };
        // The caret starts where the window puts it: the first stop of the first line.
        let v = e.shown();
        if let Some(b) = v.lines.first().and_then(|l| l.line.first_stop().and_then(|s| l.line.source_at(s))) {
            (e.anchor, e.head) = (b, b);
        }
        e
    }

    pub fn shown(&self) -> Shown {
        lines::shown(&self.text, Some(self.head))
    }

    pub fn selection(&self, doc: &Doc) -> Option<(Pos, Pos)> {
        Some((doc.locate(&self.text, self.anchor, self.hint)?, doc.locate(&self.text, self.head, self.hint)?))
    }

    fn set_selection(&mut self, doc: &Doc, a: Pos, h: Pos) {
        let (Some(ab), Some(hb)) = (doc.source(a), doc.source(h)) else { return };
        (self.anchor, self.head, self.hint) = (ab, hb, Some(h.line));
    }

    /// Makes `changes`, the selection then `after` (bytes of the new text).
    fn commit(&mut self, changes: &[Change], after: (usize, usize), done: &mut Done) -> Result<(), Broken> {
        check_changes(&self.text, changes)?;
        let (changes, after) =
            if self.anchor == self.head { edit::leaving_empty_markup(&self.text, self.head, changes, after) } else { (changes.to_vec(), after) };
        let (changes, added) = edit::kept_apart(&self.text, &changes);
        let after = (edit::kept_apart_offset(&added, after.0), edit::kept_apart_offset(&added, after.1));
        let changes = changes.as_slice();
        let new = edit::apply(&self.text, changes);
        let after = if after.0 == after.1 { (edit::out_of_ligature(&new, after.0), edit::out_of_ligature(&new, after.1)) } else { after };
        for b in [after.0, after.1] {
            if b > new.len() || !new.is_char_boundary(b) {
                return Err(Broken(format!("the caret after the edit, byte {b}, is not a place in the text ({} bytes)", new.len())));
            }
        }
        if done.steps == 0 {
            done.changes = changes.to_vec();
        }
        done.steps += 1;
        self.text = new;
        (self.anchor, self.head, self.hint) = (after.0, after.1, None);
        Ok(())
    }

    /// Runs `op` on the view `v` of the text.
    pub fn run(&mut self, op: &Op) -> Result<Done, Broken> {
        let v = self.shown();
        let doc = v.doc();
        let mut done = Done::default();
        let Some((a, h)) = self.selection(&doc) else { return Ok(done) };
        match op {
            Op::Click(l, s) => {
                if let Some(p) = pick(&v, *l, *s) {
                    self.set_selection(&doc, p, p);
                }
            }
            Op::Extend(l, s) => {
                if let Some(p) = pick(&v, *l, *s) {
                    self.set_selection(&doc, a, p);
                }
            }
            Op::DoubleClick(l, s) => {
                if let Some((x, y)) = pick(&v, *l, *s).and_then(|p| word_at(&v, p)) {
                    self.set_selection(&doc, x, y);
                }
            }
            Op::TripleClick(l) => {
                if let Some(i) = pick_line(&v, *l)
                    && let Some(line) = doc.lines.get(i)
                    && let (Some(x), Some(y)) = (line.first_stop(), line.last_stop())
                {
                    self.set_selection(&doc, Pos { line: i, at: x }, Pos { line: i, at: y });
                }
            }
            Op::SelectAll => {
                let last = doc.lines.len().saturating_sub(1);
                let first = Pos { line: 0, at: doc.lines.first().and_then(|l| l.first_stop()).unwrap_or(0) };
                let end = Pos { line: last, at: doc.lines.last().and_then(|l| l.last_stop()).unwrap_or(0) };
                self.set_selection(&doc, first, end);
            }
            Op::Left { shift, word } | Op::Right { shift, word } => self.horizontal(&v, (a, h), matches!(op, Op::Left { .. }), *shift, *word),
            Op::Up { shift } | Op::Down { shift } => self.vertical(&doc, (a, h), matches!(op, Op::Up { .. }), *shift),
            Op::Home { shift } | Op::End { shift } => {
                let home = matches!(op, Op::Home { .. });
                if let Some(line) = doc.lines.get(h.line)
                    && let Some(at) = if home { line.first_stop() } else { line.last_stop() }
                {
                    let to = Pos { line: h.line, at };
                    self.set_selection(&doc, if *shift { a } else { to }, to);
                }
            }
            Op::Type(s) => self.type_text(&v, (a, h), s, &mut done)?,
            Op::Backspace | Op::Delete => self.delete(&v, (a, h), matches!(op, Op::Backspace), &mut done)?,
            Op::Enter => self.enter(&v, (a, h), &mut done)?,
            Op::Tab | Op::ShiftTab => {
                let back = matches!(op, Op::ShiftTab);
                if let Some(c) = v.lines.get(h.line).and_then(|l| l.cell) {
                    self.next_cell(&v, h.line, c, back);
                } else {
                    let e = if back { edit::outdent(&self.text, self.head) } else { edit::indent(&self.text, self.head) };
                    done.keeps_words = true;
                    self.structure(e, &mut done)?;
                }
            }
            Op::Paste(s) => {
                if self.takes_structure(&v, (a, h)) {
                    done.selection_deleted = Some((a.min(h), a.max(h)));
                    self.replace(&v, (a, h), "", &mut done)?;
                    return self.then(op, done);
                }
                done.typed = Some(s.clone());
                if a != h {
                    done.selection_deleted = Some((a.min(h), a.max(h)));
                }
                let at = self.insertion(&doc, (a, h));
                let source = edit::paste(&self.text, at, s);
                let source = edit::joined(&self.text, at, &source);
                self.replace_at(&v, (a, h), (&source, edit::paste_caret(&source)), &mut done)?;
            }
            Op::Cut => {
                if a != h {
                    done.selection_deleted = Some((a.min(h), a.max(h)));
                    self.replace(&v, (a, h), "", &mut done)?;
                }
            }
            Op::Bar(id) => self.bar(&v, (a, h), id, &mut done)?,
            Op::Cite(key) => self.insert_atom(&doc, (a, h), &format!("\\cite{{{key}}}"), &mut done)?,
            Op::Ref(key) => self.insert_atom(&doc, (a, h), &format!("\\ref{{{key}}}"), &mut done)?,
            Op::Footnote(t) => {
                let end = edit::typing_point(&self.text, if a == h { self.head } else { doc.source(a.max(h)).unwrap_or(self.head) });
                let source = format!("\\footnote{{{}}}", edit::escape(t));
                let caret = end + source.len();
                self.commit(&[Change { range: end..end, text: source }], (caret, caret), &mut done)?;
            }
            Op::Link(url, t) => self.link(&v, (a, h), url, t, &mut done)?,
            Op::Math(kind, latex) => match *kind {
                "inline" => self.replace(&v, (a, h), &format!("\\({latex}\\)"), &mut done)?,
                "display" if !edit::takes_block(&self.text, self.insertion(&doc, (a, h))) => {}
                "display" => self.replace(&v, (a, h), &format!("\\[ {latex} \\]"), &mut done)?,
                _ => self.new_block(&format!("\\begin{{equation}}\n  {latex}\n\\end{{equation}}"), None, &mut done)?,
            },
            Op::Table(name, n) => self.table_op(name, *n, &mut done)?,
            Op::Figure => self.figure(&mut done)?,
        }
        Ok(done)
    }

    /// `op` again on the view made after the first part of it (the window's pending edits).
    fn then(&mut self, op: &Op, mut done: Done) -> Result<Done, Broken> {
        let more = self.run(op)?;
        done.steps += more.steps;
        done.typed = more.typed;
        Ok(done)
    }

    fn horizontal(&mut self, v: &Shown, (a, h): (Pos, Pos), left: bool, shift: bool, word: bool) {
        let doc = v.doc();
        if !shift && a != h && !word {
            let to = if left { a.min(h) } else { a.max(h) };
            self.set_selection(&doc, to, to);
            return;
        }
        let Some(line) = doc.lines.get(h.line) else { return };
        let next = if word {
            word_stop(v, h, left)
        } else if left {
            line.prev_stop(h.at)
        } else {
            line.next_stop(h.at)
        };
        let to = match next {
            Some(at) => Pos { line: h.line, at },
            None if left && h.line > 0 => Pos { line: h.line - 1, at: doc.lines.get(h.line - 1).and_then(|l| l.last_stop()).unwrap_or(0) },
            None if !left && h.line + 1 < doc.lines.len() => {
                Pos { line: h.line + 1, at: doc.lines.get(h.line + 1).and_then(|l| l.first_stop()).unwrap_or(0) }
            }
            None => h,
        };
        self.set_selection(&doc, if shift { a } else { to }, to);
    }

    /// Up and Down: the line before or after, at the stop nearest the same character (lines are
    /// one row here).
    fn vertical(&mut self, doc: &Doc, (a, h): (Pos, Pos), up: bool, shift: bool) {
        let other = if up { h.line.checked_sub(1) } else { Some(h.line + 1).filter(|l| *l < doc.lines.len()) };
        let to = match other.and_then(|i| Some((i, doc.lines.get(i)?.nearest_stop(h.at)?))) {
            Some((line, at)) => Pos { line, at },
            None => return,
        };
        self.set_selection(doc, if shift { a } else { to }, to);
    }

    fn next_cell(&mut self, v: &Shown, line: usize, (table, _, _): (usize, usize, usize), back: bool) {
        let next = if back { line.checked_sub(1) } else { Some(line + 1) };
        let Some((i, l)) = next.and_then(|i| Some((i, v.lines.get(i).filter(|d| d.cell.is_some_and(|c| c.0 == table))?))) else { return };
        let (first, last) = (l.line.first_stop().unwrap_or(0), l.line.last_stop().unwrap_or(0));
        self.set_selection(&v.doc(), Pos { line: i, at: first }, Pos { line: i, at: last });
    }

    fn takes_structure(&self, v: &Shown, (a, h): (Pos, Pos)) -> bool {
        let doc = v.doc();
        a != h && doc.deleted(&self.text, a, h) != doc.pieces(&self.text, a, h)
    }

    fn insertion(&self, doc: &Doc, (a, h): (Pos, Pos)) -> usize {
        if a == h {
            return edit::typing_point(&self.text, self.head);
        }
        edit::insertion(&self.text, doc, (a, h)).unwrap_or(self.head)
    }

    fn replace(&mut self, v: &Shown, (a, h): (Pos, Pos), source: &str, done: &mut Done) -> Result<(), Broken> {
        self.replace_at(v, (a, h), (source, source.len()), done)
    }

    fn replace_at(&mut self, v: &Shown, (a, h): (Pos, Pos), (source, caret): (&str, usize), done: &mut Done) -> Result<(), Broken> {
        let doc = v.doc();
        if !source.is_empty() && self.takes_structure(v, (a, h)) {
            if let Some(e) = edit::replace_structure(&self.text, &doc, (a, h), (source, caret)) {
                return self.commit(&e.changes, (e.caret, e.caret), done);
            }
            return Ok(());
        }
        let pieces = match (a == h, source.is_empty()) {
            (true, _) => Vec::new(),
            (false, true) => doc.deleted(&self.text, a, h),
            (false, false) => doc.pieces(&self.text, a, h),
        };
        // A deletion of what is alone on its line takes the line's end (no blank line is left).
        let pieces: Vec<_> = if source.is_empty() { pieces.into_iter().map(|r| edit::alone_on_line(&self.text, r)).collect() } else { pieces };
        let mut changes: Vec<Change> = pieces.iter().map(|r| Change { range: r.clone(), text: String::new() }).collect();
        let at = self.insertion(&doc, (a, h));
        match changes.first_mut() {
            Some(first) if first.range.start <= at => first.text = source.to_owned(),
            _ if source.is_empty() => {}
            _ => changes.insert(0, Change { range: at..at, text: source.to_owned() }),
        }
        let at = changes.first().map_or(at, |c| c.range.start.min(at));
        let caret = at + caret.min(source.len());
        let caret = if source.is_empty() && a != h {
            let caret = edit::deleted_caret(&self.text, &doc, (a, h), &changes).unwrap_or(caret);
            let after = edit::apply(&self.text, &changes);
            edit::in_float(&after, edit::into_item(&after, caret))
        } else {
            caret
        };
        self.commit(&changes, (caret, caret), done)
    }

    fn type_text(&mut self, v: &Shown, (a, h): (Pos, Pos), s: &str, done: &mut Done) -> Result<(), Broken> {
        if s == " " && a == h {
            let chars = v.lines.get(h.line).map(|l| l.chars.clone()).unwrap_or_default();
            let before = h.at.checked_sub(1).and_then(|i| chars.get(i));
            if before.is_none_or(|c| c.is_whitespace()) || chars.get(h.at).is_some_and(|c| *c == ' ') {
                return Ok(());
            }
        }
        if self.takes_structure(v, (a, h)) {
            done.selection_deleted = Some((a.min(h), a.max(h)));
            self.replace(v, (a, h), "", done)?;
            let more = self.run(&Op::Type(s.to_owned()))?;
            done.steps += more.steps;
            done.typed = more.typed;
            return Ok(());
        }
        let at = self.head;
        let source = edit::joined(&self.text, self.insertion(&v.doc(), (a, h)), &edit::escape(s));
        done.typed = Some(s.to_owned());
        if a != h {
            done.selection_deleted = Some((a.min(h), a.max(h)));
        }
        let before = self.text.clone();
        self.replace(v, (a, h), &source, done)?;
        if a == h && s.ends_with('.') && self.text != before {
            let at = at + source.len();
            if let Some(c) = edit::ellipsis(&self.text, at) {
                let caret = c.range.start + c.text.len();
                self.commit(std::slice::from_ref(&c), (caret, caret), done)?;
            }
        }
        Ok(())
    }

    fn delete(&mut self, v: &Shown, (a, h): (Pos, Pos), back: bool, done: &mut Done) -> Result<(), Broken> {
        let doc = v.doc();
        if a != h {
            done.selection_deleted = Some((a.min(h), a.max(h)));
            return self.replace(v, (a, h), "", done);
        }
        let Some(line) = doc.lines.get(h.line) else { return Ok(()) };
        if let Some(r) = if back { line.atom_before(h.at) } else { line.atom_after(h.at) } {
            (self.anchor, self.head, self.hint) = (r.start, r.end, Some(h.line));
            return Ok(());
        }
        if let Some(r) = if back { line.char_before(h.at) } else { line.char_after(h.at) } {
            let width = line.char_width(h.at, back);
            done.char_deleted = Some((if back { Pos { line: h.line, at: h.at - width } } else { h }, width));
            if let Some((field, caret)) = edit::emptied_field(&self.text, &r) {
                return self.commit(&[Change { range: field, text: String::new() }], (caret, caret), done);
            }
            let r = edit::alone_on_line(&self.text, r);
            let mut changes = vec![Change { range: r.clone(), text: String::new() }];
            let mut caret = r.start;
            if let Some(empty) = edit::empty_styles_at(&self.text, self.head).filter(|e| e.end <= r.start || r.end <= e.start) {
                if !back {
                    caret = empty.start;
                }
                changes.push(Change { range: empty, text: String::new() });
                changes.sort_by_key(|c| c.range.start);
            }
            let caret = edit::map_offset(&changes, caret);
            return self.commit(&changes, (caret, caret), done);
        }
        if let Some(at) = if back { line.prev_stop(h.at) } else { line.next_stop(h.at) } {
            self.set_selection(&doc, Pos { line: h.line, at }, Pos { line: h.line, at });
            return Ok(());
        }
        done.keeps_words = true;
        if back && let Some(e) = edit::backspace_at_start(&self.text, self.head) {
            return self.commit(&e.changes, (e.caret, e.caret), done);
        }
        let gap = if back { h.line.checked_sub(1).and_then(|l| doc.gap_after(&self.text, l)) } else { doc.gap_after(&self.text, h.line) };
        if let Some(gap) = gap {
            return self.commit(&[Change { range: gap.clone(), text: String::new() }], (gap.start, gap.start), done);
        }
        Ok(())
    }

    fn enter(&mut self, v: &Shown, (a, h): (Pos, Pos), done: &mut Done) -> Result<(), Broken> {
        if a != h {
            // A selected atom opens its editor instead; other selections go first.
            let (lo, hi) = (self.anchor.min(self.head), self.anchor.max(self.head));
            if v.doc().lines.get(h.line).is_some_and(|l| l.atom_after(a.min(h).at) == Some(lo..hi)) {
                return Ok(());
            }
            done.selection_deleted = Some((a.min(h), a.max(h)));
            self.replace(v, (a, h), "", done)?;
            let more = self.run(&Op::Enter)?;
            done.steps += more.steps;
            // The line is broken after: the words are what is checked.
            done.keeps_words = true;
            return Ok(());
        }
        done.keeps_words = true;
        match edit::enter(&self.text, self.head) {
            Some(e) => self.commit(&e.changes, (e.caret, e.caret), done),
            None => Ok(()),
        }
    }

    fn structure(&mut self, e: Option<edit::SourceEdit>, done: &mut Done) -> Result<(), Broken> {
        match e {
            Some(e) => self.commit(&e.changes, (e.caret, e.caret), done),
            None => Ok(()),
        }
    }

    fn bar(&mut self, v: &Shown, (a, h): (Pos, Pos), id: &str, done: &mut Done) -> Result<(), Broken> {
        let at = self.head;
        done.keeps_words = true;
        let names = edit::heading_commands(&self.text);
        let heading = match id {
            "heading_1" => Some(names[0]),
            "heading_2" => Some(names[1]),
            "heading_3" => Some(names[2]),
            _ => None,
        };
        if let Some(name) = heading {
            let here = edit::here(&self.text, at);
            if here.heading.is_some() && here.heading == view::heading_level(name) {
                return Ok(());
            }
            if let Some(e) = edit::heading(&self.text, at, name) {
                self.commit(&e.changes, (e.caret, e.caret), done)?;
            }
            return Ok(());
        }
        match id {
            "bold" | "italic" | "code" => {
                let here = edit::here(&self.text, at);
                let (name, by) = match id {
                    "bold" => ("textbf", here.bold_by),
                    "italic" => ("textit", here.italic_by),
                    _ => ("texttt", here.code_by),
                };
                match by {
                    Some(StyledBy::Declaration) => Ok(()),
                    Some(StyledBy::Emph) => self.style(v, (a, h), "emph", done),
                    _ => self.style(v, (a, h), name, done),
                }
            }
            "body" => match edit::body(&self.text, at) {
                Some(e) => self.commit(&e.changes, (e.caret, e.caret), done),
                None => Ok(()),
            },
            "itemize" | "enumerate" => {
                let here = edit::here(&self.text, at);
                let same = matches!((here.list.map(|l| l.0), id), (Some(ListKind::Bullets), "itemize") | (Some(ListKind::Numbers), "enumerate"));
                if same {
                    return self.structure(edit::outdent(&self.text, at), done);
                }
                self.structure(edit::set_list(&self.text, at, id), done)
            }
            "table" => {
                done.keeps_words = false;
                self.new_block(TABLE, Some("Heading"), done)
            }
            _ => Ok(()),
        }
    }

    fn style(&mut self, v: &Shown, (a, h): (Pos, Pos), name: &str, done: &mut Done) -> Result<(), Broken> {
        let doc = v.doc();
        let empty = edit::empty_style_at(&self.text, self.head);
        if a == h
            && let Some(r) = empty.clone().filter(|r| self.text.get(r.clone()).is_some_and(|s| s.starts_with(&format!("\\{name}{{"))))
        {
            return self.commit(&[Change { range: r.clone(), text: String::new() }], (r.start, r.start), done);
        }
        let empty = empty.is_some();
        let (a, h) = if a == h && !empty { word_at(v, h).unwrap_or((a, h)) } else { (a.min(h), a.max(h)) };
        if a == h {
            // Where typing goes (in a heading or a caption, not after its label).
            let at = edit::typing_point(&self.text, self.head);
            let inserted = format!("\\{name}{{}}");
            let caret = at + inserted.len() - 1;
            return self.commit(&[Change { range: at..at, text: inserted }], (caret, caret), done);
        }
        let mut pieces = Vec::new();
        for i in a.line..=h.line {
            let Some(line) = doc.lines.get(i) else { continue };
            let from = if i == a.line { a.at } else { 0 };
            let to = if i == h.line { h.at } else { line.len() };
            pieces.extend(line.pieces(from, to));
        }
        let changes = edit::toggle_style(&self.text, &pieces, name);
        if changes.is_empty() {
            return Ok(());
        }
        let (Some(start), Some(end)) = (pieces.first().map(|r| r.start), pieces.last().map(|r| r.end)) else { return Ok(()) };
        let start = edit::map_offset(&changes, start);
        let end = map_before(&changes, end);
        self.commit(&changes, (start, end), done)
    }

    fn insert_atom(&mut self, doc: &Doc, (a, h): (Pos, Pos), source: &str, done: &mut Done) -> Result<(), Broken> {
        let end = edit::typing_point(&self.text, if a == h { self.head } else { doc.source(a.max(h)).unwrap_or(self.head) });
        let after_word = self.text.get(..end).and_then(|s| s.chars().next_back()).is_some_and(char::is_alphanumeric);
        let inserted = if after_word { format!("~{source}") } else { source.to_owned() };
        let caret = end + inserted.len();
        self.commit(&[Change { range: end..end, text: inserted }], (caret, caret), done)
    }

    fn link(&mut self, v: &Shown, (a, h): (Pos, Pos), url: &str, text: &str, done: &mut Done) -> Result<(), Broken> {
        if a == h {
            let source = if text.is_empty() { format!("\\url{{{url}}}") } else { format!("\\href{{{url}}}{{{}}}", edit::escape(text)) };
            return self.replace(v, (a, h), &source, done);
        }
        if let [one] = v.doc().pieces(&self.text, a, h).as_slice()
            && !v
                .doc()
                .lines
                .get(a.min(h).line)
                .is_some_and(|l| l.parts().iter().any(|p| matches!(p, edit::Part::Atom { range, .. } if range == one)))
        {
            let open = format!("\\href{{{url}}}{{");
            let changes = [Change { range: one.start..one.start, text: open.clone() }, Change { range: one.end..one.end, text: "}".to_owned() }];
            let caret = one.end + open.len() + 1;
            return self.commit(&changes, (caret, caret), done);
        }
        Ok(())
    }

    fn new_block(&mut self, source: &str, select: Option<&str>, done: &mut Done) -> Result<(), Broken> {
        let caret = self.head;
        let at = edit::block_point(&self.text, caret);
        let before = self.text.get(..at).unwrap_or_default();
        let lead = if before.is_empty() || before.ends_with("\n\n") {
            ""
        } else if before.ends_with('\n') {
            "\n"
        } else {
            "\n\n"
        };
        let inserted = format!("{lead}{source}\n");
        let start = at + lead.len();
        let sel = select.and_then(|s| source.find(s).map(|i| (start + i, start + i + s.len()))).unwrap_or((start, start));
        let (changes, sel) = edit::dropping_empty_style(&self.text, caret, Change { range: at..at, text: inserted }, sel);
        self.commit(&changes, sel, done)
    }

    /// A figure as the window inserts it for an image picked or dropped (`App::figure_for_drop`,
    /// the engine's `figure.insert`): on lines of its own where a block goes, the caret in its
    /// caption.
    fn figure(&mut self, done: &mut Done) -> Result<(), Broken> {
        if let Some(empty) = edit::empty_markup_at(&self.text, self.head) {
            self.commit(&[Change { range: empty.clone(), text: String::new() }], (empty.start, empty.start), done)?;
        }
        let at = edit::block_point(&self.text, self.head);
        let changes = texcraft_latex::figure::FigureChanges {
            image: Some("img/kiln.png".to_owned()),
            width: Some(0.8),
            placement: Some("htbp".to_owned()),
            caption: None,
            label: Some("fig:kiln".to_owned()),
        };
        let (before, after) = (self.text.get(..at).unwrap_or_default(), self.text.get(at..).unwrap_or_default());
        let lead = if before.is_empty() || before.ends_with('\n') { "" } else { "\n" };
        let trail = if after.is_empty() || after.starts_with('\n') { "" } else { "\n" };
        let change = Change { range: at..at, text: format!("{lead}{}{trail}", texcraft_latex::figure::new_figure(&changes)) };
        // The caret in its caption.
        let caret = change.text.find("\\caption{").map_or(at, |i| at + i + "\\caption{".len());
        self.commit(&[change], (caret, caret), done)
    }

    /// A table toolbar's operation on the cell holding the caret, as `table.edit` makes it.
    fn table_op(&mut self, name: &str, n: u32, done: &mut Done) -> Result<(), Broken> {
        let Some(Ok(t)) = table::find(&self.text, self.head) else { return Ok(()) };
        let Some((r, c)) = t.rows.iter().enumerate().find_map(|(r, row)| {
            let cell = row.cells.iter().find(|c| c.range.start <= self.head && self.head <= c.range.end.max(c.end()))?;
            Some((r, cell.column))
        }) else {
            return Ok(());
        };
        let text = self.text.clone();
        let made = match name {
            "insert_row_above" | "insert_row_below" => t.insert_row(&text, r, name.ends_with("below")),
            "delete_row" => t.delete_row(&text, r),
            "insert_column_left" | "insert_column_right" => t.insert_column(&text, c, name.ends_with("right")),
            "delete_column" => t.delete_column(c),
            "align_column" => t.set_align(c, [Align::Left, Align::Center, Align::Right][n as usize % 3]),
            "borders" => t.set_borders(&text, [Borders::None, Borders::All, Borders::Booktabs][n as usize % 3]),
            "header" => t.set_header(&text, n.is_multiple_of(2)),
            "merge" => t.merge(&text, r, c, 2 + n as usize % 2),
            "split" => t.split(&text, r, c),
            _ => return Ok(()),
        };
        let Ok(made) = made else { return Ok(()) };
        if made.changes.is_empty() {
            return Ok(());
        }
        let at = edit::map_offset(&made.changes, self.head);
        self.commit(&made.changes, (at, at), done)?;
        let at = table::cell_caret(&self.text, self.head);
        (self.anchor, self.head) = (at, at);
        Ok(())
    }
}

const TABLE: &str = "\\begin{table}[htbp]\n  \\centering\n  \\begin{tabular}{ll}\n    \\hline\n    Heading & Heading \\\\\n    \\hline\n    Text & Text \\\\\n    \\hline\n  \\end{tabular}\n  \\caption{Caption}\n  \\label{tab:new}\n\\end{table}";

/// Changes the window's buffer takes: in order, not overlapping, on the text and at its characters'
/// boundaries.
pub fn check_changes(text: &str, changes: &[Change]) -> Result<(), Broken> {
    for w in changes.windows(2) {
        if let [x, y] = w
            && x.range.end > y.range.start
        {
            return Err(Broken(format!("overlapping or unsorted changes: {:?} then {:?}", x.range, y.range)));
        }
    }
    for c in changes {
        if c.range.start > c.range.end || text.get(c.range.clone()).is_none() {
            return Err(Broken(format!("a change off the text or off a character boundary: {:?}", c.range)));
        }
    }
    Ok(())
}

fn map_before(changes: &[Change], at: usize) -> usize {
    let mut delta: isize = 0;
    for c in changes {
        if c.range.start >= at && !(c.range.start == at && c.range.end > at && c.text.is_empty()) {
            break;
        }
        if at < c.range.end {
            return c.range.start.saturating_add_signed(delta);
        }
        delta += c.text.len() as isize - (c.range.end - c.range.start) as isize;
    }
    at.saturating_add_signed(delta)
}

/// The line `l` picks among those with stops.
fn pick_line(v: &Shown, l: u32) -> Option<usize> {
    let with: Vec<usize> = v.lines.iter().enumerate().filter(|(_, x)| x.line.first_stop().is_some()).map(|(i, _)| i).collect();
    with.get(l as usize % with.len().max(1)).copied()
}

/// The stop `(l, s)` picks.
fn pick(v: &Shown, l: u32, s: u32) -> Option<Pos> {
    let line = pick_line(v, l)?;
    let stops = lines::stops(&v.lines.get(line)?.line);
    Some(Pos { line, at: *stops.get(s as usize % stops.len().max(1))? })
}

fn word_at(v: &Shown, h: Pos) -> Option<(Pos, Pos)> {
    let ViewLine { line, chars, .. } = v.lines.get(h.line)?;
    let is_word = |i: usize| chars.get(i).is_some_and(|c| c.is_alphanumeric());
    if !(is_word(h.at) || (h.at > 0 && is_word(h.at - 1))) {
        return None;
    }
    let (mut s, mut e) = (h.at, h.at);
    while s > 0 && is_word(s - 1) {
        s -= 1;
    }
    while is_word(e) {
        e += 1;
    }
    let s = if line.is_stop(s) { s } else { line.next_stop(s)? };
    let e = if line.is_stop(e) { e } else { line.prev_stop(e)? };
    (s < e).then_some((Pos { line: h.line, at: s }, Pos { line: h.line, at: e }))
}

fn word_stop(v: &Shown, h: Pos, left: bool) -> Option<usize> {
    let ViewLine { line, chars, .. } = v.lines.get(h.line)?;
    let is_word = |i: usize| chars.get(i).is_some_and(|c| c.is_alphanumeric());
    let mut p = h.at;
    if left {
        if p == 0 || line.prev_stop(p).is_none() {
            return None;
        }
        while p > 0 && !is_word(p - 1) {
            p -= 1;
        }
        while p > 0 && is_word(p - 1) {
            p -= 1;
        }
        if line.is_stop(p) { Some(p) } else { line.next_stop(p).filter(|s| *s < h.at).or_else(|| line.prev_stop(h.at)) }
    } else {
        line.next_stop(p)?;
        while p < chars.len() && !is_word(p) {
            p += 1;
        }
        while p < chars.len() && is_word(p) {
            p += 1;
        }
        if line.is_stop(p) { Some(p) } else { line.prev_stop(p).filter(|s| *s > h.at).or_else(|| line.next_stop(h.at)) }
    }
}
