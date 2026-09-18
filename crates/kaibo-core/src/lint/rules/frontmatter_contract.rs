//! Structural: every key in `lint.frontmatter_contract.required_keys` is
//! present (default `title`, `tags`, `status`, `updated`), any configured
//! `allowed_status` allow-list is respected, and `type` matches the name of
//! the page's immediate containing folder, subject to
//! `type_folder_overrides`.
//!
//! Every parameter here is a field this rule is constructed with, never a
//! value read from the file being checked - see the module doc on
//! [`super`] for why that split is load-bearing rather than incidental.

use std::collections::BTreeMap;

use super::{LintedFile, Rule, violation};
use crate::frontmatter::Frontmatter;
use crate::lint::{Severity, Violation};
use crate::trust;

pub(crate) struct FrontmatterContractRule {
    required_keys: Vec<String>,
    allowed_status: Vec<String>,
    type_folder_overrides: BTreeMap<String, String>,
}

impl FrontmatterContractRule {
    pub(crate) fn new(
        required_keys: Vec<String>,
        allowed_status: Vec<String>,
        type_folder_overrides: BTreeMap<String, String>,
    ) -> Self {
        Self {
            required_keys,
            allowed_status,
            type_folder_overrides,
        }
    }
}

#[cfg(test)]
impl Default for FrontmatterContractRule {
    /// Built from [`crate::config::LintConfig::default`], not a second copy
    /// of the same literals - a test using this gets exactly what a caller
    /// who configures nothing gets, and can't drift from it by accident.
    fn default() -> Self {
        let config = crate::config::LintConfig::default();
        Self::new(
            config.required_frontmatter_keys,
            config.allowed_status,
            config.type_folder_overrides,
        )
    }
}

impl Rule for FrontmatterContractRule {
    fn id(&self) -> &'static str {
        "frontmatter-contract"
    }

    fn severity(&self) -> Severity {
        Severity::Structural
    }

    fn check(&self, file: &LintedFile) -> Vec<Violation> {
        let fm = match &file.frontmatter {
            Err(detail) => {
                // A YAML/date parse error can echo back a fragment of the
                // offending corpus text; stripped before it reaches this
                // rule's own output, same as any other corpus-derived
                // scalar this module reports.
                let detail = trust::strip_control_chars(detail);
                return vec![violation(
                    self,
                    &file.repo_relative_path,
                    format!("frontmatter block is malformed: {detail}"),
                )];
            }
            Ok(fm) => fm,
        };

        let mut out = Vec::new();

        for key in &self.required_keys {
            if let Some(message) = self.missing_key_violation(fm, key) {
                out.push(violation(self, &file.repo_relative_path, message));
            }
        }

        if !self.allowed_status.is_empty()
            && let Some(status) = &fm.status
        {
            let actual = status.as_str();
            if !self.allowed_status.iter().any(|allowed| allowed == actual) {
                // `actual` is corpus content; stripped before it reaches
                // this rule's own output, same treatment every other
                // corpus-derived scalar in this rule gets.
                let actual = trust::strip_control_chars(actual);
                out.push(violation(
                    self,
                    &file.repo_relative_path,
                    format!(
                        "frontmatter `status: {actual}` is not one of the allowed values: {}",
                        self.allowed_status.join(", ")
                    ),
                ));
            }
        }

        if let Some(expected) = self.expected_type(&file.repo_relative_path) {
            match fm.doc_type.as_deref() {
                Some(actual) if actual == expected => {}
                Some(actual) => {
                    // `actual` is corpus content (frontmatter written by
                    // whoever authored the page); stripped before it
                    // reaches this rule's own output, same treatment
                    // `moc::build_section` gives an MOC bullet value.
                    let actual = trust::strip_control_chars(actual);
                    out.push(violation(
                        self,
                        &file.repo_relative_path,
                        format!(
                            "frontmatter `type: {actual}` does not match containing folder `{expected}`"
                        ),
                    ));
                }
                None => out.push(violation(
                    self,
                    &file.repo_relative_path,
                    format!("missing required frontmatter field `type` (expected `{expected}`)"),
                )),
            }
        }

        out
    }
}

impl FrontmatterContractRule {
    /// `None` when `key` is present; the violation message for `key` when
    /// it's missing. The four keys this rule has always known about keep
    /// their original, specific wording; anything else in
    /// `required_keys` - a downstream user's own addition - is checked
    /// generically against the frontmatter's passthrough map, with the same
    /// message shape.
    fn missing_key_violation(&self, fm: &Frontmatter, key: &str) -> Option<String> {
        let present = match key {
            "title" => !fm.title.as_deref().unwrap_or("").trim().is_empty(),
            "tags" => fm.tags.as_ref().is_some_and(|tags| !tags.is_empty()),
            "status" => fm.status.is_some(),
            "updated" => fm.updated.is_some(),
            "type" => fm.doc_type.is_some(),
            other => fm.extra.contains_key(other),
        };
        (!present).then(|| format!("missing required frontmatter field `{key}`"))
    }

