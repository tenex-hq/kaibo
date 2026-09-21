//! `kaibo lint [path...]`: the structural gate on the corpus.
//!
//! A rule registry, not a hardcoded function: each rule in [`rules`]
//! declares an id and a check against one already-parsed file. **Every
//! violation gates** - any violation makes the run exit non-zero, the same
//! "malformed corpus content is bad input, not a kaibo bug" mapping
//! [`crate::frontmatter::FrontmatterError`] already uses
//! ([`crate::error::ExitCode::Usage`]).
//!
//! `lint` never calls qmd and never self-heals: it reads whatever is
//! already on disk under the configured clone, which is also what the
//! knowledge repo's own CI checkout looks like. `--explain` therefore has
//! nothing to print - `lint` shells out to nothing - and that empty plan
//! is itself the proof that `--explain` runs nothing for this verb.
//!
//! **A directory walk only ever turns up what [`crate::collection_mask`]
//! accepts** - the same mask `sync` registers the `knowledge` collection
//! with, so a full-corpus run (or a directory `path` argument) checks
//! exactly the file set `sync` indexes, never more. A root `README.md` or a
//! domain's `_index.md` is navigation, not knowledge; `sync` never embeds
//! it, and a walk here never surfaces it either, so it can no longer fail
//! `frontmatter-contract` forever for lacking frontmatter it was never
//! meant to carry. Naming a file explicitly as a `path` argument still
//! checks it regardless of the mask - see [`collect_markdown_files`].
//!
//! **Every path this module reads comes from an argument or a directory
//! walk, never from parsed page content.** A `path` argument is corpus-
//! shaped user input (like `doctrine`'s `<domain>`), not a flag naming a
//! repo, clone path, index or collection, so it carries no exemption from
//! the trust boundary: each one is validated with
//! [`trust::resolve_contained_path`] before anything is read from it, and
//! every file discovered by walking a directory is validated again
//! individually before its contents are read, exactly the defense-in-depth
//! `doctrine::load_current_pages` already uses for a symlinked file
//! discovered underneath an otherwise-contained directory.
//!
//! **Frontmatter and body values are corpus content, read by rules but
//! never fed back into a command or a path.** A rule may *report* a tag or
//! a folder name in its violation message; nothing a rule reads ever
//! reaches `std::process::Command` or a second filesystem lookup - the
//! only path construction in this module is the walk over already-
//! validated directories above.
//!
//! **Rule parameters come from [`Config::lint`], never from a page.** Four
//! parameters are configurable - `frontmatter-contract`'s required keys and
//! allowed `status` values, its folder-to-type mapping, and
//! `tags-kebab-case`'s pattern - plus which rules run at all
//! ([`crate::config::LintConfig::disabled_rules`]). All five are bound by
//! [`Config::resolve`] before `gather` reads a single corpus byte, the same
//! lifecycle that keeps `repo` or `index` out of a page's reach. The
//! intuitive place to put a "custom rule" is next to the content it
//! governs, in the knowledge repo itself - that is exactly what this
//! guarantee forbids: a parameter sourced from the corpus would let anyone
//! who can merge a knowledge PR change what `lint` enforces. See
//! [`rules::registry`] for where the four parameters turn into rules, and
//! `tests::hostile_frontmatter_cannot_change_which_rules_run_or_how` for the
//! guardrail proving a page cannot reach them.
//!
//! What stays out of reach on purpose: declarative rule files loaded from
//! the corpus (rules are compiled, not authored in markdown) and shelling
//! out to a user-named linter (arbitrary execution driven by a path is the
//! one thing every guarantee in this crate exists to prevent). Both were
//! considered and rejected - what's configurable is the four parameters
//! above, because most "custom rules" a corpus actually wants turn out to
//! be one of these three rules with different constants.

use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::collection_mask;
use crate::config::Config;
use crate::error::ExitCode;
use crate::explain::{Explainable, PlannedCommand};
use crate::frontmatter;
use crate::output::{Render, RenderOptions};
use crate::trust;

mod rules;

