//! Primitives for treating corpus content as untrusted, shared so `query`,
//! `doctrine` and `contribute` reuse one implementation instead of each
//! getting it slightly wrong.
//!
//! The fence is escape-resistant, not unescapable: see
//! `a_snippet_containing_the_literal_fence_marker_cannot_forge_a_fence_boundary`
//! in `query/tests.rs` for what it actually defeats.

use std::path::PathBuf;

use crate::config::Config;
use crate::frontmatter::Status;

/// Strip control characters from a corpus scalar at ingest time, so text
/// and JSON rendering can never disagree about what was stripped. Not a
/// substitute for [`fence`], which handles values shown verbatim instead.
pub(crate) fn strip_control_chars(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// Kept as constants so the marker [`fence`] emits and the marker
/// [`neutralize_marker`] blocks can never drift apart.
const FENCE_OPEN_MARKER: &str = "<<<UNTRUSTED CORPUS CONTENT";
const FENCE_CLOSE_MARKER: &str = "<<<END UNTRUSTED CORPUS CONTENT";

/// Replace the fence marker's opening sequence (`<<<`) with a visually
/// similar but byte-distinct stand-in, so content containing the literal
/// marker text cannot forge a fence boundary.
pub(crate) fn neutralize_marker(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains("<<<") {
        std::borrow::Cow::Owned(text.replace("<<<", "\u{FF1C}\u{FF1C}\u{FF1C}"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Wrap retrieved content in an explicit, path-naming delimiter pair so the
/// trust boundary is a format, not a discipline the caller has to maintain.
///
/// Neutralizing `path` and `content` closes the one forgery this function
/// is responsible for; it says nothing about whether `path` is itself a
/// safe, contained filesystem path - see [`repo_relative_path`] and
/// [`resolve_contained_path`] for that.
pub(crate) fn fence(path: &str, content: &str) -> String {
    let safe_path = neutralize_marker(path);
    let safe_content = neutralize_marker(content);
    format!(
        "{FENCE_OPEN_MARKER} path={safe_path:?}>>>\n{safe_content}\n{FENCE_CLOSE_MARKER} path={safe_path:?}>>>"
    )
}

/// Recover the repo-relative path from qmd's `file` field, e.g.
/// `qmd://knowledge/kaibo/how-to/write-a-good-query.md?index=kaibo` becomes
/// `kaibo/how-to/write-a-good-query.md`. `None` if the remainder is absolute
/// or contains a `..` component.
///
/// String-shape check only: does not catch a same-named symlink inside the
/// clone pointing outside it, see [`resolve_contained_path`].
pub(crate) fn repo_relative_path(file: &str) -> Option<String> {
    let without_query = file.split('?').next().unwrap_or(file);
    let rest = without_query.strip_prefix("qmd://")?;
    let (_, path) = rest.split_once('/')?;
    if path.is_empty() {
        return None;
    }
    let is_contained = std::path::Path::new(path).components().all(|component| {
        matches!(
            component,
            std::path::Component::Normal(_) | std::path::Component::CurDir
        )
    });
    if !is_contained {
        return None;
    }
    Some(path.to_string())
}

/// Canonicalize both the clone root and the candidate path and require the
/// latter to start with the former, catching the symlink route
/// [`repo_relative_path`]'s component check cannot.
///
/// `None` covers both "failed to canonicalize" and "resolved outside the
/// clone" - a caller getting `None` back has no basis to treat it as a
/// confirmed escape, only as unverified.
pub(crate) fn resolve_contained_path(config: &Config, repo_relative_path: &str) -> Option<PathBuf> {
    let full_path = config.clone_path().join(repo_relative_path);

    let canonical_clone = config.clone_path().canonicalize().ok()?;
    let canonical_full = full_path.canonicalize().ok()?;
    if !canonical_full.starts_with(&canonical_clone) {
        return None;
    }

    Some(canonical_full)
}

/// Unverified is treated the same as a verified draft: this crate cannot
/// distinguish "unverifiable" from "happens to be a draft".
pub(crate) fn admits_unverified(verified: bool, include_drafts: bool) -> bool {
    include_drafts || verified
}

pub(crate) fn admits_draft_status(status: &Option<Status>, include_drafts: bool) -> bool {
    include_drafts || *status != Some(Status::Draft)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- defect 3: a hit's `file` is joined into a path with no containment
    // check -----------------------------------------------------------------

    #[test]
    fn repo_relative_path_rejects_parent_dir_traversal() {
        assert_eq!(
            repo_relative_path("qmd://knowledge/../outside/secret.md?index=kaibo"),
            None
        );
    }

    #[test]
    fn repo_relative_path_rejects_an_absolute_remainder() {
        assert_eq!(
            repo_relative_path("qmd://knowledge//abs/path/elsewhere.md?index=kaibo"),
            None
        );
    }

    #[test]
    fn repo_relative_path_accepts_a_plain_contained_path() {
        assert_eq!(
            repo_relative_path("qmd://knowledge/kaibo/reference/page.md?index=kaibo"),
            Some("kaibo/reference/page.md".to_string())
        );
    }
}
