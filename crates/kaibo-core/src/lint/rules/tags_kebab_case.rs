//! Structural: every frontmatter tag is kebab-case (lowercase ASCII
//! letters and digits, hyphen-separated, no leading, trailing or doubled
//! hyphen).

use super::{LintedFile, Rule, violation};
use crate::lint::{Severity, Violation};
use crate::trust;

pub(crate) struct TagsKebabCaseRule;

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
            .filter(|tag| !is_kebab_case(tag))
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
                    format!("tag {safe_tag:?} is not kebab-case"),
                )
            })
            .collect()
    }
}

fn is_kebab_case(tag: &str) -> bool {
    if tag.is_empty() {
        return false;
    }
    if tag.starts_with('-') || tag.ends_with('-') || tag.contains("--") {
        return false;
    }
    tag.chars()
        .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-')
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
        assert_eq!(TagsKebabCaseRule.id(), "tags-kebab-case");
    }

    #[test]
    fn all_kebab_case_tags_produce_no_violations() {
        let f = file_with_tags(vec!["knowledge-management", "cli", "rule-registry-v2"]);
        assert_eq!(TagsKebabCaseRule.check(&f), Vec::new());
    }

    #[test]
    fn a_camel_case_tag_is_reported() {
        let f = file_with_tags(vec!["camelCaseTag"]);
        let violations = TagsKebabCaseRule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("camelCaseTag"));
    }

    #[test]
    fn a_tag_with_an_underscore_is_reported() {
        let f = file_with_tags(vec!["rule_registry"]);
        assert_eq!(TagsKebabCaseRule.check(&f).len(), 1);
    }

    #[test]
    fn a_tag_with_a_doubled_hyphen_is_reported() {
        let f = file_with_tags(vec!["rule--registry"]);
        assert_eq!(TagsKebabCaseRule.check(&f).len(), 1);
    }

    #[test]
    fn a_tag_with_a_leading_hyphen_is_reported() {
        let f = file_with_tags(vec!["-rule"]);
        assert_eq!(TagsKebabCaseRule.check(&f).len(), 1);
    }

    #[test]
    fn no_tags_field_produces_no_violations_here_frontmatter_contract_owns_that_gap() {
        let f = LintedFile {
            repo_relative_path: "kaibo/reference/page.md".to_string(),
            frontmatter: Ok(Frontmatter::default()),
            body: String::new(),
        };
        assert_eq!(TagsKebabCaseRule.check(&f), Vec::new());
    }

    #[test]
    fn a_control_character_embedded_in_a_tag_is_stripped_from_the_reported_message() {
        let f = file_with_tags(vec!["forged\rinjected: line"]);
        let violations = TagsKebabCaseRule.check(&f);
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
        assert_eq!(TagsKebabCaseRule.check(&f), Vec::new());
    }
}
