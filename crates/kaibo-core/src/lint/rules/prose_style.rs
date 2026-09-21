//! Heuristic: no em dash, no en dash, no `--` used as punctuation, in the
//! document body. Scope is the whole file, matching the house style this
//! mirrors (see the project's own writing rules) - a contributor who
//! introduced one three paragraphs above the diff still trips this, the
//! same "whole file, not the changed lines" behaviour the corpus already
//! expects.
//!
//! Scoped to prose, not to the raw markdown source: a fenced code block
//! (``` or ~~~ delimited, info string included), an inline code span
//! (backtick delimited, any number of backticks), an HTML comment, and a
//! table delimiter row (`|---|---|`) are all stripped before any of the
//! three checks run. A command-line flag or a quoted dash inside a code
//! sample is the documented syntax of whatever it demonstrates, a table's
//! own row of dashes is structure rather than a sentence, and a comment is
//! not published prose at all - none of the three checks owns an opinion
//! on any of them, so all three get the same treatment rather than one
//! check skipping markdown syntax while the others still flag it.
//!
//! Heuristic, not structural: a prose preference must not block a
//! contribution the way a missing required field does.
//!
//! This is the rule a downstream user is most likely to turn off entirely
//! via `lint.disabled_rules = ["prose-style"]` (see
//! [`crate::config::LintConfig::disabled_rules`]). It has no parameters of
//! its own to reconfigure - the other three rules exist because a corpus's
//! structural contract genuinely varies (different required fields,
//! different status vocabulary, different folder names); this one doesn't,
//! because a house dash convention isn't a fact about what the corpus needs
//! to stay queryable, it's a preference about how this particular team
//! writes. A corpus with its own prose convention doesn't need a different
//! `prose-style` parameter, it needs `prose-style` off.

use super::{LintedFile, Rule, violation};
use crate::lint::{Severity, Violation};

const EM_DASH: char = '\u{2014}';
const EN_DASH: char = '\u{2013}';

pub(crate) struct ProseStyleRule;

impl Rule for ProseStyleRule {
    fn id(&self) -> &'static str {
        "prose-style"
    }

    fn severity(&self) -> Severity {
        Severity::Heuristic
    }

    fn check(&self, file: &LintedFile) -> Vec<Violation> {
        let mut out = Vec::new();
        let prose = prose_only(&file.body);

        if prose.contains(EM_DASH) {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "body contains an em dash (U+2014); use a plain hyphen instead",
            ));
        }
        if prose.contains(EN_DASH) {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "body contains an en dash (U+2013); use a plain hyphen instead",
            ));
        }
        if prose.contains("--") {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "body uses `--` as punctuation; use a plain hyphen instead",
            ));
        }

        out
    }
}

/// Strip fenced code blocks, table delimiter rows, HTML comments and
/// inline code spans, leaving only what a reader would call prose. A
/// command-line flag or a quoted dash inside a code sample is quoted
/// content, a table's row of dashes is structure, and a comment is not
/// published prose at all - none of it is this team's writing.
fn prose_only(body: &str) -> String {
    let without_blocks_and_rows = strip_non_prose_lines(body);
    let without_comments = strip_html_comments(&without_blocks_and_rows);
    strip_inline_code_spans(&without_comments)
}

/// Drop every line inside a ``` or ~~~ fenced code block (the fence
/// delimiters themselves included) and every table delimiter row, such as
/// `|---|---|`. The fence scanner is the same shape as
/// [`super::normative_atomicity`]'s, for the same reason: it does not
/// bother checking that a closing fence carries no trailing text, because
/// this is a heuristic annotation, not the structural parser in
/// [`crate::normative`] that a corpus's own CI depends on.
fn strip_non_prose_lines(body: &str) -> String {
    let mut out = String::new();
    let mut open: Option<(char, usize)> = None;

    for line in body.lines() {
        let trimmed = line.trim_start();

        if let Some((ch, len)) = fence_marker(trimmed) {
            match open {
                Some((open_ch, open_len)) if ch == open_ch && len >= open_len => open = None,
                Some(_) => {}
                None => open = Some((ch, len)),
            }
            continue;
        }
        if open.is_some() {
            continue;
        }
        if is_table_delimiter_row(trimmed) {
            continue;
        }

        out.push_str(line);
        out.push('\n');
    }

    out
}

