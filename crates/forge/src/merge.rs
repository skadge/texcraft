//! The three-way merge of a text file (diff3), by line and then by word: the changes each side
//! made to the base, found with a Myers diff, are put together where they do not overlap. Two
//! sides making the same change is no overlap. Lines are compared without their line ending
//! (`\n` or `\r\n`), and a merged file keeps the line ending the local side uses.
//!
//! Where both sides changed the same lines differently, those lines are merged again token by
//! token (words, whitespace, punctuation, LaTeX's `\commands`, braces, `$` and comments, see
//! `words`): a LaTeX paragraph is often one line, and two people editing different sentences of
//! it make no conflict. What still overlaps, token by token, is a conflict, given as passages
//! ([`Passage`]) by [`merge_text`]. A passage of more than [`MAX_PASSAGE`] bytes on a side, or
//! whose sides differ from the base by more than [`MAX_EDITS`] tokens, is not merged by word (a
//! conflict, as by line), which keeps the cost bounded.
//!
//! Nothing here knows of TeXCraft: it is meant to move to craft-libs.

mod diff;
mod words;

use std::collections::HashMap;
use std::ops::Range;

use similar::{Algorithm, DiffOp, capture_diff_slices};

/// The largest passage, in bytes on one side, merged word by word.
pub const MAX_PASSAGE: usize = 200_000;

/// The most tokens inserted and deleted between the base and a side for a passage merged word by
/// word.
pub const MAX_EDITS: usize = 1_000;

/// Whether `bytes` are text a merge can work on: UTF-8 without NUL.
pub fn is_text(bytes: &[u8]) -> bool {
    !bytes.contains(&0) && std::str::from_utf8(bytes).is_ok()
}

/// What a three-way merge gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Merge {
    /// The merged file.
    Clean(Vec<u8>),
    /// The passages where the sides made different changes to the same text, in order.
    Conflicts(Vec<Passage>),
    /// A side is not text ([`is_text`]).
    NotText,
}

/// A passage both sides changed differently: where it is in each file (byte ranges), and its text
/// there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Passage {
    pub base: Range<usize>,
    pub ours: Range<usize>,
    pub theirs: Range<usize>,
    pub base_text: String,
    pub ours_text: String,
    pub theirs_text: String,
}

/// `base` merged with the changes `ours` and `theirs` made to it; none when they overlap (two
/// different changes to the same words, or next to each other) or a side is not text.
pub fn merge3(base: &[u8], ours: &[u8], theirs: &[u8]) -> Option<Vec<u8>> {
    match merge_text(base, ours, theirs) {
        Merge::Clean(bytes) => Some(bytes),
        Merge::Conflicts(_) | Merge::NotText => None,
    }
}

/// `base` merged with the changes `ours` and `theirs` made to it, or the passages in conflict.
pub fn merge_text(base: &[u8], ours: &[u8], theirs: &[u8]) -> Merge {
    let (Ok(base), Ok(ours), Ok(theirs)) = (std::str::from_utf8(base), std::str::from_utf8(ours), std::str::from_utf8(theirs)) else {
        return Merge::NotText;
    };
    if [base, ours, theirs].iter().any(|t| t.contains('\0')) {
        return Merge::NotText;
    }
    let texts = [base, ours, theirs];
    let [b, o, t] = texts.map(lines);
    let [bk, ok, tk] = [&b, &o, &t].map(|l| l.iter().map(key).collect::<Vec<_>>());
    let [bs, os, ts] = [&b, &o, &t].map(|l| starts(l));
    let ending = ending(ours);
    let mut out = String::with_capacity(ours.len().max(theirs.len()));
    let mut conflicts = Vec::new();
    for c in chunks(&bk, &ok, &tk, &line_matches(&bk, &ok), &line_matches(&bk, &tk)) {
        let spans = [bs[c.base.start]..bs[c.base.end], os[c.ours.start]..os[c.ours.end], ts[c.theirs.start]..ts[c.theirs.end]];
        match c.take {
            Take::Ours => out.push_str(&ours[spans[1].clone()]),
            Take::Theirs => t[c.theirs].iter().for_each(|line| push_line(&mut out, line, ending)),
            Take::Conflict => {
                let [sb, so, st] = [0, 1, 2].map(|i| &texts[i][spans[i].clone()]);
                match merge_words(sb, so, st, ending) {
                    Ok(merged) => out.push_str(&merged),
                    Err(within) => {
                        for ranges in within {
                            let ranges: [Range<usize>; 3] = [0, 1, 2].map(|i| spans[i].start + ranges[i].start..spans[i].start + ranges[i].end);
                            conflicts.push(passage(texts, ranges));
                        }
                    }
                }
            }
        }
    }
    if conflicts.is_empty() { Merge::Clean(out.into_bytes()) } else { Merge::Conflicts(conflicts) }
}

fn passage(texts: [&str; 3], [base, ours, theirs]: [Range<usize>; 3]) -> Passage {
    Passage {
        base_text: texts[0][base.clone()].to_owned(),
        ours_text: texts[1][ours.clone()].to_owned(),
        theirs_text: texts[2][theirs.clone()].to_owned(),
        base,
        ours,
        theirs,
    }
}

