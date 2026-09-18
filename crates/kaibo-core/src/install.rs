//! `kaibo install`: put the skills this binary carries on disk where Claude
//! Code finds them, and take them away again.
//!
//! The skills are compiled in with `include_str!` rather than shipped as a
//! separate plugin repository. One artifact, one version: upgrading the
//! binary upgrades the prose that drives it, in the same step, so a skill
//! cannot describe a mechanism the binary no longer has. That coupling is
//! the whole point of this module, and [`crate::status`] is what makes a
//! break in it visible.
//!
//! The install target is a *plugin* directory, not a bare skill directory.
//! A bare skill in the user-global skills directory is invoked as `/query`;
//! the same skill inside a directory carrying a `.claude-plugin/plugin.json`
//! is discovered as a plugin and keeps its `kaibo:` prefix, with no
//! marketplace to register and no cache directory to write into.
//!
//! Nothing here reads the knowledge corpus, and nothing it writes is
//! derived from corpus content: the destination comes from
//! [`Config::skills_dir`], the content is a compile-time constant.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::config::Config;
use crate::error::ExitCode;
use crate::output::{Render, RenderOptions};

/// The plugin directory's name, which is also the skill namespace: a skill
/// under it is invoked as `kaibo:<skill>`.
pub const PLUGIN_NAME: &str = "kaibo";

/// The plugin manifest's `description`. Claude Code shows this when listing
/// plugins; it is not a trigger string, so it stays short.
const PLUGIN_DESCRIPTION: &str =
    "Query and contribute to a git-backed markdown knowledge corpus, through the kaibo CLI.";

const MANIFEST_DIR: &str = ".claude-plugin";
const MANIFEST_FILE: &str = "plugin.json";
const SKILLS_SUBDIR: &str = "skills";
const SKILL_FILE: &str = "SKILL.md";

/// One skill's prose, compiled into the binary.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EmbeddedSkill {
    pub name: &'static str,
    pub source: &'static str,
}

/// Every skill this binary carries. Alphabetical, which is also the order
/// [`InstallReport`] lists its changes in.
pub const EMBEDDED_SKILLS: &[EmbeddedSkill] = &[
    EmbeddedSkill {
        name: "contribute",
        source: include_str!("../skills/contribute/SKILL.md"),
    },
    EmbeddedSkill {
        name: "query",
        source: include_str!("../skills/query/SKILL.md"),
    },
    EmbeddedSkill {
        name: "sync",
        source: include_str!("../skills/sync/SKILL.md"),
    },
];

/// The absolute paths `install` owns, derived from config and nothing else.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PluginLayout {
    root: PathBuf,
}

