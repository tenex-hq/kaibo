//! Structural: `title`, `tags`, `status` and `updated` are present, and
//! `type` matches the name of the page's immediate containing folder.

use super::{LintedFile, Rule, violation};
use crate::lint::{Severity, Violation};
use crate::trust;

pub(crate) struct FrontmatterContractRule;

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

        if fm.title.as_deref().unwrap_or("").trim().is_empty() {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "missing required frontmatter field `title`",
            ));
        }

        match &fm.tags {
            Some(tags) if !tags.is_empty() => {}
            _ => out.push(violation(
                self,
                &file.repo_relative_path,
                "missing required frontmatter field `tags`",
            )),
        }

        if fm.status.is_none() {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "missing required frontmatter field `status`",
            ));
        }

        if fm.updated.is_none() {
            out.push(violation(
                self,
                &file.repo_relative_path,
                "missing required frontmatter field `updated`",
            ));
        }

        if let Some(expected) = expected_type(&file.repo_relative_path) {
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

/// The expected `type` value for a page: the name of its immediate parent
/// folder, when the page is nested at least one folder below the domain
/// root (e.g. `kaibo/reference/foo.md` -> `reference`). A page directly
/// under the domain root (e.g. `kaibo/_index.md`) has no such folder to
/// check against and is exempt.
fn expected_type(repo_relative_path: &str) -> Option<&str> {
    let parts: Vec<&str> = repo_relative_path.split('/').collect();
    if parts.len() < 3 {
        return None;
    }
    Some(parts[parts.len() - 2])
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
        assert_eq!(FrontmatterContractRule.id(), "frontmatter-contract");
    }

    #[test]
    fn a_page_with_every_required_field_and_a_matching_type_has_no_violations() {
        let f = file("kaibo/reference/page.md", well_formed_frontmatter());
        assert_eq!(FrontmatterContractRule.check(&f), Vec::new());
    }

    #[test]
    fn a_page_missing_title_tags_status_and_updated_reports_all_four() {
        let fm = crate::frontmatter::Frontmatter {
            doc_type: Some("reference".to_string()),
            ..Default::default()
        };
        let f = file("kaibo/reference/page.md", fm);
        let violations = FrontmatterContractRule.check(&f);
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
        let violations = FrontmatterContractRule.check(&f);
        assert!(violations.iter().any(|v| v.message.contains("`tags`")));
    }

    #[test]
    fn a_type_that_does_not_match_the_containing_folder_is_reported() {
        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("how-to".to_string());
        let f = file("kaibo/reference/page.md", fm);
        let violations = FrontmatterContractRule.check(&f);
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
        assert_eq!(FrontmatterContractRule.check(&f), Vec::new());
    }

    #[test]
    fn a_page_directly_under_the_domain_root_is_exempt_from_the_type_check() {
        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("index".to_string());
        let f = file("_index.md", fm);
        assert_eq!(FrontmatterContractRule.check(&f), Vec::new());
    }

    #[test]
    fn a_control_character_embedded_in_a_mismatched_type_value_is_stripped_from_the_message() {
        let mut fm = well_formed_frontmatter();
        fm.doc_type = Some("how-to\rinjected: line".to_string());
        let f = file("kaibo/reference/page.md", fm);
        let violations = FrontmatterContractRule.check(&f);
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
        let violations = FrontmatterContractRule.check(&f);
        assert_eq!(violations.len(), 1);
        assert!(violations[0].message.contains("malformed"));
    }

    #[test]
    fn every_violation_is_reported_as_structural_severity() {
        let f = file("kaibo/reference/page.md", Frontmatter::default());
        for v in FrontmatterContractRule.check(&f) {
            assert_eq!(v.severity, Severity::Structural);
        }
    }
}
