//! Rule registry: each rule declares an id, a severity policy, and a check
//! against one already-parsed file. Adding a rule means adding a file
//! under this module plus one line in [`registry`] - there is no match arm
//! on rule id anywhere in this crate for a rule to fall out of.

use crate::frontmatter::Frontmatter;
use crate::lint::{Severity, Violation};

mod frontmatter_contract;
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

pub(crate) fn registry() -> Vec<Box<dyn Rule>> {
    vec![
        Box::new(frontmatter_contract::FrontmatterContractRule),
        Box::new(tags_kebab_case::TagsKebabCaseRule),
        Box::new(prose_style::ProseStyleRule),
    ]
}
