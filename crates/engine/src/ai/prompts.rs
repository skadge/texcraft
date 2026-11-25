//! The instructions each kind of AI request sends (issue #15): the app's own, which the user can
//! change in Settings › AI › Prompts (`ai.prompts`, `ai.set_prompt`).
//!
//! A prompt is a template: `{name}` is replaced by a part of the request (the selection, the
//! build message, the description …), once, so a part's own text is never read as a placeholder.
//! A line holding only a placeholder whose part is empty is left out, with the blank line after
//! it. Each kind has placeholders it cannot do without ([`Placeholder::required`]); a changed
//! prompt that lost one is kept, for the user to fix, but not used: the app's own is sent instead,
//! and that is said. Only changed prompts are kept in the preferences, so a kind the user left
//! alone follows the app's wording when it changes. What comes back is checked as before (the
//! LaTeX kept, the result typeset, the entry read) whatever the prompt says.

use craft_ai_core::{CapabilityId, InputRole, Payload, Request};

use crate::Result;

/// A part of a request a prompt can place.
#[derive(Debug, Clone, Copy)]
pub struct Placeholder {
    pub name: &'static str,
    /// The request makes no sense without it: a prompt that lacks it is not used.
    pub required: bool,
    /// What it is replaced by, for Settings › AI › Prompts.
    pub means: &'static str,
}

/// A kind of request's prompt: its default and its placeholders.
#[derive(Debug, Clone, Copy)]
pub struct PromptSpec {
    /// As [`super::REQUESTS`] names it.
    pub kind: &'static str,
    pub default: &'static str,
    pub placeholders: &'static [Placeholder],
}

/// The longest prompt kept, in characters.
pub const MAX_PROMPT_CHARS: usize = 20_000;

const fn p(name: &'static str, required: bool, means: &'static str) -> Placeholder {
    Placeholder { name, required, means }
}

const BEFORE: Placeholder =
    p("before", false, "The text just before the selection, for context, between <<<BEFORE and BEFORE>>> (left out at the start of the file).");
const AFTER: Placeholder =
    p("after", false, "The text just after the selection, for context, between <<<AFTER and AFTER>>> (left out at the end of the file).");
const PASSAGE: Placeholder = p("passage", true, "The selected text.");

