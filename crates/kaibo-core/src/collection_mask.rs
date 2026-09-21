//! The single definition of the mask that separates corpus *knowledge* from
//! corpus *metadata*: typed content folders only (`reference`, `how-to`,
//! `faq`) one level under each domain folder. Root files like `README.md`,
//! `_index.md`, and `CODEOWNERS` are navigation and metadata, not
//! knowledge.
//!
//! Three call sites need this exact shape, and used to each carry their own
//! copy of the literal, kept in agreement only by convention:
//!
//! - [`crate::sync`] registers it with qmd (`qmd collection add ... <mask>`),
//!   so only matching files are ever embedded.
//! - [`crate::status`]'s `qmd_contract_check` (opt-in, non-hermetic) proves
//!   qmd itself applies this exact mask the way this crate assumes.
//! - [`crate::lint`] filters its own directory walk through [`matches`], so
//!   the file set it checks is provably the file set `sync` indexes - a
//!   page nothing indexes (the corpus `README.md`, a domain's `_index.md`)
//!   can no longer fail lint just for existing outside the mask.
//!
//! A fourth, independently-typed copy of the literal is exactly the bug
//! this module exists to close - see
//! `tests::collection_mask_literal_is_confined_to_this_module` below, which
//! scans the rest of the crate for it the same way
//! `qmd::tests::qmd_command_literal_is_confined_to_this_module` scans for a
//! raw `"qmd"` command literal.

/// Typed content folders only (`reference`, `how-to`, `faq`) one level
/// under each domain folder, at any depth beneath that.
pub(crate) const COLLECTION_MASK: &str = "*/{reference,how-to,faq}/**/*.md";

/// True if `repo_relative_path` (forward-slash separated, no leading `/`)
/// matches [`COLLECTION_MASK`].
///
/// Interprets the mask generically rather than hardcoding its shape a
/// second time: `*` matches one non-empty path segment (or, when it
/// prefixes a literal suffix like `*.md`, any filename ending in that
/// suffix), `{a,b,c}` matches one of the listed alternatives for a single
/// segment, and `**` matches zero or more whole path segments. A future
/// change to [`COLLECTION_MASK`] changes what this accepts without a
/// second hand-written implementation to keep in sync with it.
pub(crate) fn matches(repo_relative_path: &str) -> bool {
    matches_glob(COLLECTION_MASK, repo_relative_path)
}

fn matches_glob(mask: &str, path: &str) -> bool {
    let mask_segments: Vec<&str> = mask.split('/').collect();
    let path_segments: Vec<&str> = path.split('/').collect();
    matches_segments(&mask_segments, &path_segments)
}

fn matches_segments(mask: &[&str], path: &[&str]) -> bool {
    match mask.first() {
        None => path.is_empty(),
        Some(&"**") => {
            // `**` matches zero segments (try the rest of the mask here)
            // or one-or-more (consume a segment and stay on `**`).
            matches_segments(&mask[1..], path)
                || match path.split_first() {
                    Some((_, rest)) => matches_segments(mask, rest),
                    None => false,
                }
        }
        Some(segment) => match path.split_first() {
            Some((first, rest)) => {
                matches_segment(segment, first) && matches_segments(&mask[1..], rest)
            }
            None => false,
        },
    }
}

fn matches_segment(mask_segment: &str, path_segment: &str) -> bool {
    if let Some(alternatives) = mask_segment
        .strip_prefix('{')
        .and_then(|s| s.strip_suffix('}'))
    {
        return alternatives
            .split(',')
            .any(|alternative| matches_segment(alternative, path_segment));
    }
    if let Some(suffix) = mask_segment.strip_prefix('*') {
        return !path_segment.is_empty() && path_segment.ends_with(suffix);
    }
    mask_segment == path_segment
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::matches;

    #[test]
    fn a_page_directly_under_a_typed_domain_folder_matches() {
        assert!(matches("kaibo/reference/page.md"));
    }

    #[test]
    fn a_how_to_page_nested_several_levels_deeper_still_matches() {
        assert!(matches("kaibo/how-to/nested/deeper/page.md"));
    }

    #[test]
    fn a_faq_page_under_a_different_domain_matches() {
        assert!(matches("observability/faq/page.md"));
    }

    #[test]
    fn a_repo_root_readme_does_not_match() {
        assert!(!matches("README.md"));
    }

    #[test]
    fn a_domain_root_file_with_no_typed_folder_does_not_match() {
        assert!(!matches("kaibo/README.md"));
    }

    #[test]
    fn a_file_under_an_untyped_domain_folder_does_not_match() {
        assert!(!matches("kaibo/misc/page.md"));
    }

    #[test]
    fn a_non_markdown_file_under_a_typed_folder_does_not_match() {
        assert!(!matches("kaibo/reference/page.txt"));
    }

    /// Guardrail, mirroring
    /// `qmd::tests::qmd_command_literal_is_confined_to_this_module`: nothing
    /// outside this module may spell out the mask literal as a Rust string.
    /// A second, independently-typed copy is exactly the bug this module
    /// closes (see the module doc) - the type system does not stop it, so
    /// this scan does.
    ///
    /// Detection looks for the *quoted* literal (`"*/{reference,how-to,faq}/**/*.md"`,
    /// including the surrounding double quotes), deliberately narrower than
    /// the bare glob text - several test files legitimately hardcode the
    /// mask as a literal function argument when building an *expected*
    /// `PlannedCommand` to assert against (the house rule that an expected
    /// value must never come from the code under test), and a couple of doc
    /// comments mention the glob in backticks. Neither is the bug this
    /// guards against, and this narrower pattern only matches source
    /// files outside this module that reconstruct the mask as their own
    /// `&str` literal - i.e. a fourth, independent production copy.
    #[test]
    fn collection_mask_literal_is_confined_to_this_module() {
        fn rust_files(dir: &Path, out: &mut Vec<std::path::PathBuf>) {
            for entry in std::fs::read_dir(dir).expect("read directory") {
                let path = entry.expect("dir entry").path();
                if path.is_dir() {
                    rust_files(&path, out);
                } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                    out.push(path);
                }
            }
        }

        let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let this_file = src_dir.join("collection_mask.rs");

        let mut files = Vec::new();
        rust_files(&src_dir, &mut files);

        let mut violations = Vec::new();
        for path in files {
            if path == this_file {
                continue;
            }
            // Test files legitimately hardcode the mask as an expected-value
            // literal (see the test's own doc comment above) - excluded by
            // name rather than by `#[cfg(test)]`, which a plain text scan
            // cannot see.
            let is_test_file = path.file_name().and_then(|n| n.to_str()) == Some("tests.rs");
            if is_test_file {
                continue;
            }

            let contents = std::fs::read_to_string(&path).expect("read source file");
            if contents.contains("\"*/{reference,how-to,faq}/**/*.md\"") {
                violations.push(
                    path.strip_prefix(&src_dir)
                        .unwrap_or(&path)
                        .display()
                        .to_string(),
                );
            }
        }

        assert!(
            violations.is_empty(),
            "the collection mask literal must be defined only in collection_mask.rs as \
             COLLECTION_MASK; found an independent copy in: {violations:?}"
        );
    }
}