/// One rule's finding against one file. Every violation gates the run -
/// `normative-atomicity`, the last rule whose violations only annotated,
/// was deleted once nothing consumed a heuristic finding (`contribute
/// apply` filtered to structural before ever reporting one), which is why
/// there is no severity field here to distinguish one violation from
/// another.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    pub rule_id: String,
    pub path: String,
    pub message: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LintOutcome {
    /// The configured clone does not exist (or isn't a directory) at all -
    /// a broken/unsynced corpus, not a gap: this crate has no basis to say
    /// whether the requested paths would have had anything to lint.
    CloneMissing,
    /// A `path` argument did not resolve to a file or directory contained
    /// in the clone. Bad input, not a corpus defect.
    InvalidPath { path: String },
    /// `Config::lint()` could not build a registry from its parameters -
    /// today, only an invalid `tags-kebab-case` regex pattern. A config
    /// mistake, not a corpus defect, so it is reported before any file is
    /// read rather than once per file.
    InvalidConfig { detail: String },
    /// Every candidate path resolved, but none of them turned up a
    /// markdown file to check. The gap signal.
    NoFilesFound,
    /// At least one file was checked, with whatever violations (possibly
    /// none) the registry found.
    Finished {
        files_checked: usize,
        violations: Vec<Violation>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LintReport {
    pub paths: Vec<String>,
    pub outcome: LintOutcome,
}

impl LintReport {
    /// `Stale` when the clone itself is missing; `Usage` for a bad `path`
    /// argument *or* at least one violation (both are "bad input", per
    /// [`crate::frontmatter::FrontmatterError::exit_code`]); `NoHits` when
    /// nothing was there to check; `Success` otherwise - a run with no
    /// violations at all still exits 0.
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            LintOutcome::CloneMissing => ExitCode::Stale,
            LintOutcome::InvalidPath { .. } => ExitCode::Usage,
            LintOutcome::InvalidConfig { .. } => ExitCode::Usage,
            LintOutcome::NoFilesFound => ExitCode::NoHits,
            LintOutcome::Finished { violations, .. } => {
                if violations.is_empty() {
                    ExitCode::Success
                } else {
                    ExitCode::Usage
                }
            }
        }
    }
}

/// The `kaibo lint` verb, bound to a resolved `Config` and zero or more
/// path arguments taken verbatim from argv. An empty list means "the whole
/// corpus".
pub struct LintVerb<'a> {
    config: &'a Config,
    paths: Vec<String>,
}

impl<'a> LintVerb<'a> {
    pub fn new(config: &'a Config, paths: Vec<String>) -> Self {
        Self { config, paths }
    }

    pub fn gather(&self) -> LintReport {
        gather(self.config, &self.paths)
    }
}

impl Explainable for LintVerb<'_> {
    fn explain(&self) -> Vec<PlannedCommand> {
        // lint reads local markdown directly and shells out to nothing.
        Vec::new()
    }
}

fn gather(config: &Config, paths: &[String]) -> LintReport {
    if !config.clone_path().is_dir() {
        return LintReport {
            paths: paths.to_vec(),
            outcome: LintOutcome::CloneMissing,
        };
    }

    let mut candidates = Vec::new();
    if paths.is_empty() {
        collect_markdown_files(config.clone_path(), config.clone_path(), &mut candidates);
    } else {
        for path in paths {
            // Validated for containment before anything is read from it -
            // but the *canonicalized* path this returns is deliberately
            // not what gets walked below: canonicalizing can rewrite the
            // clone root itself (e.g. a `/tmp` that is a symlink to
            // `/private/tmp`), which would make a later
            // `strip_prefix(config.clone_path())` fail for every file
            // found, not just an escaping one. The raw join is what gets
            // walked; this call exists purely to reject an escaping `path`
            // up front, exactly as `doctrine::load_current_pages` does.
            let Some(canonical) = trust::resolve_contained_path(config, path) else {
                return LintReport {
                    paths: paths.to_vec(),
                    outcome: LintOutcome::InvalidPath { path: path.clone() },
                };
            };
            let raw = config.clone_path().join(path);
            if canonical.is_dir() {
                // A directory argument is still a *walk*, so it is filtered
                // through the same collection mask `sync` indexes with -
                // only a single, explicitly-named file (the `else` branch
                // below) bypasses it, since naming one file by hand is a
                // request to check that file, not a discovery step.
                collect_markdown_files(&raw, config.clone_path(), &mut candidates);
            } else {
                candidates.push(raw);
            }
        }
    }
    candidates.sort();
    candidates.dedup();

    let registry = match rules::registry(config.lint()) {
        Ok(registry) => registry,
        Err(detail) => {
            return LintReport {
                paths: paths.to_vec(),
                outcome: LintOutcome::InvalidConfig { detail },
            };
        }
    };
    let mut violations = Vec::new();
    let mut files_checked = 0usize;

    for full_path in &candidates {
        let Ok(repo_relative) = full_path.strip_prefix(config.clone_path()) else {
            continue;
        };
        let repo_relative = repo_relative.to_string_lossy().replace('\\', "/");

        // Re-validated per file, not just for the directory it was found
        // under: a file discovered by the walk can itself be a symlink
        // resolving outside the clone.
        let Some(canonical) = trust::resolve_contained_path(config, &repo_relative) else {
            continue;
        };
        let Ok(contents) = std::fs::read_to_string(&canonical) else {
            continue;
        };

        files_checked += 1;

        let linted = match frontmatter::parse(&contents) {
            Ok(doc) => rules::LintedFile {
                repo_relative_path: trust::strip_control_chars(&repo_relative),
                frontmatter: Ok(doc.frontmatter),
                body: doc.body,
            },
            Err(err) => rules::LintedFile {
                repo_relative_path: trust::strip_control_chars(&repo_relative),
                frontmatter: Err(err.to_string()),
                body: String::new(),
            },
        };

        for rule in &registry {
            violations.extend(rule.check(&linted));
        }
    }

    if files_checked == 0 {
        return LintReport {
            paths: paths.to_vec(),
            outcome: LintOutcome::NoFilesFound,
        };
    }

    LintReport {
        paths: paths.to_vec(),
        outcome: LintOutcome::Finished {
            files_checked,
            violations,
        },
    }
}

