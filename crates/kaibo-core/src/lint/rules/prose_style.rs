//! Heuristic: no em dash, no en dash, no `--` used as punctuation, in the
//! document body. Scope is the whole file, matching the house style this
//! mirrors (see the project's own writing rules) - a contributor who
//! introduced one three paragraphs above the diff still trips this, the
//! same "whole file, not the changed lines" behaviour the corpus already
//! expects.
//!
//! Heuristic, not structural: a prose preference must not block a
//! contribution the way a missing required field does.

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

        if file.body.contains(EM_DASH) {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "body contains an em dash (U+2014); use a plain hyphen instead",
            ));
        }
        if file.body.contains(EN_DASH) {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "body contains an en dash (U+2013); use a plain hyphen instead",
            ));
        }
        if file.body.contains("--") {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "body uses `--` as punctuation; use a plain hyphen instead",
            ));
        }

        out
    }
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
}