/// A CommonMark fence opener or closer: three or more of the same backtick
/// or tilde, whatever else is on the line.
fn fence_marker(line: &str) -> Option<(char, usize)> {
    let ch = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = line.chars().take_while(|c| *c == ch).count();
    (len >= 3).then_some((ch, len))
}

/// A GFM table delimiter row: pipe-separated cells that are each nothing
/// but dashes with optional leading/trailing alignment colons, e.g.
/// `|---|---|` or `| :--- | ---: |`. Requires at least one pipe, so a bare
/// `---` (a thematic break or a setext heading underline) is left alone -
/// this rule is not the place to adjudicate either of those.
fn is_table_delimiter_row(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.contains('|') {
        return false;
    }
    let cells: Vec<&str> = trimmed
        .trim_start_matches('|')
        .trim_end_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    !cells.is_empty() && cells.iter().all(|cell| is_delimiter_cell(cell))
}

fn is_delimiter_cell(cell: &str) -> bool {
    let inner = cell.trim_start_matches(':').trim_end_matches(':');
    !inner.is_empty() && inner.chars().all(|c| c == '-')
}

/// Remove `<!-- ... -->` HTML comments, which may span multiple lines. An
/// unterminated `<!--` drops everything from there to the end of the body:
/// CommonMark treats the rest of the document as inside the comment too, so
/// there is no well-formed prose left to check past that point.
fn strip_html_comments(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;

    while let Some(start) = rest.find("<!--") {
        out.push_str(&rest[..start]);
        match rest[start + 4..].find("-->") {
            Some(rel_end) => {
                let end = start + 4 + rel_end + 3;
                rest = &rest[end..];
            }
            None => {
                rest = "";
            }
        }
    }
    out.push_str(rest);

    out
}

