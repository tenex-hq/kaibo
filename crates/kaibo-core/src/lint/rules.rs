//! Rule registry: each rule declares an id, a severity policy, and a check
//! against one already-parsed file. Adding a rule means adding a file
//! under this module plus one line in [`registry`] - there is no match arm
//! on rule id anywhere in this crate for a rule to fall out of.
//!
//! Rules stay compiled: what a config file can change is *which* rules run
//! ([`crate::config::LintConfig::disabled_rules`]) and *how they're
//! parameterised* (the other four [`crate::config::LintConfig`] fields),
//! never *what code runs*. [`registry`] builds every rule from that struct
//! and nothing else - no rule constructor here reads a path, an argument, or
//! anything parsed out of a page. That is deliberate and covered by
//! `crate::lint::tests::hostile_frontmatter_cannot_change_which_rules_run_or_how`:
//! a page in the corpus clone can put a `lint:` table in its own
//! frontmatter, but nothing under `crate::lint` ever looks at frontmatter
//! *before* the registry is built, so there is nothing for that table to
//! reach.

use regex::Regex;

use crate::config::LintConfig;
use crate::frontmatter::Frontmatter;
use crate::lint::{Severity, Violation};

mod frontmatter_contract;
mod normative_schema;
mod prose_style;
mod tags_kebab_case;

/// One markdown file, already read and parsed, handed to every rule.
///
/// `frontmatter` is `Err` when the frontmatter block itself failed to
/// parse - a page-level structural defect a rule reports as data, not
/// something this module papers over as an empty, well-formed block.
pub(crate) struct LintedFile {
    pub repo_relative_path: String,
    pub frontmatter: Result<Frontmatter, String>,
    pub body: String,
}

pub(crate) trait Rule {
    fn id(&self) -> &'static str;
    fn severity(&self) -> Severity;
    fn check(&self, file: &LintedFile) -> Vec<Violation>;
}

/// Build a [`Violation`] tagged with `rule`'s own id and severity, so a
/// rule implementation never has to restate either.
pub(crate) fn violation(rule: &dyn Rule, path: &str, message: impl Into<String>) -> Violation {
    Violation {
        rule_id: rule.id().to_string(),
        severity: rule.severity(),
        path: path.to_string(),
        message: message.into(),
    }
}

/// Build the registry from `config`, minus any rule named in
/// `config.disabled_rules`. The only failure mode is a `tag_pattern` that
/// doesn't compile as a regex - a config mistake, reported by name rather
/// than panicking or silently falling back to the compiled default.
pub(crate) fn registry(config: &LintConfig) -> Result<Vec<Box<dyn Rule>>, String> {
    let tag_pattern = Regex::new(&format!("^(?:{})$", config.tag_pattern)).map_err(|err| {
        format!(
            "lint.tags_kebab_case.pattern {:?} is not a valid regex: {err}",
            config.tag_pattern
        )
    })?;

    let mut rules: Vec<Box<dyn Rule>> = vec![
        Box::new(frontmatter_contract::FrontmatterContractRule::new(
            config.required_frontmatter_keys.clone(),
            config.allowed_status.clone(),
            config.type_folder_overrides.clone(),
        )),
        Box::new(tags_kebab_case::TagsKebabCaseRule::new(tag_pattern)),
        Box::new(normative_schema::NormativeSchemaRule),
        Box::new(prose_style::ProseStyleRule),
    ];

    rules.retain(|rule| !config.disabled_rules.iter().any(|id| id == rule.id()));

    Ok(rules)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_config_builds_every_compiled_rule() {
        let rules = registry(&LintConfig::default()).unwrap();
        let ids: Vec<&str> = rules.iter().map(|r| r.id()).collect();
        assert_eq!(
            ids,
            vec![
                "frontmatter-contract",
                "tags-kebab-case",
                "normative-schema",
                "prose-style"
            ]
        );
    }

    #[test]
    fn a_disabled_rule_id_is_absent_from_the_registry() {
        let config = LintConfig {
            disabled_rules: vec!["prose-style".to_string()],
            ..LintConfig::default()
        };
        let rules = registry(&config).unwrap();
        assert!(!rules.iter().any(|r| r.id() == "prose-style"));
        assert_eq!(rules.len(), 3);
    }

    #[test]
    fn disabling_every_rule_leaves_an_empty_but_valid_registry() {
        let config = LintConfig {
            disabled_rules: vec![
                "frontmatter-contract".to_string(),
                "tags-kebab-case".to_string(),
                "normative-schema".to_string(),
                "prose-style".to_string(),
            ],
            ..LintConfig::default()
        };
        let rules = registry(&config).unwrap();
        assert!(rules.is_empty());
    }

    #[test]
    fn an_unrecognised_disabled_rule_id_is_silently_ignored_not_an_error() {
        // Naming a rule that doesn't exist (a typo, a rule renamed since)
        // is not grounds to fail every lint run - `retain` just never
        // matches it.
        let config = LintConfig {
            disabled_rules: vec!["no-such-rule".to_string()],
            ..LintConfig::default()
        };
        let rules = registry(&config).unwrap();
        assert_eq!(rules.len(), 4);
    }

    #[test]
    fn an_invalid_tag_pattern_regex_fails_the_registry_by_name_not_a_panic() {
        let config = LintConfig {
            tag_pattern: "[unclosed".to_string(),
            ..LintConfig::default()
        };
        let Err(err) = registry(&config) else {
            panic!("expected an invalid tag_pattern to fail the registry");
        };
        assert!(err.contains("[unclosed"));
    }

    #[test]
    fn the_configured_tag_pattern_is_anchored_to_the_whole_tag_before_the_rule_sees_it() {
        // Deliberately a substring-only pattern: unanchored, "[a-z]+" would
        // accept any tag containing a lowercase run. `registry` wraps it as
        // `^(?:pattern)$`, so a tag with characters outside the pattern is
        // still rejected.
        let config = LintConfig {
            tag_pattern: "[a-z]+".to_string(),
            ..LintConfig::default()
        };
        let rules = registry(&config).unwrap();
        let tags_rule = rules.iter().find(|r| r.id() == "tags-kebab-case").unwrap();

        let file = LintedFile {
            repo_relative_path: "kaibo/reference/page.md".to_string(),
            frontmatter: Ok(Frontmatter {
                tags: Some(vec!["9abc".to_string()]),
                ..Default::default()
            }),
            body: String::new(),
        };
        assert_eq!(tags_rule.check(&file).len(), 1);
    }
}
