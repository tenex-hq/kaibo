//! The boundary this crate holds around corpus content: everything read out
//! of the local clone (a page's frontmatter, a snippet qmd returned, a
//! heading in the root MOC) is data the crate is retrieving on the caller's
//! behalf, never an instruction the crate itself acts on. This module holds
//! the mechanisms that boundary rests on, so `query` today - and `doctrine`
//! and `contribute` once they exist - can reuse the same primitives against
//! the same untrusted corpus instead of each reimplementing (and each
//! slightly misimplementing) their own version.
//!
//! What this module guarantees:
//!
//! - [`fence`] wraps untrusted content in an explicit, path-naming delimiter
//!   pair, after [`neutralize_marker`] rewrites any occurrence of the
//!   delimiter's own marker text found inside the path or the content being
//!   fenced. A page cannot forge a fence boundary of its own by including
//!   the literal marker text in its snippet.
//! - [`strip_control_chars`] removes control characters (newlines,
//!   carriage returns, ...) from a corpus scalar before it is stored, so a
//!   value printed unfenced later (a title, a path, a status word, a MOC
//!   heading) cannot inject an extra line into the caller's own output.
//! - [`repo_relative_path`] rejects, by string shape alone, a `file` value
//!   that is absolute or contains a `..` component, before it is ever
//!   joined onto the clone root.
//! - [`resolve_contained_path`] closes the gap a string-shape check cannot:
//!   it canonicalizes both the clone root and the candidate path and
//!   requires the latter to start with the former, so a same-named entry
//!   that is itself a symlink pointing outside the clone is rejected too.
//! - [`admits_unverified`] and [`admits_draft_status`] decide whether a hit
//!   is withheld from the caller by default: unless the caller opted in to
//!   seeing drafts, anything this crate could not positively verify is
//!   withheld exactly like a page verified as a draft - the two are
//!   deliberately not distinguished, since an unverifiable page might well
//!   be a draft and there is no way to tell which.
//!
//! What this module does not guarantee:
//!
//! - The fence is **escape-resistant, not unescapable**: [`neutralize_marker`]
//!   defeats the one forgery this module knows to look for - content
//!   containing the fence's own marker text - not every way adversarial
//!   corpus content could otherwise confuse a consumer of kaibo's output.
//! - [`resolve_contained_path`]'s containment check **depends on the clone
//!   root itself being canonicalizable**. If `config.clone_path()` cannot be
//!   canonicalized at all, this module has no basis for a containment
//!   decision one way or the other, and treats the path as unverified - not
//!   as contained, and not as a confirmed escape either.
//! - Nothing here decides *what* to read, or reads anything itself; that
//!   remains the calling verb's job. This module only decides whether
//!   something the caller already read is safe to trust, name, or show.

use std::path::PathBuf;

use crate::config::Config;
use crate::frontmatter::Status;

/// Strip characters that could let corpus content forge extra lines of
/// kaibo's own output - newlines, carriage returns, and other control
/// characters - from a scalar pulled out of qmd's JSON or a page's
/// frontmatter, before it is stored at all. Applied at ingest rather than
/// only at render time, so text and JSON rendering can never disagree about
/// what was stripped. Not a substitute for [`fence`]: a value fenced instead
/// (a snippet) is never passed through this function, since fencing is the
/// mechanism responsible for it.
pub(crate) fn strip_control_chars(s: &str) -> String {
    s.chars().filter(|c| !c.is_control()).collect()
}

/// The literal delimiter [`fence`] wraps untrusted content in. Kept as
/// constants so the marker text used to build a fence and the marker text
/// [`neutralize_marker`] blocks from appearing inside fenced content can
/// never drift apart.
const FENCE_OPEN_MARKER: &str = "<<<UNTRUSTED CORPUS CONTENT";
const FENCE_CLOSE_MARKER: &str = "<<<END UNTRUSTED CORPUS CONTENT";

/// Replace any occurrence of the fence's own opening sequence (`<<<`) with a
/// visually similar but byte-distinct stand-in, so a page whose snippet or
/// path contains literal fence-marker text cannot forge a fake fence
/// boundary and have the rest of its content read as kaibo's own output.
/// This runs on both `path` and `content` before either is placed inside
/// [`fence`]'s output, so neither can smuggle in an extra `<<<`.
pub(crate) fn neutralize_marker(text: &str) -> std::borrow::Cow<'_, str> {
    if text.contains("<<<") {
        std::borrow::Cow::Owned(text.replace("<<<", "\u{FF1C}\u{FF1C}\u{FF1C}"))
    } else {
        std::borrow::Cow::Borrowed(text)
    }
}

