//! Structural: every frontmatter tag matches `lint.tags_kebab_case.pattern`
//! in full. The compiled default pattern is lowercase ASCII letters and
//! digits, hyphen-separated, no leading, trailing or doubled hyphen - what
//! this rule has always called "kebab-case".
//!
//! The pattern is a field this rule is constructed with
//! ([`super::registry`] compiles it once from [`crate::config::LintConfig`]),
//! never something read from the file being checked.

use regex::Regex;

use super::{LintedFile, Rule, violation};
use crate::lint::{Severity, Violation};
use crate::trust;

pub(crate) struct TagsKebabCaseRule {
    pattern: Regex,
}

impl TagsKebabCaseRule {
    pub(crate) fn new(pattern: Regex) -> Self {
        Self { pattern }
    }
}

#[cfg(test)]
impl Default for TagsKebabCaseRule {
    /// Compiled from [`crate::config::LintConfig::default`]'s
    /// `tag_pattern`, the same anchoring [`super::registry`] applies - a
    /// test using this exercises exactly what a caller who configures
    /// nothing gets.
    fn default() -> Self {
        let config = crate::config::LintConfig::default();
        Self::new(
            Regex::new(&format!("^(?:{})$", config.tag_pattern))
                .expect("the compiled default tag pattern is a fixed, known-valid regex literal"),
        )
    }
}

impl Rule for TagsKebabCaseRule {
    fn id(&self) -> &'static str {
        "tags-kebab-case"
    }

    fn severity(&self) -> Severity {
        Severity::Structural
    }

    fn check(&self, file: &LintedFile) -> Vec<Violation> {
        let Ok(fm) = &file.frontmatter else {
            return Vec::new();
        };
        let Some(tags) = &fm.tags else {
            return Vec::new();
        };

        tags.iter()
            .filter(|tag| !self.pattern.is_match(tag))
            .map(|tag| {
                // `tag` is corpus content; stripped before it reaches this
                // rule's own output so an embedded control character in a
                // tag cannot forge an extra line of it, exactly the
                // forgery `moc::build_section` already defends its own
                // bullet values against.
                let safe_tag = trust::strip_control_chars(tag);
                violation(
                    self,
                    &file.repo_relative_path,
                    format!("tag {safe_tag:?} does not match the configured tag pattern"),
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::Frontmatter;

    fn file_with_tags(tags: Vec<&str>) -> LintedFile {
        LintedFile {
            repo_relative_path: "kaibo/reference/page.md".to_string(),
            frontmatter: Ok(Frontmatter {
                tags: Some(tags.into_iter().map(str::to_string).collect()),
                ..Default::default()
            }),
            body: String::new(),
        }
    }

    #[test]
    fn the_rules_id_is_tags_kebab_case() {
        assert_eq!(TagsKebabCaseRule::default().id(), "tags-kebab-case");
    }

    #[test]
    fn all_kebab_case_tags_produce_no_violations() {
        let f = file_with_tags(vec!["knowledge-management", "cli", "rule-registry-v2"]);
        assert_eq!(TagsKebabCaseRule::default().check(&f), Vec::new());
    }

    #[test]
    fn a_camel_case_tag_is_reported() {
        let f = file_with_tags(vec!["camelCaseTag"]);
        let violations = TagsKebabCaseRule::default().check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("camelCaseTag"));
    }

    #[test]
    fn a_tag_with_an_underscore_is_reported() {
        let f = file_with_tags(vec!["rule_registry"]);
        assert_eq!(TagsKebabCaseRule::default().check(&f).len(), 1);
    }

    #[test]
    fn a_tag_with_a_doubled_hyphen_is_reported() {
        let f = file_with_tags(vec!["rule--registry"]);
        assert_eq!(TagsKebabCaseRule::default().check(&f).len(), 1);
    }

    #[test]
    fn a_tag_with_a_leading_hyphen_is_reported() {
        let f = file_with_tags(vec!["-rule"]);
        assert_eq!(TagsKebabCaseRule::default().check(&f).len(), 1);
    }

    #[test]
    fn no_tags_field_produces_no_violations_here_frontmatter_contract_owns_that_gap() {
        let f = LintedFile {
            repo_relative_path: "kaibo/reference/page.md".to_string(),
            frontmatter: Ok(Frontmatter::default()),
            body: String::new(),
        };
        assert_eq!(TagsKebabCaseRule::default().check(&f), Vec::new());
    }

    #[test]
    fn a_control_character_embedded_in_a_tag_is_stripped_from_the_reported_message() {
        let f = file_with_tags(vec!["forged\rinjected: line"]);
        let violations = TagsKebabCaseRule::default().check(&f);
        assert_eq!(violations.len(), 1);
        assert!(!violations[0].message.contains('\r'));
    }

    #[test]
    fn a_malformed_frontmatter_block_produces_no_violations_here_frontmatter_contract_owns_that() {
        let f = LintedFile {
            repo_relative_path: "kaibo/reference/page.md".to_string(),
            frontmatter: Err("unterminated".to_string()),
            body: String::new(),
        };
        assert_eq!(TagsKebabCaseRule::default().check(&f), Vec::new());
    }

    // --- configurable: pattern -----------------------------------------

    #[test]
    fn a_configured_pattern_accepts_tags_the_default_kebab_case_pattern_would_reject() {
        // snake_case, not kebab-case - rejected by the compiled default,
        // accepted once the pattern is configured to allow underscores.
        let rule = TagsKebabCaseRule::new(Regex::new(r"^(?:[a-z0-9_]+)$").unwrap());
        let f = file_with_tags(vec!["rule_registry"]);
        assert_eq!(rule.check(&f), Vec::new());
    }

    #[test]
    fn a_configured_pattern_rejects_tags_the_default_kebab_case_pattern_would_accept() {
        // The configured pattern requires at least one digit; an
        // all-letters tag that the default pattern accepts is rejected.
        let rule = TagsKebabCaseRule::new(Regex::new(r"^(?:[a-z]*[0-9]+[a-z0-9-]*)$").unwrap());
        let f = file_with_tags(vec!["no-digits-here"]);
        assert_eq!(rule.check(&f).len(), 1);
    }

    #[test]
    fn the_rule_matches_whatever_anchoring_the_compiled_pattern_carries() {
        // `super::registry` is what wraps a configured pattern as
        // `^(?:pattern)$` before building this rule (see its own tests);
        // this rule just runs whatever `Regex` it's handed. An anchored
        // pattern here rejects a tag that only partially matches it.
        let rule = TagsKebabCaseRule::new(Regex::new(r"^(?:[a-z]+)$").unwrap());
        let f = file_with_tags(vec!["9abc"]);
        assert_eq!(rule.check(&f).len(), 1);
    }
}
