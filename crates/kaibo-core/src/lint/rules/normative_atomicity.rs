//! Heuristic: a binding standard states one normative claim, not six.
//!
//! `normative-schema` proves the keys are well formed. It cannot see that
//! the body says three different things, because nothing in the schema is
//! about the prose. That gap matters: a contract returns one verdict per
//! standard, so a page binding three claims produces a verdict nobody can
//! read. Which of the three failed?
//!
//! Counting claims is a judgment call, and this rule does not pretend
//! otherwise. It counts *blocks that state something normative* rather
//! than sentences, because the failure it is looking for is a page that
//! enumerates rules, and an enumeration is shaped like a list or like
//! separate paragraphs. One claim argued across four sentences of the same
//! paragraph is one claim, and must not trip this.
//!
//! Heuristic, never structural. Everything left here after
//! `normative-schema` took the all-or-nothing half is a reading of prose,
//! and a reading of prose that blocks a contributor works against the
//! low-ceremony-to-contribute invariant. It annotates; `LintReport` never
//! fails a run on it.
//!
//! The ceiling is [`crate::config::LintConfig::max_normative_claims`],
//! default 1. A corpus mid-atomicity-refactor raises it to see the worst
//! offenders first rather than every page at once.

use super::{LintedFile, Rule, violation};
use crate::lint::{Severity, Violation};
use crate::normative;

/// The words that make a block a claim. Deliberately short and deliberately
/// not configurable: a longer list buys precision this rule cannot use,
/// because its output is a nudge and its threshold is already adjustable.
const CLAIM_MARKERS: [&str; 10] = [
    "must",
    "shall",
    "never",
    "always",
    "should",
    "required",
    "forbidden",
    "prohibited",
    "do not",
    "don't",
];

pub(crate) struct NormativeAtomicityRule {
    max_claims: usize,
}

impl NormativeAtomicityRule {
    pub(crate) fn new(max_claims: usize) -> Self {
        NormativeAtomicityRule { max_claims }
    }
}

impl Rule for NormativeAtomicityRule {
    fn id(&self) -> &'static str {
        "normative-atomicity"
    }

    fn severity(&self) -> Severity {
        Severity::Heuristic
    }

    fn check(&self, file: &LintedFile) -> Vec<Violation> {
        // A page whose frontmatter did not parse, or which is not a binding
        // standard at all, is not this rule's business. `binding` by
        // omission is the whole corpus, and atomicity is only owed by a
        // page that will be compiled into a contract.
        let Ok(frontmatter) = &file.frontmatter else {
            return Vec::new();
        };
        let Ok(Some(_)) = normative::parse(frontmatter, &file.body) else {
            return Vec::new();
        };

        let claims = count_claims(&file.body);
        if claims <= self.max_claims {
            return Vec::new();
        }

        vec![violation(
            self,
            &file.repo_relative_path,
            format!(
                "a binding page states {claims} normative claims, and the ceiling is \
                 {}; one page, one claim, one verdict, so split it",
                self.max_claims
            ),
        )]
    }
}

/// How many blocks of `body` state something normative.
fn count_claims(body: &str) -> usize {
    blocks(body).iter().filter(|b| states_a_claim(b)).count()
}

/// Split `body` into the units a claim is counted in.
///
/// A block ends at a blank line, at a heading, or at the start of a list
/// item. List items split because an enumerated rule set is exactly what
/// this rule exists to find, and headings are dropped entirely because a
/// heading names the claim stated below it, which would count it twice.
/// Fenced code is dropped because a `kaibo-checks` block full of patterns
/// is not prose and a sample command saying "you must" is not a claim.
fn blocks(body: &str) -> Vec<String> {
    let mut finished: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    let mut open: Option<(char, usize)> = None;

    for line in body.lines() {
        let trimmed = line.trim_start();

        if let Some((ch, len)) = fence(trimmed) {
            match open {
                Some((open_ch, open_len)) if ch == open_ch && len >= open_len => open = None,
                Some(_) => {}
                None => {
                    flush(&mut finished, &mut current);
                    open = Some((ch, len));
                }
            }
            continue;
        }
        if open.is_some() {
            continue;
        }

        if trimmed.is_empty() || trimmed.starts_with('#') || starts_a_list_item(trimmed) {
            flush(&mut finished, &mut current);
        }
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        current.push(trimmed);
    }

    flush(&mut finished, &mut current);
    finished
}

fn flush(finished: &mut Vec<String>, current: &mut Vec<&str>) {
    if !current.is_empty() {
        finished.push(current.join(" "));
        current.clear();
    }
}

/// A CommonMark fence opener: three or more of the same backtick or tilde.
/// Same shape as the scanner in [`crate::normative`], and for the same
/// reason: a page documenting the schema must not be read as one.
fn fence(line: &str) -> Option<(char, usize)> {
    let ch = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = line.chars().take_while(|c| *c == ch).count();
    (len >= 3).then_some((ch, len))
}

fn starts_a_list_item(line: &str) -> bool {
    let rest = match line.chars().next() {
        Some('-') | Some('*') | Some('+') => &line[1..],
        Some(c) if c.is_ascii_digit() => {
            let digits = line.chars().take_while(char::is_ascii_digit).count();
            match line[digits..].chars().next() {
                Some('.') | Some(')') => &line[digits + 1..],
                _ => return false,
            }
        }
        _ => return false,
    };
    rest.starts_with(' ') || rest.is_empty()
}

fn states_a_claim(block: &str) -> bool {
    let lowered = block.to_lowercase();
    CLAIM_MARKERS
        .iter()
        .any(|marker| contains_word(&lowered, marker))
}

/// `contains`, but refusing a match inside a longer word, so "mustard" is
/// not a claim and "must." is.
fn contains_word(haystack: &str, needle: &str) -> bool {
    let bytes = haystack.as_bytes();
    let mut from = 0;
    while let Some(offset) = haystack[from..].find(needle) {
        let start = from + offset;
        let end = start + needle.len();
        let opens = start == 0 || !bytes[start - 1].is_ascii_alphanumeric();
        let closes = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if opens && closes {
            return true;
        }
        from = start + 1;
    }
    false
}

#[cfg(test)]
mod tests;