impl PluginLayout {
    /// `None` when config could not work out where Claude Code stores
    /// skills, which is the one state that leaves `install` with nowhere
    /// to write.
    pub fn new(config: &Config) -> Option<Self> {
        config.skills_dir().map(|dir| Self {
            root: dir.join(PLUGIN_NAME),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn manifest_path(&self) -> PathBuf {
        self.root.join(MANIFEST_DIR).join(MANIFEST_FILE)
    }

    pub fn skill_path(&self, skill: &str) -> PathBuf {
        self.root.join(SKILLS_SUBDIR).join(skill).join(SKILL_FILE)
    }

    /// Every directory `install` may have created, deepest first - the order
    /// `uninstall` has to prune them in.
    fn owned_dirs(&self) -> Vec<PathBuf> {
        let mut dirs: Vec<PathBuf> = EMBEDDED_SKILLS
            .iter()
            .map(|skill| self.root.join(SKILLS_SUBDIR).join(skill.name))
            .collect();
        dirs.push(self.root.join(SKILLS_SUBDIR));
        dirs.push(self.root.join(MANIFEST_DIR));
        dirs.push(self.root.clone());
        dirs
    }
}

/// The manifest bytes for `version`. Built through `serde_json` rather than
/// formatted by hand, so a version string containing a quote cannot produce
/// a file Claude Code fails to parse.
pub(crate) fn manifest_contents(version: &str) -> String {
    let manifest = serde_json::json!({
        "name": PLUGIN_NAME,
        "version": version,
        "description": PLUGIN_DESCRIPTION,
    });
    format!(
        "{}\n",
        serde_json::to_string_pretty(&manifest)
            .expect("a serde_json::json! object of owned strings always serialises")
    )
}

/// What `install` did, or would do, to one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileAction {
    /// The path did not exist.
    Created,
    /// The path existed with different bytes.
    Updated,
    /// The path already held exactly these bytes.
    Unchanged,
    /// The path existed and was removed.
    Removed,
    /// There was nothing at the path to remove.
    Absent,
}

impl FileAction {
    fn as_str(self) -> &'static str {
        match self {
            FileAction::Created => "created",
            FileAction::Updated => "updated",
            FileAction::Unchanged => "unchanged",
            FileAction::Removed => "removed",
            FileAction::Absent => "absent",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FileChange {
    pub path: PathBuf,
    pub action: FileAction,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InstallMode {
    Install,
    Uninstall,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstallOutcome {
    Ok {
        changes: Vec<FileChange>,
        /// Directories left in place because they hold something `install`
        /// did not put there. Stop and report, never discard: a directory
        /// with a stranger's file in it is not kaibo's to delete.
        retained: Vec<PathBuf>,
    },
    Failed {
        detail: String,
        /// The exact command or change that fixes this failure. Errors are
        /// instructions, and the two ways `install` fails need different
        /// ones.
        fix: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InstallReport {
    pub mode: InstallMode,
    /// `None` when there is no home directory and no `CLAUDE_CONFIG_DIR`,
    /// so there is nowhere to install to.
    pub root: Option<PathBuf>,
    pub version: String,
    /// `false` for a `--explain` run, which computes every action and
    /// performs none of them.
    pub applied: bool,
    pub outcome: InstallOutcome,
}

impl InstallReport {
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            // A path that cannot be written is the user's environment
            // needing a fix (a read-only home, a file where a directory
            // belongs), not a bug in kaibo.
            InstallOutcome::Failed { .. } => ExitCode::Usage,
            InstallOutcome::Ok { .. } => ExitCode::Success,
        }
    }

    pub fn findings(&self) -> Vec<(String, Option<String>)> {
        let mut findings = Vec::new();
        match &self.outcome {
            InstallOutcome::Failed { detail, fix } => {
                findings.push((detail.clone(), Some(fix.clone())))
            }
            InstallOutcome::Ok { retained, .. } => {
                for dir in retained {
                    findings.push((
                        format!(
                            "left {} in place: it holds files kaibo did not install",
                            dir.display()
                        ),
                        None,
                    ));
                }
            }
        }
        findings
    }
}

/// The `kaibo install` verb, bound to a resolved `Config`.
pub struct InstallVerb<'a> {
    config: &'a Config,
    version: String,
    mode: InstallMode,
}

impl<'a> InstallVerb<'a> {
    /// `version` is the `kaibo` binary's own version, known only to the
    /// binary crate, so it is threaded in here the same way
    /// [`crate::status::StatusVerb::gather`] takes it.
    pub fn new(config: &'a Config, version: impl Into<String>, mode: InstallMode) -> Self {
        Self {
            config,
            version: version.into(),
            mode,
        }
    }

    /// Work out every action without performing any of them. This is what
    /// `--explain` runs: `install` shells out to nothing, so there is no
    /// command to print, but there is a filesystem change to describe.
    pub fn plan(&self) -> InstallReport {
        self.run(false)
    }

    pub fn apply(&self) -> InstallReport {
        self.run(true)
    }

    fn run(&self, apply: bool) -> InstallReport {
        let Some(layout) = PluginLayout::new(self.config) else {
            return InstallReport {
                mode: self.mode,
                root: None,
                version: self.version.clone(),
                applied: apply,
                outcome: InstallOutcome::Failed {
                    detail: "could not work out where Claude Code stores skills: \
                             no home directory and no CLAUDE_CONFIG_DIR"
                        .to_string(),
                    fix: "export CLAUDE_CONFIG_DIR=/path/to/.claude, then re-run \
                          `kaibo install`"
                        .to_string(),
                },
            };
        };
        let outcome = match self.mode {
            InstallMode::Install => install(&layout, &self.version, apply),
            InstallMode::Uninstall => uninstall(&layout, apply),
        };
        InstallReport {
            mode: self.mode,
            root: Some(layout.root().to_path_buf()),
            version: self.version.clone(),
            applied: apply,
            outcome,
        }
    }
}

/// Every path `install` writes, with the bytes it writes there.
fn desired_files(layout: &PluginLayout, version: &str) -> Vec<(PathBuf, String)> {
    let mut files = vec![(layout.manifest_path(), manifest_contents(version))];
    for skill in EMBEDDED_SKILLS {
        files.push((layout.skill_path(skill.name), skill.source.to_string()));
    }
    files
}

fn writable_fix(layout: &PluginLayout) -> String {
    format!(
        "make {} writable, then re-run `kaibo install`",
        layout.root().display()
    )
}

fn install(layout: &PluginLayout, version: &str, apply: bool) -> InstallOutcome {
    let mut changes = Vec::new();
    for (path, contents) in desired_files(layout, version) {
        let existing = fs::read_to_string(&path).ok();
        let action = match &existing {
            Some(current) if *current == contents => FileAction::Unchanged,
            Some(_) => FileAction::Updated,
            None => FileAction::Created,
        };

        if apply && action != FileAction::Unchanged {
            if let Some(parent) = path.parent()
                && let Err(err) = fs::create_dir_all(parent)
            {
                return InstallOutcome::Failed {
                    detail: format!("could not create {}: {err}", parent.display()),
                    fix: writable_fix(layout),
                };
            }
            if let Err(err) = fs::write(&path, &contents) {
                return InstallOutcome::Failed {
                    detail: format!("could not write {}: {err}", path.display()),
                    fix: writable_fix(layout),
                };
            }
        }

        changes.push(FileChange { path, action });
    }

    InstallOutcome::Ok {
        changes,
        retained: Vec::new(),
    }
}

fn uninstall(layout: &PluginLayout, apply: bool) -> InstallOutcome {
    let mut changes = Vec::new();
    for (path, _) in desired_files(layout, "") {
        let action = if path.is_file() {
            if apply && let Err(err) = fs::remove_file(&path) {
                return InstallOutcome::Failed {
                    detail: format!("could not remove {}: {err}", path.display()),
                    fix: writable_fix(layout),
                };
            }
            FileAction::Removed
        } else {
            FileAction::Absent
        };
        changes.push(FileChange { path, action });
    }

    // Prune deepest first, and only a directory that holds nothing but
    // kaibo's own entries. A directory with a stranger's file in it is
    // reported and left alone rather than removed with its contents, and
    // every directory above such a directory is left alone silently: the
    // deepest one is the whole explanation.
    let owned: HashSet<PathBuf> = changes
        .iter()
        .map(|change| change.path.clone())
        .chain(layout.owned_dirs())
        .collect();
    let mut retained: Vec<PathBuf> = Vec::new();
    for dir in layout.owned_dirs() {
        if retained.iter().any(|kept| kept.starts_with(&dir)) {
            continue;
        }
        match dir_state(&dir, &owned) {
            DirState::Missing => {}
            DirState::Foreign => retained.push(dir),
            DirState::OwnedOnly => {
                if apply && let Err(err) = fs::remove_dir(&dir) {
                    return InstallOutcome::Failed {
                        detail: format!("could not remove {}: {err}", dir.display()),
                        fix: writable_fix(layout),
                    };
                }
            }
        }
    }

    InstallOutcome::Ok { changes, retained }
}

enum DirState {
    /// Nothing at this path to prune.
    Missing,
    /// Everything in it is kaibo's own, so removing it discards nothing.
    OwnedOnly,
    /// It holds at least one entry kaibo did not install.
    Foreign,
}

fn dir_state(dir: &Path, owned: &HashSet<PathBuf>) -> DirState {
    let Ok(entries) = fs::read_dir(dir) else {
        return DirState::Missing;
    };
    for entry in entries.flatten() {
        if !owned.contains(&entry.path()) {
            return DirState::Foreign;
        }
    }
    DirState::OwnedOnly
}

// --- what `status` reads ----------------------------------------------

/// What is on disk at the install path, as facts. Whether a version counts
/// as stale is the caller's comparison to make, not this module's - see
/// [`crate::status`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum InstalledSkills {
    /// There is no install path: no home directory and no
    /// `CLAUDE_CONFIG_DIR`, so nothing can be installed or found.
    LocationUnknown,
    /// No manifest at the install path.
    Absent,
    /// A manifest is there but could not be read or parsed.
    ManifestUnreadable { detail: String },
    Present {
        version: String,
        /// Skills whose file on disk holds different bytes than the copy
        /// compiled into this binary. Content, never a timestamp: a file
        /// rewritten byte-for-byte identically is not a change, and one
        /// edited without its mtime moving still is.
        changed: Vec<String>,
        /// Skills with no file on disk at all.
        missing: Vec<String>,
    },
}

/// Read the install path and report what is there. Read-only.
pub fn inspect(config: &Config) -> InstalledSkills {
    let Some(layout) = PluginLayout::new(config) else {
        return InstalledSkills::LocationUnknown;
    };
    let manifest_path = layout.manifest_path();

    let raw = match fs::read_to_string(&manifest_path) {
        Ok(raw) => raw,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return InstalledSkills::Absent,
        Err(err) => {
            return InstalledSkills::ManifestUnreadable {
                detail: format!("{}: {err}", manifest_path.display()),
            };
        }
    };

    let version = match serde_json::from_str::<Value>(&raw) {
        Ok(value) => match value.get("version").and_then(Value::as_str) {
            Some(version) => version.to_string(),
            None => {
                return InstalledSkills::ManifestUnreadable {
                    detail: format!("{} has no `version` field", manifest_path.display()),
                };
            }
        },
        Err(err) => {
            return InstalledSkills::ManifestUnreadable {
                detail: format!("{} is not valid JSON: {err}", manifest_path.display()),
            };
        }
    };

    let mut changed = Vec::new();
    let mut missing = Vec::new();
    for skill in EMBEDDED_SKILLS {
        match fs::read_to_string(layout.skill_path(skill.name)) {
            Ok(on_disk) if on_disk == skill.source => {}
            Ok(_) => changed.push(skill.name.to_string()),
            Err(_) => missing.push(skill.name.to_string()),
        }
    }

    InstalledSkills::Present {
        version,
        changed,
        missing,
    }
}

// --- rendering ---------------------------------------------------------

impl Render for InstallReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = vec![format!(
            "kaibo {}{}",
            match self.mode {
                InstallMode::Install => "install",
                InstallMode::Uninstall => "install --uninstall",
            },
            if self.applied { "" } else { " (explain)" },
        )];
        lines.push(match &self.root {
            Some(root) => format!("plugin: {PLUGIN_NAME} at {}", root.display()),
            None => format!("plugin: {PLUGIN_NAME}, install location unknown"),
        });

        match &self.outcome {
            InstallOutcome::Failed { .. } => lines.push("result: failed".to_string()),
            InstallOutcome::Ok { changes, .. } => {
                lines.push(format!(
                    "result: {}",
                    if self.applied { "done" } else { "planned" }
                ));
                for change in changes {
                    lines.push(format!(
                        "  - {} {}",
                        change.action.as_str(),
                        change.path.display()
                    ));
                }
            }
        }

        for (message, fix) in self.findings() {
            match fix {
                Some(fix) => lines.push(format!("  - {message} -> next: `{fix}`")),
                None => lines.push(format!("  - {message}")),
            }
        }

        if options.full {
            lines.push(format!("exit code: {}", self.exit_code().code()));
        }
        lines.join("\n")
    }

    fn render_json(&self) -> Value {
        let outcome = match &self.outcome {
            InstallOutcome::Failed { detail, fix } => {
                serde_json::json!({"state": "failed", "detail": detail, "fix": fix})
            }
            InstallOutcome::Ok { changes, retained } => serde_json::json!({
                "state": "ok",
                "changes": changes.iter().map(|change| serde_json::json!({
                    "path": change.path.display().to_string(),
                    "action": change.action.as_str(),
                })).collect::<Vec<_>>(),
                "retained": retained.iter()
                    .map(|dir| dir.display().to_string())
                    .collect::<Vec<_>>(),
            }),
        };
        serde_json::json!({
            "mode": match self.mode {
                InstallMode::Install => "install",
                InstallMode::Uninstall => "uninstall",
            },
            "plugin": PLUGIN_NAME,
            "root": self.root.as_ref().map(|root| root.display().to_string()),
            "version": self.version,
            "applied": self.applied,
            "outcome": outcome,
            "exit_code": self.exit_code().code(),
        })
    }
}

#[cfg(test)]
mod tests;
