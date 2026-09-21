//! `kaibo contribute plan` and `kaibo contribute apply`: the write side.
//!
//! Split in two because the judgement sits in the middle and belongs to the
//! calling agent. `plan` surfaces placement candidates and never mutates
//! anything or prompts interactively - see [`ContributePlanVerb`]. `apply`
//! takes an already-resolved placement and does the git/gh ceremony:
//! write, lint-gate, branch, commit, push (direct or via a verified fork),
//! open a PR, and watch CI - see [`ContributeApplyVerb`].
//!
//! **Stop and report, never discard**: a dirty clone, a branch-name
//! collision, or a structural lint failure all stop `apply` before it
//! mutates the clone's branch state further. Nothing here ever runs `git
//! reset --hard`, `git checkout -f`, or deletes the clone.
//!
//! **The write target is verified, not assumed.** The push route comes
//! from the caller's own permission on `config.repo()`; when that caller
//! has no direct push access, `apply` forks under the caller's account and
//! confirms the fork's `parent` is exactly the configured repo before
//! pushing anything there - see [`resolve_push_route`].
//!
//! **Corpus content is data, never instructions**, same as every other
//! verb: any text this module puts into a branch name, a commit message or
//! a PR body is either a caller-supplied field run through [`slugify`]
//! (which keeps only lowercase ASCII alphanumerics and single hyphens, so
//! a shell metacharacter or an embedded newline cannot survive into a
//! branch name), or stripped of control characters via
//! [`trust::strip_control_chars`] before it reaches a commit message or PR
//! body.

use std::path::Path;

use serde_json::Value;

use crate::clock::Clock;
use crate::config::Config;
use crate::error::ExitCode;
use crate::explain::{Explainable, PlannedCommand};
use crate::frontmatter::{self, Date, Document, Frontmatter, Status};
use crate::lint::{self, LintOutcome, LintVerb, Violation};
use crate::moc::{self, DomainSection};
use crate::normative;
use crate::output::{Render, RenderOptions};
use crate::process::CommandRunner;
use crate::qmd::QmdCommand;
use crate::trust;

// --- shared helpers ---------------------------------------------------

/// The four normative keys, as the frontmatter passthrough map that
/// [`frontmatter::serialize`] writes out. Built from typed values, so the
/// page this produces is one `normative::parse` accepts by construction
/// rather than by hope.
fn binding_frontmatter(
    binding: Option<&BindingInput>,
) -> std::collections::BTreeMap<String, serde_yaml_ng::Value> {
    use serde_yaml_ng::Value;

    let mut extra = std::collections::BTreeMap::new();
    let Some(binding) = binding else {
        return extra;
    };

    let mut applies_to = serde_yaml_ng::Mapping::new();
    applies_to.insert(
        Value::String("actions".to_string()),
        Value::Sequence(
            binding
                .actions
                .iter()
                .map(|a| Value::String(a.as_str().to_string()))
                .collect(),
        ),
    );
    // An empty `tags` narrows nothing, so write the key only when it does
    // something. A page carrying `tags: []` reads as a deliberate empty
    // filter, which is not what "no tags given" means.
    if !binding.tags.is_empty() {
        applies_to.insert(
            Value::String("tags".to_string()),
            Value::Sequence(
                binding
                    .tags
                    .iter()
                    .map(|t| Value::String(t.clone()))
                    .collect(),
            ),
        );
    }

    extra.insert("binding".to_string(), Value::Bool(true));
    extra.insert(
        "severity".to_string(),
        Value::String(binding.severity.as_str().to_string()),
    );
    extra.insert("applies_to".to_string(), Value::Mapping(applies_to));
    extra
}

