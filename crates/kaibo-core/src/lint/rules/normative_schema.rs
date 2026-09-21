//! Structural: a page that declares anything normative declares all of it,
//! and every part of it parses.
//!
//! The whole rule is [`crate::normative::parse`] - the schema lives there,
//! this is the gate that runs it over the corpus. That is deliberate: a
//! second, looser copy of the schema living in a lint rule is how a page
//! comes to pass CI and then fail the thing that actually reads it.
//!
//! **Structural, not heuristic.** Every violation here is a shape a reader
//! cannot resolve - a `severity` nobody binds, a pattern that does not
//! compile, two blocks where the schema allows one. None of them is a
//! judgment call about a contributor's writing, which is what the house
//! policy reserves heuristic severity for.
//!
//! What this rule deliberately does **not** check: whether a binding page
//! states exactly one normative claim. That is atomicity, it is a heuristic
//! over prose, and it is [#30](https://github.com/tenex-hq/kaibo/issues/30).

use super::{LintedFile, Rule, violation};
use crate::lint::{Severity, Violation};
use crate::normative;

pub(crate) struct NormativeSchemaRule;

impl Rule for NormativeSchemaRule {
    fn id(&self) -> &'static str {
        "normative-schema"
    }

    fn severity(&self) -> Severity {
        Severity::Structural
    }

    fn check(&self, file: &LintedFile) -> Vec<Violation> {
        // A frontmatter block that did not parse is already one rule's
        // violation; it is not also this one's.
        let Ok(fm) = &file.frontmatter else {
            return Vec::new();
        };

        match normative::parse(fm, &file.body) {
            Ok(_) => Vec::new(),
            Err(err) => vec![violation(self, &file.repo_relative_path, err.to_string())],
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn file(frontmatter_yaml: &str, body: &str) -> LintedFile {
        let doc = crate::frontmatter::parse(&format!("---\n{frontmatter_yaml}\n---\n{body}\n"))
            .expect("the fixture's frontmatter block is well formed");
        LintedFile {
            repo_relative_path: "kaibo/reference/a-standard.md".to_string(),
            frontmatter: Ok(doc.frontmatter),
            body: doc.body,
        }
    }

    const BINDING_KEYS: &str = "\
type: reference
title: A standard
status: current
binding: true
severity: must
applies_to:
  actions: [file-edit]";

    #[test]
    fn a_page_declaring_nothing_normative_is_not_this_rules_business() {
        let violations = NormativeSchemaRule.check(&file(
            "type: reference\ntitle: A page\nstatus: current",
            "Prose.\n",
        ));
        assert!(violations.is_empty());
    }

    #[test]
    fn a_complete_standard_passes() {
        let violations = NormativeSchemaRule.check(&file(
            BINDING_KEYS,
            "```json kaibo-checks\n[{\"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"TODO\"}]\n```\n",
        ));
        assert!(violations.is_empty(), "{violations:?}");
    }

    #[test]
    fn a_half_declared_standard_fails_the_build_with_the_schemas_own_message() {
        let violations =
            NormativeSchemaRule.check(&file("type: reference\nseverity: must", "Prose.\n"));

        assert_eq!(violations.len(), 1);
        assert_eq!(violations[0].rule_id, "normative-schema");
        assert_eq!(violations[0].severity, Severity::Structural);
        assert_eq!(violations[0].path, "kaibo/reference/a-standard.md");
        assert!(violations[0].message.contains("binding: true"));
    }

    #[test]
    fn a_pattern_that_does_not_compile_is_caught_in_the_corpuss_own_ci() {
        let violations = NormativeSchemaRule.check(&file(
            BINDING_KEYS,
            "```json kaibo-checks\n[{\"id\": \"a\", \"kind\": \"forbid_regex\", \"pattern\": \"[unclosed\"}]\n```\n",
        ));

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("[unclosed"));
    }

    #[test]
    fn a_judgment_check_fails_rather_than_being_ignored() {
        let violations = NormativeSchemaRule.check(&file(
            BINDING_KEYS,
            "```json kaibo-checks\n[{\"id\": \"a\", \"kind\": \"judgment\", \"criterion\": \"taste\"}]\n```\n",
        ));

        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("judgment"));
    }

    #[test]
    fn a_page_whose_frontmatter_did_not_parse_is_left_to_the_rule_that_reports_that() {
        // Reporting the same broken block twice, under two rule ids, tells a
        // contributor there are two problems.
        let broken = LintedFile {
            repo_relative_path: "kaibo/reference/broken.md".to_string(),
            frontmatter: Err("invalid YAML".to_string()),
            body: "```json kaibo-checks\n[]\n```\n".to_string(),
        };

        assert!(NormativeSchemaRule.check(&broken).is_empty());
    }

    #[test]
    fn a_control_character_in_a_rejected_value_never_reaches_the_violation() {
        let violations = NormativeSchemaRule.check(&file(
            "type: reference\nbinding: true\nseverity: \"hi\\rgh\"\napplies_to:\n  actions: [file-edit]",
            "Prose.\n",
        ));

        assert_eq!(violations.len(), 1);
        assert!(!violations[0].message.contains('\r'));
    }
}