/// Recursively collect every `.md` file under `dir` that also matches
/// [`collection_mask::matches`] - the same mask `sync` registers the
/// `knowledge` collection with - skipping any directory whose name starts
/// with `.` (chiefly `.git`, which is neither corpus content nor safe to
/// walk into wholesale) and not following a symlinked directory -
/// `DirEntry::file_type` does not follow symlinks, so a symlinked
/// subdirectory is skipped here rather than walked into.
///
/// `clone_root` is always the configured clone root, not necessarily `dir`
/// itself: a directory `path` argument recurses starting below the clone
/// root, and the mask is defined relative to the clone root, so the file
/// set a directory argument turns up stays identical to what a full-corpus
/// run would have found under that same subtree. This is what makes lint's
/// walk provably cover the same files `sync` indexes - see
/// `crate::collection_mask` and, for the corpus `README.md` this used to
/// wrongly fail on, `tests::a_repo_root_readme_is_excluded_from_the_default_corpus_walk`.
fn collect_markdown_files(dir: &Path, clone_root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let name_is_hidden = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with('.'));
        if name_is_hidden {
            continue;
        }
        if file_type.is_dir() {
            collect_markdown_files(&path, clone_root, out);
        } else if path.extension().and_then(|ext| ext.to_str()) == Some("md") {
            let repo_relative = path
                .strip_prefix(clone_root)
                .ok()
                .map(|relative| relative.to_string_lossy().replace('\\', "/"));
            if repo_relative.is_some_and(|relative| collection_mask::matches(&relative)) {
                out.push(path);
            }
        }
    }
}

fn violation_json(violation: &Violation) -> Value {
    serde_json::json!({
        "rule_id": violation.rule_id,
        "path": violation.path,
        "message": violation.message,
    })
}

impl Render for LintReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = Vec::new();
        lines.push(format!("kaibo lint {:?}", self.paths));

        match &self.outcome {
            LintOutcome::CloneMissing => {
                lines.push("result: clone missing".to_string());
                lines.push("  - next: `kaibo sync`".to_string());
            }
            LintOutcome::InvalidPath { path } => {
                lines.push(format!("result: invalid path {path:?}"));
                lines.push(
                    "  - next: pass a path inside the configured clone, or none for the whole corpus"
                        .to_string(),
                );
            }
            LintOutcome::InvalidConfig { detail } => {
                lines.push(format!("result: invalid lint configuration: {detail}"));
                lines.push(
                    "  - next: fix `[lint]` in `~/.kaibo/config.toml`, then re-run `kaibo lint`"
                        .to_string(),
                );
            }
            LintOutcome::NoFilesFound => {
                lines.push("result: gap, no markdown files found".to_string());
            }
            LintOutcome::Finished {
                files_checked,
                violations,
            } => {
                lines.push(format!(
                    "result: {files_checked} file(s) checked, {} violation(s)",
                    violations.len()
                ));
                for violation in violations {
                    lines.push(format!(
                        "- {} {}: {}",
                        violation.rule_id, violation.path, violation.message,
                    ));
                }
            }
        }

        if options.full {
            lines.push(format!("exit code: {}", self.exit_code().code()));
        }

        lines.join("\n")
    }

    fn render_json(&self) -> Value {
        let outcome = match &self.outcome {
            LintOutcome::CloneMissing => serde_json::json!({ "state": "clone_missing" }),
            LintOutcome::InvalidPath { path } => serde_json::json!({
                "state": "invalid_path",
                "path": path,
            }),
            LintOutcome::InvalidConfig { detail } => serde_json::json!({
                "state": "invalid_config",
                "detail": detail,
            }),
            LintOutcome::NoFilesFound => serde_json::json!({ "state": "no_files_found" }),
            LintOutcome::Finished {
                files_checked,
                violations,
            } => serde_json::json!({
                "state": "finished",
                "files_checked": files_checked,
                "violations": violations.iter().map(violation_json).collect::<Vec<_>>(),
            }),
        };

        serde_json::json!({
            "paths": self.paths,
            "outcome": outcome,
            "exit_code": self.exit_code().code(),
        })
    }
}

#[cfg(test)]
mod tests;