/// Remove backtick-delimited inline code spans: any run of backticks opens
/// one, and only the next run of exactly the same length closes it,
/// mirroring CommonMark's rule that a longer or shorter run does not
/// match. A run with no matching close later in the text is not a code
/// span and is left in place as literal backticks.
fn strip_inline_code_spans(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::new();
    let mut i = 0;

    while i < chars.len() {
        if chars[i] != '`' {
            out.push(chars[i]);
            i += 1;
            continue;
        }

        let open_start = i;
        let mut j = i;
        while j < chars.len() && chars[j] == '`' {
            j += 1;
        }
        let open_len = j - open_start;

        let mut k = j;
        let mut close: Option<usize> = None;
        while k < chars.len() {
            if chars[k] == '`' {
                let close_start = k;
                let mut m = k;
                while m < chars.len() && chars[m] == '`' {
                    m += 1;
                }
                if m - close_start == open_len {
                    close = Some(m);
                    break;
                }
                k = m;
            } else {
                k += 1;
            }
        }

        match close {
            Some(end) => i = end,
            None => {
                out.extend(&chars[open_start..j]);
                i = j;
            }
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::Frontmatter;

    fn file_with_body(body: &str) -> LintedFile {
        LintedFile {
            repo_relative_path: "kaibo/reference/page.md".to_string(),
            frontmatter: Ok(Frontmatter::default()),
            body: body.to_string(),
        }
    }

    #[test]
    fn the_rules_id_is_prose_style() {
        assert_eq!(ProseStyleRule.id(), "prose-style");
    }

    #[test]
    fn plain_hyphenated_prose_has_no_violations() {
        let f = file_with_body("A well-formed sentence - with a hyphen aside.");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn an_em_dash_in_the_body_is_reported() {
        let f = file_with_body("A sentence \u{2014} with an em dash aside.");
        let violations = ProseStyleRule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("em dash"));
    }

    #[test]
    fn an_en_dash_in_the_body_is_reported() {
        let f = file_with_body("Pages 12\u{2013}14 cover this.");
        let violations = ProseStyleRule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("en dash"));
    }

    #[test]
    fn a_double_hyphen_used_as_punctuation_is_reported() {
        let f = file_with_body("A sentence -- used as punctuation.");
        let violations = ProseStyleRule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("`--`"));
    }

    #[test]
    fn every_violation_from_this_rule_is_heuristic_severity_so_it_never_blocks() {
        let f = file_with_body("An em dash \u{2014} and a double hyphen -- together.");
        let violations = ProseStyleRule.check(&f);
        assert_eq!(violations.len(), 2);
        for v in violations {
            assert_eq!(v.severity, Severity::Heuristic);
        }
    }

    #[test]
    fn an_em_dash_inside_a_fenced_code_block_is_not_reported() {
        let f = file_with_body("Example:\n\n```\nA sentence \u{2014} inside a code sample.\n```\n");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn an_en_dash_inside_a_fenced_code_block_is_not_reported() {
        let f = file_with_body("```\nPages 12\u{2013}14 cover this.\n```\n");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn a_double_hyphen_inside_a_fenced_code_block_is_not_reported() {
        let f = file_with_body("Run it:\n\n```sh\nkaibo query --index prose --format json\n```\n");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn a_fenced_code_block_delimited_by_tildes_is_also_skipped() {
        let f = file_with_body("~~~\nkaibo query --index prose\n~~~\n");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn a_double_hyphen_inside_an_inline_code_span_is_not_reported() {
        let f = file_with_body("Pass `--locked` on every cargo invocation.");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn a_double_hyphen_inside_a_multi_backtick_code_span_is_not_reported() {
        let f = file_with_body("Use ``--flag`` today.");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn a_double_hyphen_in_prose_after_a_fenced_code_block_is_still_reported() {
        let f = file_with_body(
            "```\nkaibo query --index prose\n```\n\nAnd another thought -- as an aside.",
        );
        let violations = ProseStyleRule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("`--`"));
    }

    #[test]
    fn a_double_hyphen_inside_a_single_line_html_comment_is_not_reported() {
        let f = file_with_body(
            "Visible text.\n\n<!-- TODO: revisit this -- later -->\n\nMore visible text.",
        );
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn an_em_dash_inside_a_multiline_html_comment_is_not_reported() {
        let f = file_with_body(
            "<!--\nDraft notes \u{2014} not for publication.\n-->\n\nPublished text.",
        );
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn an_unterminated_html_comment_drops_everything_after_it_rather_than_treating_it_as_prose() {
        let f = file_with_body("<!-- unterminated comment with -- inside it");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn a_table_delimiter_row_is_not_reported() {
        let f = file_with_body("| A | B |\n|---|---|\n| 1 | 2 |");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn a_table_delimiter_row_with_alignment_colons_is_not_reported() {
        let f = file_with_body("| A | B |\n|:---|---:|\n| 1 | 2 |");
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }

    #[test]
    fn a_double_hyphen_in_prose_next_to_a_table_is_still_reported() {
        let f = file_with_body(
            "| A | B |\n|---|---|\n| 1 | 2 |\n\nAnd a final thought -- worth noting.",
        );
        let violations = ProseStyleRule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("`--`"));
    }

    #[test]
    fn an_em_dash_used_to_document_the_character_inside_a_code_span_is_not_reported() {
        let f = file_with_body(
            "The pattern is written `\u{2014}` so a page about punctuation does not trip this rule.",
        );
        assert_eq!(ProseStyleRule.check(&f), Vec::new());
    }
}