/// Every kind's prompt, in the order of [`super::REQUESTS`].
pub const PROMPTS: [PromptSpec; 8] = [
    PromptSpec {
        kind: "explain",
        default: "You help someone writing a LaTeX document understand a message from building it. \
Explain in plain language, in a few sentences, what the message means and what in the source causes it. \
If changing the source lines shown would fix it, end your answer with all of those lines as they should be, \
between a line <<<FIX and a line FIX>>>, changing nothing else. If no change to them fixes it, leave the markers out.\n\n\
{message}\n\n{source}\n\n{preamble}\n",
        placeholders: &[
            p("message", true, "The build message: where it comes from, its help, its file and line."),
            p("source", true, "The source lines around the message, between <<<SOURCE and SOURCE>>> (left out when the message has no line)."),
            p("preamble", false, "The main document's preamble, between <<<PREAMBLE and PREAMBLE>>>, when the message is in it."),
        ],
    },
    PromptSpec {
        kind: "rewrite",
        default: "You rewrite a passage of a LaTeX document. \
Keep every LaTeX command, every \\label, \\ref and \\cite with its key, and every formula exactly as written; change only the words around them. \
Answer with the rewritten passage alone, between a line <<<TEXT and a line TEXT>>>. {how}\n\n\
{before}\n\nThe passage:\n<<<PASSAGE\n{passage}\nPASSAGE>>>\n\n{after}\n",
        placeholders: &[
            p("how", true, "How to rewrite it, in a sentence: clearer, shorter, more formal, its grammar and spelling only, or translated."),
            PASSAGE,
            BEFORE,
            AFTER,
            p("language", false, "The language chosen for Translate to…, in English words (empty for the other rewrites)."),
        ],
    },
    PromptSpec {
        kind: "instruct",
        default: "You change a passage of a LaTeX document as its author asks. \
Keep every \\label, \\ref and \\cite of the passage with its key, and every formula of it exactly as written; \
you may add text, and new LaTeX, as the request needs. \
Answer with the changed passage alone, between a line <<<TEXT and a line TEXT>>>.\n\n\
The author's request: {instruction}\n\n{before}\n\nThe passage:\n<<<PASSAGE\n{passage}\nPASSAGE>>>\n\n{after}\n",
        placeholders: &[p("instruction", true, "What you asked for, in your words."), PASSAGE, BEFORE, AFTER],
    },
    PromptSpec {
        kind: "complete",
        default: "You continue a paragraph of a LaTeX document as its author would. \
Give only the next few words that follow the text (at most one sentence, on one line), in its language and style, as valid LaTeX. \
Do not repeat the text. Answer with the continuation alone.\n\n<<<TEXT\n{text}\nTEXT>>>\n",
        placeholders: &[p("text", true, "The paragraph being typed, up to the caret.")],
    },
    PromptSpec {
        kind: "make",
        default: "You write LaTeX for a document whose preamble is shown. \
Write only what is asked, to go in the document's body as it is: no \\documentclass, no \\begin{document}. \
Use the packages the preamble loads; if what you write needs another one, give its \\usepackage lines (and any \\usetikzlibrary lines) between a line <<<PACKAGES and a line PACKAGES>>>. \
Answer with the LaTeX between a line <<<LATEX and a line LATEX>>>. {kind}\n\n\
What to write: {description}\n\n{preamble}\n\n{images}\n\n{again}\n",
        placeholders: &[
            p("kind", true, "What kind of thing to write, in a sentence: a table, an equation, a TikZ diagram or a figure, and what it holds."),
            p("description", true, "Your description of it."),
            p("preamble", false, "The main document's preamble, between <<<PREAMBLE and PREAMBLE>>>."),
            p("images", false, "For a figure: the project's images, one path a line (left out for the other kinds)."),
            p("again", false, "Asking again with the error: the last answer and what the typesetting engine said of it (left out otherwise)."),
        ],
    },
    PromptSpec {
        kind: "reference",
        default: "You draft a BibTeX entry for the work a title or a DOI names. \
Give only the fields you know (author, title, year, and journal, booktitle or publisher, volume, number, pages, doi); leave out what you do not know rather than guess. \
Answer with the entry alone between a line <<<BIB and a line BIB>>>.\n\nThe work: {work}\n",
        placeholders: &[p("work", true, "The title or DOI you typed.")],
    },
    PromptSpec {
        kind: "ask",
        default: "You answer questions about a LaTeX project from its files, shown below with their lines numbered. \
Answer in plain language, briefly. When you refer to a place in a file, write it as path:line (such as main.tex:12), with the numbers shown. \
If the files do not answer the question, say so.\n\nThe question: {question}\n\n{files}\n",
        placeholders: &[
            p("question", true, "Your question."),
            p("files", true, "The open file, or every text file of the project, each between <<<FILE path and FILE>>>, its lines numbered."),
        ],
    },
    PromptSpec {
        kind: "review",
        default: "You review a section of a LaTeX document as a careful colleague would: \
point out unclear sentences, mistakes in reasoning, grammar or LaTeX, and what needs explaining. \
For each remark, copy the exact words it is about, as written in the text (a short passage, at most one sentence), and write the remark, as\n\
QUOTE: the exact words\nREMARK: the remark\n\
with a blank line between remarks. Give at most 12 remarks; when there is nothing to remark, answer NONE.\n\n\
<<<SECTION\n{section}\nSECTION>>>\n",
        placeholders: &[p("section", true, "The section at the caret.")],
    },
];