    /// The expected `type` value for a page: the name of its immediate
    /// parent folder, when the page is nested at least one folder below the
    /// domain root (e.g. `kaibo/reference/foo.md` -> `reference`), remapped
    /// through `type_folder_overrides` when that folder is named there. A
    /// page directly under the domain root (e.g. `kaibo/_index.md`) has no
    /// such folder to check against and is exempt.
    fn expected_type<'a>(&'a self, repo_relative_path: &'a str) -> Option<&'a str> {
        let parts: Vec<&str> = repo_relative_path.split('/').collect();
        if parts.len() < 3 {
            return None;
        }
        let folder = parts[parts.len() - 2];
        Some(
            self.type_folder_overrides
                .get(folder)
                .map(String::as_str)
                .unwrap_or(folder),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::frontmatter::{Date, Frontmatter, Status};

    fn well_formed_frontmatter() -> Frontmatter {
        Frontmatter {
            doc_type: Some("reference".to_string()),
            title: Some("A title".to_string()),
            tags: Some(vec!["one".to_string()]),
            status: Some(Status::Current),
            updated: Some(Date {
                year: 2024,
                month: 1,
                day: 1,
            }),
            extra: Default::default(),
        }
    }

    fn file(repo_relative_path: &str, frontmatter: Frontmatter) -> LintedFile {
        LintedFile {
            repo_relative_path: repo_relative_path.to_string(),
            frontmatter: Ok(frontmatter),
            body: String::new(),
        }
    }

    #[test]
    fn the_rules_id_is_frontmatter_contract() {
        assert_eq!(
            FrontmatterContractRule::default().id(),
            "frontmatter-contract"
        );
    }

    #[test]
    fn a_page_with_every_required_field_and_a_matching_type_has_no_violations() {
        let f = file("kaibo/reference/page.md", well_formed_frontmatter());
        assert_eq!(FrontmatterContractRule::default().check(&f), Vec::new());
    }

    #[test]
    fn a_page_missing_title_tags_status_and_updated_reports_all_four() {
        let fm = crate::frontmatter::Frontmatter {
            doc_type: Some("reference".to_string()),
            ..Default::default()
        };
        let f = file("kaibo/reference/page.md", fm);
        let violations = FrontmatterContractRule::default().check(&f);
        let messages: Vec<&str> = violations.iter().map(|v| v.message.as_str()).collect();
        assert!(messages.iter().any(|m| m.contains("`title`")));
        assert!(messages.iter().any(|m| m.contains("`tags`")));
        assert!(messages.iter().any(|m| m.contains("`status`")));
        assert!(messages.iter().any(|m| m.contains("`updated`")));
    }

    #[test]
    fn empty_tags_list_is_treated_as_missing_not_present() {
        let mut fm = well_formed_frontmatter();
        fm.tags = Some(Vec::new());
        let f = file("kaibo/reference/page.md", fm);
        let violations = FrontmatterContractRule::default().check(&f);
        assert!(violations.iter().any(|v| v.message.contains("`tags`")));
    }

    #[test]
    fn a_type_that_does_not_match_the_containing_folder_is_reported() {
        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("how-to".to_string());
        let f = file("kaibo/reference/page.md", fm);
        let violations = FrontmatterContractRule::default().check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("reference"));
        assert!(violations[0].message.contains("how-to"));
    }

    #[test]
    fn the_expected_type_is_the_immediate_parent_folder_even_several_levels_deep() {
        // Five path segments, deep enough that "the immediate parent" and
        // "some other folder further up" resolve to different names -
        // `kaibo/a/b/reference/page.md`'s immediate parent is `reference`,
        // not `b`, which a path this shallow (three or four segments)
        // cannot distinguish since both would happen to name the same
        // folder either way.
        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("reference".to_string());
        let f = file("kaibo/a/b/reference/page.md", fm);
        assert_eq!(FrontmatterContractRule::default().check(&f), Vec::new());
    }

    #[test]
    fn a_page_directly_under_the_domain_root_is_exempt_from_the_type_check() {
        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("index".to_string());
        let f = file("_index.md", fm);
        assert_eq!(FrontmatterContractRule::default().check(&f), Vec::new());
    }

    #[test]
    fn a_control_character_embedded_in_a_mismatched_type_value_is_stripped_from_the_message() {
        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("how-to\rinjected: line".to_string());
        let f = file("kaibo/reference/page.md", fm);
        let violations = FrontmatterContractRule::default().check(&f);
        assert_eq!(violations.len(), 1);
        assert!(!violations[0].message.contains('\r'));
    }

    #[test]
    fn a_malformed_frontmatter_block_is_reported_once_and_short_circuits_the_other_checks() {
        let f = LintedFile {
            repo_relative_path: "kaibo/reference/page.md".to_string(),
            frontmatter: Err("frontmatter block is not closed by a `---` line".to_string()),
            body: String::new(),
        };
        let violations = FrontmatterContractRule::default().check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("malformed"));
    }