/// A passage both sides changed, merged token by token, or where it still conflicts (byte
/// ranges within base, ours and theirs).
fn merge_words(base: &str, ours: &str, theirs: &str, ending: Option<&str>) -> Result<String, Vec<[Range<usize>; 3]>> {
    let whole = || vec![[0..base.len(), 0..ours.len(), 0..theirs.len()]];
    if [base, ours, theirs].iter().any(|t| t.len() > MAX_PASSAGE) {
        return Err(whole());
    }
    let texts = [base, ours, theirs];
    let bounds = texts.map(words::bounds);
    let token = |side: usize, i: usize| &texts[side][bounds[side][i]..bounds[side][i + 1]];
    // Tokens as numbers, line endings all the same.
    let mut ids: HashMap<&str, u32> = HashMap::new();
    let [b, o, t] = [0, 1, 2].map(|side| {
        (0..bounds[side].len() - 1)
            .map(|i| {
                let tok = token(side, i);
                let tok = if words::is_line_end(tok) { "\n" } else { tok };
                let next = u32::try_from(ids.len()).unwrap_or(u32::MAX);
                *ids.entry(tok).or_insert(next)
            })
            .collect::<Vec<u32>>()
    });
    let (Some(in_ours), Some(in_theirs)) = (diff::matches(&b, &o, MAX_EDITS), diff::matches(&b, &t, MAX_EDITS)) else {
        return Err(whole());
    };
    let span = |side: usize, r: &Range<usize>| bounds[side][r.start]..bounds[side][r.end];
    let mut out = String::with_capacity(ours.len().max(theirs.len()));
    let mut conflicts = Vec::new();
    for c in chunks(&b, &o, &t, &in_ours, &in_theirs) {
        match c.take {
            Take::Ours => out.push_str(&ours[span(1, &c.ours)]),
            Take::Theirs => {
                for i in c.theirs {
                    match (token(2, i), ending) {
                        (tok, Some(ending)) if words::is_line_end(tok) => out.push_str(ending),
                        (tok, _) => out.push_str(tok),
                    }
                }
            }
            Take::Conflict => conflicts.push([span(0, &c.base), span(1, &c.ours), span(2, &c.theirs)]),
        }
    }
    if conflicts.is_empty() { Ok(out) } else { Err(conflicts) }
}

/// Which side a chunk of the merge is taken from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Take {
    /// Unchanged, changed by ours only, or changed the same way on both sides.
    Ours,
    /// Changed by theirs only.
    Theirs,
    /// Changed differently by both.
    Conflict,
}

/// A part of the merge: the same part of the base and of each side.
struct Chunk {
    base: Range<usize>,
    ours: Range<usize>,
    theirs: Range<usize>,
    take: Take,
}

/// The base cut where both sides kept an element, each part saying which side it comes from:
/// diff3's chunks, for any elements (lines or tokens) compared as keys. `in_ours` and
/// `in_theirs` give, for each element of the base, the element of the side it is kept as.
fn chunks<K: Eq>(b: &[K], o: &[K], t: &[K], in_ours: &[Option<usize>], in_theirs: &[Option<usize>]) -> Vec<Chunk> {
    let mut out = Vec::new();
    let (mut lo, mut ao, mut at) = (0, 0, 0);
    loop {
        // The next base element both sides kept: the end of the chunk where they may differ.
        let next = (lo..b.len()).find_map(|i| Some((i, in_ours[i]?, in_theirs[i]?)));
        let (i, oi, ti) = next.unwrap_or((b.len(), o.len(), t.len()));
        if (i, oi, ti) == (lo, ao, at) {
            if i == b.len() {
                return out;
            }
            // An element both kept: as ours has it.
            out.push(Chunk { base: lo..lo + 1, ours: ao..ao + 1, theirs: at..at + 1, take: Take::Ours });
            (lo, ao, at) = (lo + 1, ao + 1, at + 1);
            continue;
        }
        let (base_part, ours_part, theirs_part) = (&b[lo..i], &o[ao..oi], &t[at..ti]);
        let take = if ours_part == base_part {
            Take::Theirs
        } else if theirs_part == base_part || ours_part == theirs_part {
            Take::Ours
        } else {
            Take::Conflict
        };
        out.push(Chunk { base: lo..i, ours: ao..oi, theirs: at..ti, take });
        (lo, ao, at) = (i, oi, ti);
    }
}

/// The lines of `text`, each with its line ending.
fn lines(text: &str) -> Vec<&str> {
    text.split_inclusive('\n').collect()
}

/// Where each line starts in its text, then the text's length.
fn starts(lines: &[&str]) -> Vec<usize> {
    let mut at = 0;
    let mut out: Vec<usize> = lines
        .iter()
        .map(|l| {
            let start = at;
            at += l.len();
            start
        })
        .collect();
    out.push(at);
    out
}

/// A line without its line ending, and whether it had one: what lines are compared by.
fn key<'a>(line: &&'a str) -> (&'a str, bool) {
    match line.strip_suffix('\n') {
        Some(l) => (l.strip_suffix('\r').unwrap_or(l), true),
        None => (line, false),
    }
}

/// For each line of `base`, the line of `other` it is kept as, if it is.
fn line_matches(base: &[(&str, bool)], other: &[(&str, bool)]) -> Vec<Option<usize>> {
    let mut at = vec![None; base.len()];
    for op in capture_diff_slices(Algorithm::Myers, base, other) {
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