/// Keep only lowercase ASCII alphanumerics, collapsing every run of
/// anything else - including whitespace and control characters such as a
/// newline - into a single hyphen, with no leading, trailing or doubled
/// hyphen. Used for both the candidate/target path a page would take and
/// the `contribute/<slug>` branch name, so neither can ever contain a
/// shell metacharacter, a path separator, or a newline, regardless of what
/// the input contained: every character that is not itself pushed is a
/// hyphen boundary, so nothing needs a separate control-character pass.
pub(crate) fn slugify(input: &str) -> String {
    let mut slug = String::new();
    let mut last_was_hyphen = true; // suppresses a leading hyphen
    for ch in input.chars() {
        if ch.is_ascii_alphanumeric() {
            slug.push(ch.to_ascii_lowercase());
            last_was_hyphen = false;
        } else if !last_was_hyphen {
            slug.push('-');
            last_was_hyphen = true;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    slug
}

/// A path segment (`domain`, `content_type`) is simple: non-empty, no
/// separator, no control character, not `.` or `..`. `title`/`body`
/// text never goes through this - only fields that become directory
/// names go through this check.
fn is_simple_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && !s.contains('/')
        && !s.contains('\\')
        && !s.chars().any(|c| c.is_control())
}

fn git(clone_path: &Path, args: impl IntoIterator<Item = impl Into<String>>) -> PlannedCommand {
    let mut full = vec!["-C".to_string(), clone_path.to_string_lossy().into_owned()];
    full.extend(args.into_iter().map(Into::into));
    PlannedCommand::new("git", full)
}

/// Every git command below carries `-c core.hooksPath=/dev/null`: a
/// knowledge repo must never execute code on this machine, same rule
/// `sync` follows for `clone`/`checkout`/`pull`.
fn git_no_hooks(
    clone_path: &Path,
    args: impl IntoIterator<Item = impl Into<String>>,
) -> PlannedCommand {
    let mut full = vec![
        "-C".to_string(),
        clone_path.to_string_lossy().into_owned(),
        "-c".to_string(),
        "core.hooksPath=/dev/null".to_string(),
    ];
    full.extend(args.into_iter().map(Into::into));
    PlannedCommand::new("git", full)
}

fn git_status_porcelain(clone_path: &Path) -> PlannedCommand {
    git(clone_path, ["status", "--porcelain"])
}

fn git_branch_list(clone_path: &Path, branch: &str) -> PlannedCommand {
    git(clone_path, ["branch", "--list", branch])
}

fn git_checkout_new_branch(clone_path: &Path, branch: &str) -> PlannedCommand {
    git_no_hooks(clone_path, ["checkout", "-b", branch])
}

fn git_checkout_main(clone_path: &Path) -> PlannedCommand {
    git_no_hooks(clone_path, ["checkout", "main"])
}

fn git_add(clone_path: &Path, repo_relative_path: &str) -> PlannedCommand {
    git_no_hooks(clone_path, ["add", "--", repo_relative_path])
}

fn git_commit(clone_path: &Path, message: &str) -> PlannedCommand {
    git_no_hooks(clone_path, ["commit", "-m", message])
}

fn git_push(clone_path: &Path, remote: &str, branch: &str) -> PlannedCommand {
    git_no_hooks(clone_path, ["push", remote, branch])
}

fn git_remote_add(clone_path: &Path, name: &str, url: &str) -> PlannedCommand {
    git(clone_path, ["remote", "add", name, url])
}

/// `gh api repos/<repo> --jq .permissions.push`: whether the caller (as
/// authenticated to `gh`) has push access to the configured repo. This is
/// what decides the push route - never a flag, never corpus content.
fn gh_permission_check(repo: &str) -> PlannedCommand {
    PlannedCommand::new(
        "gh",
        ["api", &format!("repos/{repo}"), "--jq", ".permissions.push"],
    )
}

fn gh_whoami() -> PlannedCommand {
    PlannedCommand::new("gh", ["api", "user", "--jq", ".login"])
}

fn gh_fork(repo: &str) -> PlannedCommand {
    PlannedCommand::new(
        "gh",
        ["repo", "fork", repo, "--clone=false", "--remote=false"],
    )
}

/// `gh api repos/<owner>/<name> --jq .parent.full_name` - the check that
/// makes a fork whose parent is not the configured repo a stop condition
/// rather than a push destination.
fn gh_fork_parent(owner: &str, name: &str) -> PlannedCommand {
    PlannedCommand::new(
        "gh",
        [
            "api",
            &format!("repos/{owner}/{name}"),
            "--jq",
            ".parent.full_name",
        ],
    )
}

fn gh_pr_create(repo: &str, base: &str, head: &str, title: &str, body: &str) -> PlannedCommand {
    PlannedCommand::new(
        "gh",
        [
            "pr", "create", "--repo", repo, "--base", base, "--head", head, "--title", title,
            "--body", body,
        ],
    )
}

fn gh_pr_checks(repo: &str, pr_url_or_number: &str) -> PlannedCommand {
    PlannedCommand::new(
        "gh",
        ["pr", "checks", pr_url_or_number, "--repo", repo, "--watch"],
    )
}

fn repo_owner_and_name(repo: &str) -> Option<(&str, &str)> {
    repo.split_once('/')
}

// --- `kaibo contribute plan` -------------------------------------------

/// One existing page the dedup probe turned up, ranked by qmd's own score.
#[derive(Debug, Clone, PartialEq)]
pub struct Candidate {
    pub path: String,
    pub score: f64,
    /// Set when the page is a binding standard, so the caller can see what
    /// it would be contradicting before it files another one. kaibo
    /// prohibits conflicting binding standards, and a contradiction is a
    /// judgment `plan` cannot make: this is the material for it, not the
    /// verdict.
    ///
    /// `None` also covers a page that could not be read or whose
    /// frontmatter is malformed. That is a softer failure than it looks:
    /// a page kaibo cannot parse is one `kaibo lint` already refuses, and
    /// the batch integrity job is the gate. This is the nudge.
    pub binding: Option<CandidateBinding>,
}

/// What a candidate page binds, as much of it as matters for spotting a
/// contradiction: what it is about to be applied to, and how hard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CandidateBinding {
    pub severity: normative::Severity,
    pub actions: Vec<normative::ActionKind>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum PlanOutcome {
    /// The clone or its root MOC could not be read: a corpus problem, not a
    /// gap in the knowledge itself.
    CorpusUnavailable { detail: String },
    /// The MOC read fine (possibly with zero domains) and candidates were
    /// gathered - `qmd` being unreachable degrades `candidates` to empty
    /// rather than failing the whole plan, since a dedup probe is a nice
    /// to have, not a precondition for showing the domain inventory.
    Ready {
        domains: Vec<DomainSection>,
        candidates: Vec<Candidate>,
        qmd_unavailable: Option<String>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlanReport {
    pub gist: String,
    pub content_type: Option<String>,
    pub domain: Option<String>,
    /// The path a new page would take, once both `content_type` and
    /// `domain` are known. `None` is the ambiguity signal: `plan` never
    /// guesses either field.
    pub target_path: Option<String>,
    pub ambiguities: Vec<String>,
    pub outcome: PlanOutcome,
}

impl PlanReport {
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            PlanOutcome::CorpusUnavailable { .. } => ExitCode::Stale,
            PlanOutcome::Ready { .. } => ExitCode::Success,
        }
    }
}

/// The `kaibo contribute plan` verb, bound to a resolved `Config`. Never
/// mutates anything and never prompts: an unresolved `content_type` or
/// `domain` is returned as an ambiguity for the calling agent to ask the
/// user about, not guessed.
pub struct ContributePlanVerb<'a> {
    config: &'a Config,
    gist: String,
    content_type: Option<String>,
    domain: Option<String>,
}