/// The prompt of a kind of request (as [`super::REQUESTS`] names it).
pub fn spec(kind: &str) -> Option<&'static PromptSpec> {
    PROMPTS.iter().find(|s| s.kind == kind)
}

/// The required placeholders `template` lacks.
pub fn missing(spec: &PromptSpec, template: &str) -> Vec<&'static str> {
    spec.placeholders.iter().filter(|p| p.required && !template.contains(&format!("{{{}}}", p.name))).map(|p| p.name).collect()
}

/// `template` with each `{name}` of `values` replaced by its value, in one pass. A line holding
/// only a placeholder whose value is empty is left out, and the blank line after it.
pub fn render(template: &str, values: &[(&str, &str)]) -> String {
    let empty = |line: &str| {
        let t = line.trim();
        t.strip_prefix('{').and_then(|t| t.strip_suffix('}')).is_some_and(|name| values.iter().any(|(n, v)| *n == name && v.is_empty()))
    };
    let lines: Vec<&str> = template.split('\n').collect();
    let mut out: Vec<String> = Vec::with_capacity(lines.len());
    let mut skip_blank = false;
    for line in lines {
        if skip_blank && line.trim().is_empty() {
            skip_blank = false;
            continue;
        }
        skip_blank = false;
        if empty(line) {
            skip_blank = true;
            continue;
        }
        out.push(substitute(line, values));
    }
    let mut text = out.join("\n").trim_end().to_owned();
    text.push('\n');
    text
}

fn substitute(line: &str, values: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(line.len());
    let mut rest = line;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        let found = after.find('}').and_then(|close| values.iter().find(|(n, _)| *n == &after[..close]).map(|(_, v)| (close, *v)));
        match found {
            Some((close, value)) => {
                out.push_str(value);
                rest = &after[close + 1..];
            }
            None => {
                out.push('{');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// A request's parts: its placeholder, its text, and how the request log counts it (`None`: with
/// the instructions, as what the user typed).
pub(super) type Parts = Vec<(&'static str, String, Option<InputRole>)>;

/// The whole text `template` sends with `parts`.
pub(super) fn prompt(template: &str, parts: &Parts) -> String {
    let values: Vec<(&str, &str)> = parts.iter().map(|(n, v, _)| (*n, v.as_str())).collect();
    render(template, &values)
}

/// The request as the suite's consent and log see it: the instructions with what the user typed,
/// then each part of the document the template places, by role and size.
pub(super) fn request(capability: &str, template: &str, parts: &Parts) -> Result<Request> {
    let capability = CapabilityId::parse(capability).map_err(|e| e.to_string())?;
    let values: Vec<(&str, &str)> = parts.iter().map(|(n, v, role)| (*n, if role.is_some() { "" } else { v.as_str() })).collect();
    let mut r = Request::new(capability).with_input(InputRole::Prompt, Payload::text(render(template, &values)));
    for (name, value, role) in parts {
        if let Some(role) = role
            && !value.is_empty()
            && template.contains(&format!("{{{name}}}"))
        {
            r = r.with_input(role.clone(), Payload::text(value.clone()));
        }
    }
    Ok(r)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_value_is_placed_once_and_never_read_as_a_placeholder_itself() {
        let out = render("A {text} B \\begin{document} {nope}\n", &[("text", "{text} and {nope}")]);
        assert_eq!(out, "A {text} and {nope} B \\begin{document} {nope}\n");
    }

    #[test]
    fn an_empty_part_on_its_own_line_leaves_no_gap() {
        let out = render("Head\n\n{before}\n\nBody {x}\n\n{after}\n", &[("before", ""), ("x", "1"), ("after", "")]);
        assert_eq!(out, "Head\n\nBody 1\n");
    }

    #[test]
    fn every_default_has_its_required_placeholders() {
        for s in PROMPTS {
            assert!(missing(&s, s.default).is_empty(), "{} lacks {:?}", s.kind, missing(&s, s.default));
        }
    }
}
