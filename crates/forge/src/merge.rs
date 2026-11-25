//! The three-way merge of a text file, line by line (diff3): the changes each side made to the
//! base, found with a Myers diff, are put together where they do not overlap. Two sides making
//! the same change is no overlap. Lines are compared without their line ending (`\n` or `\r\n`),
//! and a merged file keeps the line ending the local side uses.

use similar::{Algorithm, DiffOp, capture_diff_slices_by_key};

/// Whether `bytes` are text a merge can work on: UTF-8 without NUL.
pub fn is_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// `base` merged with the changes `ours` and `theirs` made to it; none when they overlap (two
/// different changes to the same lines, or next to each other) or a side is not text.
pub fn merge3(base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    let (Ok(base), Ok(ours), Ok(theirs)) = (std::str::from_utf8(base), std::str::from_utf8(ours), std::str::from_utf8(theirs)) else {
        return None;
    };
    if [base, ours, theirs].iter().any(|t| t.contains('\0')) {
        return None;
    }
    let (b, o, t) = (lines(base), lines(ours), lines(theirs));
    let in_ours = matches(&b, &o);
    let in_theirs = matches(&b, &t);
    let ending = ending(ours);
    let mut out = String::with_capacity(ours.len().max(theirs.len()));
    let (mut lo, mut ao, mut at) = (0, 0, 0);
    loop {
        // The next base line both sides kept: the end of the chunk where they may differ.
        let next = (lo..b.len()).find_map(|i| Some((i, in_ours[i]?, in_theirs[i]?)));
        let (i, oi, ti) = next.unwrap_or((b.len(), o.len(), t.len()));
        if (i, oi, ti) == (lo, ao, at) {
            if i == b.len() {
                return Some(out.into_bytes());
            }
            // A line both kept: as ours has it.
            out.push_str(o[ao]);
            (lo, ao, at) = (lo + 1, ao + 1, at + 1);
            continue;
        }
        let (base_part, ours_part, theirs_part) = (&b[lo..i], &o[ao..oi], &t[at..ti]);
        if same(ours_part, base_part) {
            // Only theirs changed these lines.
            for line in theirs_part {
                push_line(&mut out, line, ending);
            }
        } else if same(theirs_part, base_part) || same(ours_part, theirs_part) {
            // Only ours changed them, or both made the same change.
            ours_part.iter().for_each(|l| out.push_str(l));
        } else {
            return None;
        }
        (lo, ao, at) = (i, oi, ti);
    }
}

/// The lines of `text`, each with its line ending.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// A line without its line ending, and whether it had one: what lines are compared by.
fn key<'a>(line: &&'a str) -> (&'a str, bool) {
    match line.strip_suffix('\n') {
        Some(l) => (l.strip_suffix('\r').unwrap_or(l), true),
        None => (line, false),
    }
}

fn same(a: &[&str], b: &[&str]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| key(x) == key(y))
}

/// For each line of `base`, the line of `other` it is kept as, if it is.
fn matches(base: &[&str], other: &[&str]) -> Vec<Option<usize>> {
    let mut at = vec![None; base.len()];
    for op in capture_diff_slices_by_key(Algorithm::Myers, base, other, key) {
        if let DiffOp::Equal { old_index, new_index, len } = op {
            for k in 0..len {
                at[old_index + k] = Some(new_index + k);
            }
        }
    }
    at
}

/// The line ending every line of `text` has (`\r\n` or `\n`); none when it has none, or both.
fn ending(text: &str) -> Option<&'static str> {
    let ends = text.matches('\n').count();
    match text.matches("\r\n").count() {
        _ if ends == 0 => None,
        0 => Some("\n"),
        n if n == ends => Some("\r\n"),
        _ => None,
    }
}

/// Appends `line`, with the line ending the local file uses, if it uses one only.
fn push_line(out: &mut String, line: &str, ending: Option<&str>) {
    match (line.strip_suffix('\n'), ending) {
        (Some(l), Some(ending)) => {
            out.push_str(l.strip_suffix('\r').unwrap_or(l));
            out.push_str(ending);
        }
        _ => out.push_str(line),
    }
}