impl<'a> ContributePlanVerb<'a> {
    pub fn new(
        config: &'a Config,
        gist: impl Into<String>,
        content_type: Option<String>,
        domain: Option<String>,
    ) -> Self {
        Self {
            config,
            gist: gist.into(),
            content_type,
            domain,
        }
    }

    /// Read-only: reads the root MOC and runs one `qmd query`. Never
    /// writes, never touches git, never touches `gh`.
    pub fn gather(&self, runner: &dyn CommandRunner) -> PlanReport {
        gather_plan(
            self.config,
            &self.gist,
            &self.content_type,
            &self.domain,
            runner,
        )
    }
}

impl Explainable for ContributePlanVerb<'_> {
    fn explain(&self) -> Vec<PlannedCommand> {
        if !self.config.clone_path().is_dir() {
            return Vec::new();
        }
        vec![QmdCommand::query(self.config, &self.gist)]
    }
}

fn gather_plan(
    config: &Config,
    gist: &str,
    content_type: &Option<String>,
    domain: &Option<String>,
    runner: &dyn CommandRunner,
) -> PlanReport {
    let mut ambiguities = Vec::new();
    if content_type.is_none() {
        ambiguities.push(
            "content type is not resolved; classify the gist yourself and pass --type".to_string(),
        );
    }
    if domain.is_none() {
        ambiguities.push(
            "domain is not resolved; pick one from the domain inventory below and pass --domain"
                .to_string(),
        );
    }

    let outcome = if !config.clone_path().is_dir() {
        PlanOutcome::CorpusUnavailable {
            detail: format!(
                "{} does not exist; run `kaibo sync`",
                config.clone_path().display()
            ),
        }
    } else {
        match moc::read_domain_sections(config.clone_path()) {
            Err(moc::MocUnreadable { detail }) => PlanOutcome::CorpusUnavailable { detail },
            Ok(domains) => {
                let (candidates, qmd_unavailable) = probe_candidates(config, gist, runner);
                PlanOutcome::Ready {
                    domains,
                    candidates,
                    qmd_unavailable,
                }
            }
        }
    };

    let target_path = match (content_type, domain) {
        (Some(content_type), Some(domain))
            if is_simple_segment(content_type) && is_simple_segment(domain) =>
        {
            let slug = slugify(gist);
            if slug.is_empty() {
                None
            } else {
                Some(format!("{domain}/{content_type}/{slug}.md"))
            }
        }
        _ => None,
    };

    PlanReport {
        gist: gist.to_string(),
        content_type: content_type.clone(),
        domain: domain.clone(),
        target_path,
        ambiguities,
        outcome,
    }
}

/// Best-effort dedup probe against the configured qmd index: a `qmd`
/// that is unreachable degrades to an empty candidate list plus a detail
/// message, rather than failing `plan` outright.
fn probe_candidates(
    config: &Config,
    gist: &str,
    runner: &dyn CommandRunner,
) -> (Vec<Candidate>, Option<String>) {
    let output = match runner.run(&QmdCommand::query(config, gist)) {
        Ok(output) if output.success() => output,
        Ok(output) => return (Vec::new(), Some(output.stderr.trim().to_string())),
        Err(err) => return (Vec::new(), Some(err.to_string())),
    };

    let raw: Vec<Value> = match serde_json::from_str(&output.stdout) {
        Ok(raw) => raw,
        Err(err) => return (Vec::new(), Some(err.to_string())),
    };

    let candidates = raw
        .into_iter()
        .filter_map(|value| {
            let score = value.get("score")?.as_f64()?;
            let file = value.get("file")?.as_str()?;
            let path = trust::repo_relative_path(file)?;
            let binding = binding_of(config.clone_path(), &path);
            Some(Candidate {
                path,
                score,
                binding,
            })
        })
        .collect();

    (candidates, None)
}

/// Read `path` out of the clone and say what it binds, if anything.
///
/// Every failure reads as "not known to be binding": an unreadable file, a
/// frontmatter block that does not parse, a half-declared standard. None of
/// those is a page `plan` should be inventing a claim about.
fn binding_of(clone_path: &Path, path: &str) -> Option<CandidateBinding> {
    let contents = std::fs::read_to_string(clone_path.join(path)).ok()?;
    let doc = frontmatter::parse(&contents).ok()?;
    let standard = normative::parse(&doc.frontmatter, &doc.body).ok()??;
    Some(CandidateBinding {
        severity: standard.severity,
        actions: standard.applies_to.actions,
    })
}

// --- `kaibo contribute apply` ------------------------------------------

/// Whether `apply` creates a new page or appends to an existing one. The
/// existing-page path is caller-supplied, corpus-shaped input (like
/// `lint`'s `path` argument) - never a flag naming the repo, clone, index
/// or collection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Placement {
    Create,
    Append { path: String },
}