    #[test]
    fn every_violation_is_reported_as_structural_severity() {
        let f = file("kaibo/reference/page.md", Frontmatter::default());
        for v in FrontmatterContractRule::default().check(&f) {
            assert_eq!(v.severity, Severity::Structural);
        }
    }

    // --- configurable: required_keys ---------------------------------

    #[test]
    fn a_required_key_not_in_the_default_four_is_checked_generically_against_extra() {
        let rule = FrontmatterContractRule::new(
            vec!["title".to_string(), "owner".to_string()],
            Vec::new(),
            BTreeMap::new(),
        );
        let mut fm = well_formed_frontmatter();
        fm.tags = None; // dropped from required_keys below, so absence is fine
        let f = file("kaibo/reference/page.md", fm);
        let violations = rule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("`owner`"));
    }

    #[test]
    fn dropping_a_default_key_from_required_keys_stops_it_being_checked() {
        let rule =
            FrontmatterContractRule::new(vec!["title".to_string()], Vec::new(), BTreeMap::new());
        let fm = Frontmatter {
            title: Some("A title".to_string()),
            doc_type: Some("reference".to_string()),
            ..Default::default()
        };
        let f = file("kaibo/reference/page.md", fm);
        // tags, status and updated are all absent, but none of them are in
        // required_keys, so this configuration reports nothing for them.
        assert_eq!(rule.check(&f), Vec::new());
    }

    #[test]
    fn a_required_key_present_in_extra_satisfies_the_generic_check() {
        let mut extra = std::collections::BTreeMap::new();
        extra.insert(
            "owner".to_string(),
            serde_yaml_ng::Value::String("team-x".to_string()),
        );
        let rule =
            FrontmatterContractRule::new(vec!["owner".to_string()], Vec::new(), BTreeMap::new());
        let f = file(
            "kaibo/reference/page.md",
            Frontmatter {
                extra,
                doc_type: Some("reference".to_string()),
                ..Default::default()
            },
        );
        assert_eq!(rule.check(&f), Vec::new());
    }

    // --- configurable: allowed_status ---------------------------------

    #[test]
    fn an_empty_allowed_status_list_accepts_any_present_status_the_original_behaviour() {
        let mut fm = well_formed_frontmatter();
        fm.status = Some(Status::Unknown("experimental".to_string()));
        let f = file("kaibo/reference/page.md", fm);
        assert_eq!(FrontmatterContractRule::default().check(&f), Vec::new());
    }

    #[test]
    fn a_status_outside_a_configured_allow_list_is_reported() {
        let rule = FrontmatterContractRule::new(
            vec!["status".to_string()],
            vec!["current".to_string(), "deprecated".to_string()],
            BTreeMap::new(),
        );
        let mut fm = well_formed_frontmatter();
        fm.status = Some(Status::Draft);
        let f = file("kaibo/reference/page.md", fm);
        let violations = rule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("draft"));
        assert!(violations[0].message.contains("current"));
    }

    #[test]
    fn a_status_inside_a_configured_allow_list_is_not_reported() {
        let rule = FrontmatterContractRule::new(
            vec!["status".to_string()],
            vec!["current".to_string()],
            BTreeMap::new(),
        );
        let mut fm = well_formed_frontmatter();
        fm.status = Some(Status::Current);
        let f = file("kaibo/reference/page.md", fm);
        assert_eq!(rule.check(&f), Vec::new());
    }

    // --- configurable: type_folder_overrides ---------------------------

    #[test]
    fn a_folder_not_named_in_overrides_still_uses_the_identity_mapping() {
        let rule = FrontmatterContractRule::new(Vec::new(), Vec::new(), BTreeMap::new());
        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("reference".to_string());
        let f = file("kaibo/reference/page.md", fm);
        assert_eq!(rule.check(&f), Vec::new());
    }

    #[test]
    fn a_folder_named_in_overrides_expects_the_mapped_type_not_the_folder_name() {
        let mut overrides = BTreeMap::new();
        overrides.insert("howto".to_string(), "how-to".to_string());
        let rule = FrontmatterContractRule::new(Vec::new(), Vec::new(), overrides);

        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("howto".to_string());
        let unmapped = file("kaibo/howto/page.md", fm.clone());
        let violations = rule.check(&unmapped);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("how-to"));

        fm.doc_type = Some("how-to".to_string());
        let mapped = file("kaibo/howto/page.md", fm);
        assert_eq!(rule.check(&mapped), Vec::new());
    }
}