/// Wrap retrieved content in an explicit, path-naming delimiter pair so the
/// trust boundary arrives as a format the consuming model cannot lose track
/// of, rather than a discipline it has to maintain. Used identically by
/// every render path that emits corpus content, so the fencing can never
/// drift between them.
///
/// Neither `path` nor `content` is trusted: both come from the corpus (a
/// hit's file name and its retrieved snippet), so both are run through
/// [`neutralize_marker`] first. That closes the specific escape this
/// function is responsible for - content containing the literal delimiter
/// text cannot terminate the fence early - but it does not itself vouch for
/// `path` being a safe, contained filesystem path; that containment check
/// happens earlier, via [`repo_relative_path`] and [`resolve_contained_path`].
pub(crate) fn fence(path: &str, content: &str) -> String {
    let safe_path = neutralize_marker(path);
    let safe_content = neutralize_marker(content);
    format!(
        "{FENCE_OPEN_MARKER} path={safe_path:?}>>>\n{safe_content}\n{FENCE_CLOSE_MARKER} path={safe_path:?}>>>"
    )
}

/// Recover the repo-relative path (domain folder first) from qmd's `file`
/// field, e.g. `qmd://knowledge/kaibo/how-to/write-a-good-query.md?index=kaibo`
/// becomes `kaibo/how-to/write-a-good-query.md` - the qmd collection name
/// (`knowledge`) is qmd's own addressing, not part of the corpus's own
/// layout, so it is stripped along with the `qmd://` scheme and the
/// trailing `?index=...` qmd appends.
///
/// `file` is qmd's own JSON field, ultimately traceable back to a corpus
/// page's own frontmatter/indexing - not something this crate should trust
/// to stay inside the clone. `None` is returned, instead of the remainder
/// verbatim, when it is absolute (a leading [`std::path::Component::RootDir`])
/// or contains any [`std::path::Component::ParentDir`] (a `..` segment):
/// both are ways the remainder could point outside the clone once joined
/// onto `config.clone_path()`. This is a string-shape check only; it does
/// not defend against a same-named file inside the clone that is itself a
/// symlink pointing outside it - see [`resolve_contained_path`] for that.
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

/// Resolve a repo-relative path under `config.clone_path()` and verify it
/// still lands inside the clone once both sides are canonicalized - the
/// check that closes the symlink route [`repo_relative_path`]'s component
/// check cannot: a same-named entry inside the clone that is itself a
/// symlink pointing outside it resolves to a path that fails `starts_with`
/// here, even though its own path string never contained a `..` or leading
/// `/`.
///
/// `None` if either side fails to canonicalize (including: the clone root
/// itself does not exist) or if the resolved path does not start with the
/// resolved clone root. A caller that gets `None` back has no basis to
/// treat the path as contained - and, per the module doc above, no basis to
/// treat it as a confirmed escape either; it simply could not be verified.
pub(crate) fn resolve_contained_path(config: &Config, repo_relative_path: &str) -> Option<PathBuf> {
    let full_path = config.clone_path().join(repo_relative_path);

    let canonical_clone = config.clone_path().canonicalize().ok()?;
    let canonical_full = full_path.canonicalize().ok()?;
    if !canonical_full.starts_with(&canonical_clone) {
        return None;
    }

    Some(canonical_full)
}

/// Whether a hit whose frontmatter could not be positively verified is
/// still admitted: unless the caller opted in to seeing drafts
/// (`include_drafts`), an unverified hit is withheld exactly like a
/// verified draft would be - this crate cannot distinguish "unverifiable"
/// from "happens to be a draft", so it treats the two with the same
/// caution rather than guessing "not a draft".
pub(crate) fn admits_unverified(verified: bool, include_drafts: bool) -> bool {
    include_drafts || verified
}

/// Whether a hit with a known frontmatter status is admitted: unless the
/// caller opted in to seeing drafts, a status of exactly `Status::Draft` is
/// withheld.
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