/// The normative keys a new page carries when the contribution is a
/// binding standard. Present or absent as a whole, never partly: the
/// schema's all-or-nothing rule (see [`crate::normative`]) is the reason
/// this is one struct rather than four optional fields that could disagree.
///
/// Typed rather than stringly, so an unknown severity or action is refused
/// at the argument surface instead of being written into a page that
/// `kaibo lint` then rejects on the contributor's behalf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingInput {
    pub severity: normative::Severity,
    pub actions: Vec<normative::ActionKind>,
    pub tags: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ApplyInput {
    pub content_type: String,
    pub domain: String,
    pub title: String,
    pub body: String,
    pub tags: Vec<String>,
    pub placement: Placement,
    /// Set only on [`Placement::Create`]. Appending to an existing page
    /// stops rather than writing these, because a second claim bolted onto
    /// a page that already binds one is exactly the atomicity failure
    /// `normative-atomicity` exists to find.
    pub binding: Option<BindingInput>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PushRouteKind {
    Direct,
    Fork,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PushRoute {
    pub kind: PushRouteKind,
    /// The fork owner login, when `kind` is `Fork`.
    pub owner: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CiVerdict {
    Passed,
    Failed { detail: String },
}

/// Why `apply` stopped, or a step it could not complete. Every variant
/// reports with a message naming the exact next command, via
/// [`ApplyReport::findings`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ApplyStop {
    RepoNotConfigured,
    CloneMissing,
    CloneUnreadable { detail: String },
    UncommittedChanges { detail: String },
    InvalidField { field: &'static str, value: String },
    BindingOnAppend { path: String },
    AppendTargetUnreadable { path: String },
    WriteFailed { detail: String },
    LintFailed { violations: Vec<Violation> },
    BranchCollision { branch: String },
    CheckoutFailed { detail: String },
    AddFailed { detail: String },
    CommitFailed { detail: String },
    PermissionCheckFailed { detail: String },
    WhoamiFailed { detail: String },
    ForkFailed { detail: String },
    ForkParentMismatch { expected: String, actual: String },
    RemoteAddFailed { detail: String },
    PushFailed { detail: String },
    PrCreateFailed { detail: String },
}

impl ApplyStop {
    fn exit_code(&self) -> ExitCode {
        match self {
            ApplyStop::RepoNotConfigured => ExitCode::Usage,
            ApplyStop::CloneMissing
            | ApplyStop::CloneUnreadable { .. }
            | ApplyStop::UncommittedChanges { .. } => ExitCode::Stale,
            _ => ExitCode::Usage,
        }
    }

    fn finding(&self, clone_display: &str) -> (String, Option<String>) {
        match self {
            ApplyStop::RepoNotConfigured => (
                "no corpus repo configured".to_string(),
                Some("set KAIBO_REPO=<owner>/<name>, then re-run `kaibo contribute apply`".to_string()),
            ),
            ApplyStop::CloneMissing => (
                format!("{clone_display} does not exist"),
                Some("run `kaibo sync`".to_string()),
            ),
            ApplyStop::CloneUnreadable { detail } => (
                format!("could not read the status of {clone_display}: {detail}"),
                Some(format!("inspect {clone_display} by hand, then re-run `kaibo contribute apply`")),
            ),
            ApplyStop::UncommittedChanges { detail } => (
                format!("uncommitted changes in {clone_display}: {detail}"),
                Some(format!("commit or stash your changes in {clone_display}, then re-run `kaibo contribute apply`")),
            ),
            ApplyStop::InvalidField { field, value } => (
                format!("{field} {value:?} is not a valid path segment"),
                Some(format!("pass a plain single-segment {field}, no `/`, `..` or control characters")),
            ),
            ApplyStop::BindingOnAppend { path } => (
                format!("a binding standard cannot be appended to {path}"),
                Some("re-run `kaibo contribute apply` without --append: one page, one normative claim, one verdict".to_string()),
            ),
            ApplyStop::AppendTargetUnreadable { path } => (
                format!("append target {path} is not a readable file contained in the clone"),
                Some("pass an existing repo-relative path returned by `kaibo contribute plan`".to_string()),
            ),
            ApplyStop::WriteFailed { detail } => (
                format!("failed to write the page: {detail}"),
                None,
            ),
            ApplyStop::LintFailed { violations } => (
                format!("`kaibo lint` found {} structural violation(s) on the written page", violations.len()),
                Some(format!(
                    "fix the reported violation(s) in {clone_display}, or revert the write with \
                     `git -C {clone_display} checkout -- <path>`, then re-run `kaibo contribute apply`"
                )),
            ),
            ApplyStop::BranchCollision { branch } => (
                format!("branch {branch} already exists"),
                Some(format!("delete or rename the existing branch in {clone_display}, or change the title, then re-run")),
            ),
            ApplyStop::CheckoutFailed { detail } => (
                format!("git checkout failed: {detail}"),
                Some(format!("inspect {clone_display} by hand")),
            ),
            ApplyStop::AddFailed { detail } => (format!("git add failed: {detail}"), None),
            ApplyStop::CommitFailed { detail } => (format!("git commit failed: {detail}"), None),
            ApplyStop::PermissionCheckFailed { detail } => (
                format!("could not check push permission on the configured repo: {detail}"),
                Some("check `gh auth status`, then re-run `kaibo contribute apply`".to_string()),
            ),
            ApplyStop::WhoamiFailed { detail } => (
                format!("could not determine the authenticated gh user: {detail}"),
                Some("check `gh auth status`, then re-run `kaibo contribute apply`".to_string()),
            ),
            ApplyStop::ForkFailed { detail } => (
                format!("could not fork the configured repo: {detail}"),
                Some("check `gh auth status` and network access, then re-run `kaibo contribute apply`".to_string()),
            ),
            ApplyStop::ForkParentMismatch { expected, actual } => (
                format!("the fork's parent is {actual}, not the configured repo {expected}"),
                Some("delete or rename the conflicting fork under your account, then re-run `kaibo contribute apply`".to_string()),
            ),
            ApplyStop::RemoteAddFailed { detail } => (format!("could not add the fork remote: {detail}"), None),
            ApplyStop::PushFailed { detail } => (
                format!("git push failed: {detail}"),
                Some("check network access and repo permissions, then re-run `kaibo contribute apply`".to_string()),
            ),
            ApplyStop::PrCreateFailed { detail } => (
                format!("gh pr create failed: {detail}"),
                Some("check `gh auth status`, then re-run `kaibo contribute apply`".to_string()),
            ),
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum ApplyOutcome {
    Stopped(ApplyStop),
    Completed {
        push_route: PushRoute,
        pr_url: String,
        ci: CiVerdict,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ApplyReport {
    pub path: String,
    pub branch: String,
    /// The clone path this run acted against, for display in a finding's
    /// message - kept as its own field rather than reconstructed from
    /// `Config` at render time, since `Render::render_text` has no access
    /// to `Config`.
    pub clone_display: String,
    /// `None` until a branch was actually created; `Some(false)` means the
    /// return-to-main checkout itself failed after branching, which is
    /// reported as its own finding regardless of the primary outcome.
    pub returned_to_main: Option<bool>,
    pub outcome: ApplyOutcome,
}

impl ApplyReport {
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            ApplyOutcome::Stopped(stop) => stop.exit_code(),
            ApplyOutcome::Completed {
                ci: CiVerdict::Failed { .. },
                ..
            } => ExitCode::Usage,
            ApplyOutcome::Completed { .. } => ExitCode::Success,
        }
    }

    pub fn findings(&self) -> Vec<(String, Option<String>)> {
        let clone_display = self.clone_display.as_str();
        let mut findings = Vec::new();
        if let ApplyOutcome::Stopped(stop) = &self.outcome {
            findings.push(stop.finding(clone_display));
        }
        if let ApplyOutcome::Completed {
            ci: CiVerdict::Failed { detail },
            pr_url,
            ..
        } = &self.outcome
        {
            findings.push((
                format!("CI failed on {pr_url}: {detail}"),
                Some(format!(
                    "inspect {pr_url}, push a fix to {}, and re-check",
                    self.branch
                )),
            ));
        }
        if self.returned_to_main == Some(false) {
            findings.push((
                format!("{clone_display} could not be returned to `main` after branching"),
                Some(format!(
                    "run `git -C {clone_display} checkout main` by hand"
                )),
            ));
        }
        findings
    }
}

pub struct ContributeApplyVerb<'a> {
    config: &'a Config,
    input: ApplyInput,
}

impl<'a> ContributeApplyVerb<'a> {
    pub fn new(config: &'a Config, input: ApplyInput) -> Self {
        Self { config, input }
    }

    pub fn apply(&self, runner: &dyn CommandRunner, clock: &dyn Clock) -> ApplyReport {
        apply(self.config, &self.input, runner, clock)
    }
}

impl Explainable for ContributeApplyVerb<'_> {
    fn explain(&self) -> Vec<PlannedCommand> {
        planned_apply_commands(self.config, &self.input)
    }
}

/// The full command sequence `apply` could run, for `--explain`. Both the
/// direct-push and the fork route are shown, since which one a real run
/// takes depends on a permission check `--explain` must not perform;
/// `<fork-owner>` is a placeholder for a value only `gh` can supply.
fn planned_apply_commands(config: &Config, input: &ApplyInput) -> Vec<PlannedCommand> {
    let clone_path = config.clone_path();
    let path = target_path(input);
    let branch = format!("contribute/{}", slugify(&input.title));
    let repo = config.repo().unwrap_or("<repo>").to_string();
    let message = commit_message(input);
    let title = trust::strip_control_chars(&input.title);
    let body = pr_body(input);

    let commands = vec![
        git_status_porcelain(clone_path),
        git_branch_list(clone_path, &branch),
        git_checkout_new_branch(clone_path, &branch),
        git_add(clone_path, &path),
        git_commit(clone_path, &message),
        gh_permission_check(&repo),
        git_push(clone_path, "origin", &branch),
        gh_whoami(),
        gh_fork(&repo),
        gh_fork_parent(
            "<fork-owner>",
            repo_owner_and_name(&repo)
                .map(|(_, n)| n)
                .unwrap_or("<repo>"),
        ),
        git_remote_add(
            clone_path,
            "contribute-fork",
            "https://github.com/<fork-owner>/<repo>.git",
        ),
        git_push(clone_path, "contribute-fork", &branch),
        gh_pr_create(
            &repo,
            "main",
            &format!("<fork-owner>:{branch}"),
            &title,
            &body,
        ),
        gh_pr_checks(&repo, "<pr-url>"),
        git_checkout_main(clone_path),
    ];
    // The write itself is a plain file write, not a shelled-out command,
    // so `Placement::Create` vs `Placement::Append` changes only `path`
    // above, not this command list.
    commands
}

fn target_path(input: &ApplyInput) -> String {
    match &input.placement {
        Placement::Append { path } => path.clone(),
        Placement::Create => format!(
            "{}/{}/{}.md",
            input.domain,
            input.content_type,
            slugify(&input.title)
        ),
    }
}

fn commit_message(input: &ApplyInput) -> String {
    let title = trust::strip_control_chars(&input.title);
    match &input.placement {
        Placement::Create => format!("contribute: add {title}"),
        Placement::Append { .. } => format!("contribute: update {title}"),
    }
}

fn pr_body(input: &ApplyInput) -> String {
    let title = trust::strip_control_chars(&input.title);
    let domain = trust::strip_control_chars(&input.domain);
    let content_type = trust::strip_control_chars(&input.content_type);
    let decision = match &input.placement {
        Placement::Create => "created a new page".to_string(),
        Placement::Append { path } => format!("appended to {}", trust::strip_control_chars(path)),
    };
    format!("Captures: {title}\n\nType: {content_type}\nDomain: {domain}\nDecision: {decision}")
}

fn apply(
    config: &Config,
    input: &ApplyInput,
    runner: &dyn CommandRunner,
    clock: &dyn Clock,
) -> ApplyReport {
    let path = target_path(input);
    let branch = format!("contribute/{}", slugify(&input.title));
    let clone_path = config.clone_path();

    let clone_display = clone_path.display().to_string();
    macro_rules! stop {
        ($stop:expr) => {
            return ApplyReport {
                path: path.clone(),
                branch: branch.clone(),
                clone_display: clone_display.clone(),
                returned_to_main: None,
                outcome: ApplyOutcome::Stopped($stop),
            }
        };
    }

    let Some(repo) = config.repo() else {
        stop!(ApplyStop::RepoNotConfigured);
    };
    let repo = repo.to_string();

    if !clone_path.is_dir() {
        stop!(ApplyStop::CloneMissing);
    }

    if !is_simple_segment(&input.domain) {
        stop!(ApplyStop::InvalidField {
            field: "domain",
            value: input.domain.clone(),
        });
    }
    if !is_simple_segment(&input.content_type) {
        stop!(ApplyStop::InvalidField {
            field: "content_type",
            value: input.content_type.clone(),
        });
    }
    if let (Some(_), Placement::Append { path }) = (&input.binding, &input.placement) {
        stop!(ApplyStop::BindingOnAppend { path: path.clone() });
    }
    let slug = slugify(&input.title);
    if slug.is_empty() {
        stop!(ApplyStop::InvalidField {
            field: "title",
            value: input.title.clone(),
        });
    }

    let status_output = match runner.run(&git_status_porcelain(clone_path)) {
        Ok(output) if output.success() => output,
        Ok(output) => stop!(ApplyStop::CloneUnreadable {
            detail: output.stderr.trim().to_string(),
        }),
        Err(err) => stop!(ApplyStop::CloneUnreadable {
            detail: err.to_string(),
        }),
    };
    if !status_output.stdout.trim().is_empty() {
        stop!(ApplyStop::UncommittedChanges {
            detail: status_output.stdout.trim().to_string(),
        });
    }

    let full_path = match &input.placement {
        Placement::Append { path: append_path } => {
            match trust::resolve_contained_path(config, append_path) {
                Some(full) if full.is_file() => full,
                _ => stop!(ApplyStop::AppendTargetUnreadable {
                    path: append_path.clone(),
                }),
            }
        }
        Placement::Create => clone_path.join(&path),
    };

    let today = today(clock);
    let new_contents = match &input.placement {
        Placement::Create => {
            let doc = Document {
                frontmatter: Frontmatter {
                    doc_type: Some(input.content_type.clone()),
                    title: Some(input.title.clone()),
                    tags: Some(input.tags.clone()),
                    status: Some(Status::Draft),
                    updated: Some(today),
                    extra: binding_frontmatter(input.binding.as_ref()),
                },
                body: input.body.clone(),
            };
            match frontmatter::serialize(&doc) {
                Ok(contents) => contents,
                Err(err) => stop!(ApplyStop::WriteFailed {
                    detail: err.to_string()
                }),
            }
        }
        Placement::Append { .. } => {
            let existing = match std::fs::read_to_string(&full_path) {
                Ok(contents) => contents,
                Err(err) => stop!(ApplyStop::WriteFailed {
                    detail: err.to_string()
                }),
            };
            let mut doc = match frontmatter::parse(&existing) {
                Ok(doc) => doc,
                Err(err) => stop!(ApplyStop::WriteFailed {
                    detail: err.to_string()
                }),
            };
            doc.frontmatter.updated = Some(today);
            doc.body = format!("{}\n\n{}", doc.body.trim_end(), input.body);
            match frontmatter::serialize(&doc) {
                Ok(contents) => contents,
                Err(err) => stop!(ApplyStop::WriteFailed {
                    detail: err.to_string()
                }),
            }
        }
    };

    if let Placement::Create = &input.placement
        && let Some(parent) = full_path.parent()
        && let Err(err) = std::fs::create_dir_all(parent)
    {
        stop!(ApplyStop::WriteFailed {
            detail: err.to_string()
        });
    }
    if let Err(err) = std::fs::write(&full_path, &new_contents) {
        stop!(ApplyStop::WriteFailed {
            detail: err.to_string()
        });
    }

    let lint_report = LintVerb::new(config, vec![path.clone()]).gather();
    if let LintOutcome::Finished { violations, .. } = &lint_report.outcome {
        let structural: Vec<Violation> = violations
            .iter()
            .filter(|v| v.severity == lint::Severity::Structural)
            .cloned()
            .collect();
        if !structural.is_empty() {
            stop!(ApplyStop::LintFailed {
                violations: structural
            });
        }
    } else if !matches!(lint_report.outcome, LintOutcome::NoFilesFound) {
        stop!(ApplyStop::WriteFailed {
            detail: "kaibo lint could not check the written page".to_string(),
        });
    }

    let branch_list_output = match runner.run(&git_branch_list(clone_path, &branch)) {
        Ok(output) if output.success() => output,
        Ok(output) => stop!(ApplyStop::CheckoutFailed {
            detail: output.stderr.trim().to_string(),
        }),
        Err(err) => stop!(ApplyStop::CheckoutFailed {
            detail: err.to_string()
        }),
    };
    if !branch_list_output.stdout.trim().is_empty() {
        stop!(ApplyStop::BranchCollision {
            branch: branch.clone()
        });
    }

    // From here on the clone is (about to be) on a feature branch: every
    // exit past this point must still return to `main`.
    let result = run_from_branch(config, input, &repo, &path, &branch, runner);
    let checkout_back = runner.run(&git_checkout_main(clone_path));
    let returned_to_main = Some(matches!(&checkout_back, Ok(output) if output.success()));

    match result {
        Ok((push_route, pr_url, ci)) => ApplyReport {
            path,
            branch,
            clone_display,
            returned_to_main,
            outcome: ApplyOutcome::Completed {
                push_route,
                pr_url,
                ci,
            },
        },
        Err(stop) => ApplyReport {
            path,
            branch,
            clone_display,
            returned_to_main,
            outcome: ApplyOutcome::Stopped(stop),
        },
    }
}

fn run_from_branch(
    config: &Config,
    input: &ApplyInput,
    repo: &str,
    path: &str,
    branch: &str,
    runner: &dyn CommandRunner,
) -> Result<(PushRoute, String, CiVerdict), ApplyStop> {
    let clone_path = config.clone_path();

    let checkout = runner
        .run(&git_checkout_new_branch(clone_path, branch))
        .map_err(|err| ApplyStop::CheckoutFailed {
            detail: err.to_string(),
        })?;
    if !checkout.success() {
        return Err(ApplyStop::CheckoutFailed {
            detail: checkout.stderr.trim().to_string(),
        });
    }

    let add = runner
        .run(&git_add(clone_path, path))
        .map_err(|err| ApplyStop::AddFailed {
            detail: err.to_string(),
        })?;
    if !add.success() {
        return Err(ApplyStop::AddFailed {
            detail: add.stderr.trim().to_string(),
        });
    }

    let commit = runner
        .run(&git_commit(clone_path, &commit_message(input)))
        .map_err(|err| ApplyStop::CommitFailed {
            detail: err.to_string(),
        })?;
    if !commit.success() {
        return Err(ApplyStop::CommitFailed {
            detail: commit.stderr.trim().to_string(),
        });
    }

    let (push_route, head) = resolve_push_route(config, repo, branch, runner)?;

    let title = trust::strip_control_chars(&input.title);
    let body = pr_body(input);
    let pr_output = runner
        .run(&gh_pr_create(repo, "main", &head, &title, &body))
        .map_err(|err| ApplyStop::PrCreateFailed {
            detail: err.to_string(),
        })?;
    if !pr_output.success() {
        return Err(ApplyStop::PrCreateFailed {
            detail: pr_output.stderr.trim().to_string(),
        });
    }
    let pr_url = pr_output
        .stdout
        .lines()
        .last()
        .unwrap_or_default()
        .trim()
        .to_string();

    let ci_output =
        runner
            .run(&gh_pr_checks(repo, &pr_url))
            .map_err(|err| ApplyStop::PrCreateFailed {
                detail: err.to_string(),
            })?;
    let ci = if ci_output.success() {
        CiVerdict::Passed
    } else {
        CiVerdict::Failed {
            detail: ci_output.stderr.trim().to_string(),
        }
    };

    Ok((push_route, pr_url, ci))
}

/// Resolve where `apply` pushes, from the caller's own permission on
/// `config.repo()` - never from a flag, never from corpus content. A fork
/// whose `parent` is not exactly the configured repo is a stop condition.
fn resolve_push_route(
    config: &Config,
    repo: &str,
    branch: &str,
    runner: &dyn CommandRunner,
) -> Result<(PushRoute, String), ApplyStop> {
    let clone_path = config.clone_path();

    let permission =
        runner
            .run(&gh_permission_check(repo))
            .map_err(|err| ApplyStop::PermissionCheckFailed {
                detail: err.to_string(),
            })?;
    if !permission.success() {
        return Err(ApplyStop::PermissionCheckFailed {
            detail: permission.stderr.trim().to_string(),
        });
    }

    if permission.stdout.trim() == "true" {
        let push = runner
            .run(&git_push(clone_path, "origin", branch))
            .map_err(|err| ApplyStop::PushFailed {
                detail: err.to_string(),
            })?;
        if !push.success() {
            return Err(ApplyStop::PushFailed {
                detail: push.stderr.trim().to_string(),
            });
        }
        return Ok((
            PushRoute {
                kind: PushRouteKind::Direct,
                owner: None,
            },
            branch.to_string(),
        ));
    }

    let whoami = runner
        .run(&gh_whoami())
        .map_err(|err| ApplyStop::WhoamiFailed {
            detail: err.to_string(),
        })?;
    if !whoami.success() {
        return Err(ApplyStop::WhoamiFailed {
            detail: whoami.stderr.trim().to_string(),
        });
    }
    let login = whoami.stdout.trim().to_string();

    let fork = runner
        .run(&gh_fork(repo))
        .map_err(|err| ApplyStop::ForkFailed {
            detail: err.to_string(),
        })?;
    if !fork.success() {
        return Err(ApplyStop::ForkFailed {
            detail: fork.stderr.trim().to_string(),
        });
    }

    let Some((_, repo_name)) = repo_owner_and_name(repo) else {
        return Err(ApplyStop::ForkFailed {
            detail: format!("configured repo {repo} is not owner/name"),
        });
    };

    let parent_check = runner
        .run(&gh_fork_parent(&login, repo_name))
        .map_err(|err| ApplyStop::ForkFailed {
            detail: err.to_string(),
        })?;
    if !parent_check.success() {
        return Err(ApplyStop::ForkFailed {
            detail: parent_check.stderr.trim().to_string(),
        });
    }
    let actual_parent = parent_check.stdout.trim().to_string();
    if actual_parent != repo {
        return Err(ApplyStop::ForkParentMismatch {
            expected: repo.to_string(),
            actual: actual_parent,
        });
    }

    let fork_url = format!("https://github.com/{login}/{repo_name}.git");
    let remote_add = runner
        .run(&git_remote_add(clone_path, "contribute-fork", &fork_url))
        .map_err(|err| ApplyStop::RemoteAddFailed {
            detail: err.to_string(),
        })?;
    if !remote_add.success() {
        return Err(ApplyStop::RemoteAddFailed {
            detail: remote_add.stderr.trim().to_string(),
        });
    }

    let push = runner
        .run(&git_push(clone_path, "contribute-fork", branch))
        .map_err(|err| ApplyStop::PushFailed {
            detail: err.to_string(),
        })?;
    if !push.success() {
        return Err(ApplyStop::PushFailed {
            detail: push.stderr.trim().to_string(),
        });
    }

    Ok((
        PushRoute {
            kind: PushRouteKind::Fork,
            owner: Some(login.clone()),
        },
        format!("{login}:{branch}"),
    ))
}

fn today(clock: &dyn Clock) -> Date {
    let secs = clock
        .now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = secs / 86_400;
    // Civil-from-days, Howard Hinnant's algorithm: no date/time dependency
    // for a crate that only ever needs "today" as Y-M-D. The reference
    // algorithm branches on `z < 0` to support dates before 0000-03-01, but
    // `z` can never be negative here: `secs` is a `u64`, so `days` is at
    // most `u64::MAX / 86_400`, far below `i64::MAX`, and adding `719_468`
    // only pushes it further positive. That branch is dropped rather than
    // kept unreachable.
    let z = days as i64 + 719_468;
    let era = z / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u8;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u8;
    let y = if m <= 2 { y + 1 } else { y };
    Date {
        year: y as u16,
        month: m,
        day: d,
    }
}

// --- rendering -----------------------------------------------------------

impl Render for PlanReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = vec!["kaibo contribute plan".to_string()];
        lines.push(format!(
            "type: {}",
            self.content_type.as_deref().unwrap_or("unresolved")
        ));
        lines.push(format!(
            "domain: {}",
            self.domain.as_deref().unwrap_or("unresolved")
        ));
        lines.push(format!(
            "target path: {}",
            self.target_path.as_deref().unwrap_or("unresolved")
        ));

        match &self.outcome {
            PlanOutcome::CorpusUnavailable { detail } => {
                lines.push(format!("result: corpus unavailable ({detail})"));
            }
            PlanOutcome::Ready {
                domains,
                candidates,
                qmd_unavailable,
            } => {
                lines.push(format!("domains: {}", domains.len()));
                if let Some(detail) = qmd_unavailable {
                    lines.push(format!("candidates: unavailable ({detail})"));
                } else {
                    lines.push(format!("candidates: {}", candidates.len()));
                    for candidate in candidates {
                        let binding = match &candidate.binding {
                            Some(binding) => format!(
                                ", binding {} on {}",
                                binding.severity.as_str(),
                                binding
                                    .actions
                                    .iter()
                                    .map(|a| a.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            ),
                            None => String::new(),
                        };
                        lines.push(format!(
                            "  - {} (score {:.3}{binding})",
                            candidate.path, candidate.score
                        ));
                    }
                }
            }
        }
        for ambiguity in &self.ambiguities {
            lines.push(format!("ambiguity: {ambiguity}"));
        }
        if options.full {
            lines.push(format!("exit code: {}", self.exit_code().code()));
        }
        lines.join("\n")
    }

    fn render_json(&self) -> Value {
        let outcome = match &self.outcome {
            PlanOutcome::CorpusUnavailable { detail } => {
                serde_json::json!({"state": "corpus_unavailable", "detail": detail})
            }
            PlanOutcome::Ready {
                domains,
                candidates,
                qmd_unavailable,
            } => serde_json::json!({
                "state": "ready",
                "domains": domains.iter().map(|d| serde_json::json!({
                    "name": d.name, "owner": d.owner, "topics": d.topics, "summary": d.summary,
                })).collect::<Vec<_>>(),
                "candidates": candidates.iter().map(|c| serde_json::json!({
                    "path": c.path,
                    "score": c.score,
                    "binding": c.binding.as_ref().map(|b| serde_json::json!({
                        "severity": b.severity.as_str(),
                        "actions": b.actions.iter().map(|a| a.as_str()).collect::<Vec<_>>(),
                    })),
                })).collect::<Vec<_>>(),
                "qmd_unavailable": qmd_unavailable,
            }),
        };
        serde_json::json!({
            "gist": self.gist,
            "content_type": self.content_type,
            "domain": self.domain,
            "target_path": self.target_path,
            "ambiguities": self.ambiguities,
            "outcome": outcome,
            "exit_code": self.exit_code().code(),
        })
    }
}

impl Render for ApplyReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = vec!["kaibo contribute apply".to_string()];
        lines.push(format!("path: {}", self.path));
        lines.push(format!("branch: {}", self.branch));

        match &self.outcome {
            ApplyOutcome::Stopped(_) => lines.push("result: stopped".to_string()),
            ApplyOutcome::Completed {
                push_route,
                pr_url,
                ci,
            } => {
                lines.push(format!(
                    "push route: {}",
                    match push_route.kind {
                        PushRouteKind::Direct => "direct".to_string(),
                        PushRouteKind::Fork => format!(
                            "fork ({})",
                            push_route.owner.as_deref().unwrap_or("unknown")
                        ),
                    }
                ));
                lines.push(format!("pr: {pr_url}"));
                lines.push(format!(
                    "ci: {}",
                    match ci {
                        CiVerdict::Passed => "passed".to_string(),
                        CiVerdict::Failed { detail } => format!("failed ({detail})"),
                    }
                ));
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
            ApplyOutcome::Stopped(_) => serde_json::json!({"state": "stopped"}),
            ApplyOutcome::Completed {
                push_route,
                pr_url,
                ci,
            } => serde_json::json!({
                "state": "completed",
                "push_route": match push_route.kind {
                    PushRouteKind::Direct => "direct",
                    PushRouteKind::Fork => "fork",
                },
                "fork_owner": push_route.owner,
                "pr_url": pr_url,
                "ci": match ci {
                    CiVerdict::Passed => serde_json::json!({"passed": true}),
                    CiVerdict::Failed { detail } => serde_json::json!({"passed": false, "detail": detail}),
                },
            }),
        };
        serde_json::json!({
            "path": self.path,
            "branch": self.branch,
            "returned_to_main": self.returned_to_main,
            "outcome": outcome,
            "exit_code": self.exit_code().code(),
        })
    }
}

#[cfg(test)]
mod tests;
