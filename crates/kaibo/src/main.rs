#![forbid(unsafe_code)]

//! `kaibo`: a typed CLI interface to a git-backed markdown knowledge corpus,
//! for AI coding agents.
//!
//! This binary is deliberately thin: argument parsing, binding the global
//! flags to config, and mapping a `kaibo_core` error to a process exit code.
//! All logic lives in `kaibo_core`.
//!
//! `status`, `sync`, `query`, `doctrine`, `domains`, `lint` and
//! `contribute` are the verbs so far; `status` only reads, `sync`
//! clones/pulls the corpus and refreshes its qmd index, `query` retrieves
//! ranked, cited evidence for a question, `doctrine` loads a named
//! domain's own MOC section plus its `current` reference pages in one call
//! (a load, not a question), `domains` lists the root MOC's domain
//! inventory as structured data, and `lint` runs a rule registry over the
//! corpus (or over given paths), gating on structural violations and only
//! annotating heuristic ones. `query` and `doctrine` both self-heal via
//! `sync` when the local corpus is missing, stale, or its qmd collection
//! is gone, and neither ever synthesises an answer - `query` holds no API
//! key and makes no network call of kaibo's own. `lint` never touches qmd
//! at all: it reads whatever is already on disk. `contribute plan` and
//! `contribute apply` are the write side: `plan` surfaces placement
//! candidates and never mutates anything, `apply` writes, lints, branches,
//! commits, pushes (directly or via a verified fork), opens a PR and
//! watches CI. None of these verbs will ever accept a flag that names a
//! repo, a clone path, or an index: that is what `Config` is for.

use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::{SystemTime, UNIX_EPOCH};

use clap::{Args, Parser, Subcommand};
use kaibo_core::clock::{Clock, SystemClock};
use kaibo_core::config::Config;
use kaibo_core::contribute::{ApplyInput, ContributeApplyVerb, ContributePlanVerb, Placement};
use kaibo_core::doctrine::DoctrineVerb;
use kaibo_core::domains::DomainsVerb;
use kaibo_core::error::ExitCoded;
use kaibo_core::event::{Caller, Event};
use kaibo_core::explain::Explainable;
use kaibo_core::install::{InstallMode, InstallVerb};
use kaibo_core::lint::LintVerb;
use kaibo_core::output::{Render, RenderOptions};
use kaibo_core::process::RealCommandRunner;
use kaibo_core::query::QueryVerb;
use kaibo_core::status::StatusVerb;
use kaibo_core::sync::SyncVerb;
use kaibo_core::trail::{self, TrailWrite};

/// A typed CLI interface to a git-backed markdown knowledge corpus.
#[derive(Parser, Debug)]
#[command(name = "kaibo", version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Emit machine-readable JSON instead of the default compact text.
    #[arg(long, global = true)]
    json: bool,

    /// Show wide output. Default schemas are narrow (3-4 fields per item).
    #[arg(long, global = true)]
    full: bool,

    /// Print the underlying git/qmd commands a verb would run, instead of running them.
    #[arg(long, global = true)]
    explain: bool,

    /// Skip this invocation's line in the paper trail at `~/.kaibo/trail.jsonl`.
    /// Equivalent to `no_log = true` in `~/.kaibo/config.toml`.
    #[arg(long, global = true)]
    no_log: bool,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Report on the health of the local clone and qmd index. Read-only.
    Status,
    /// Clone or pull the corpus, then refresh its qmd index.
    Sync(SyncCommandArgs),
    /// Retrieve ranked, cited evidence for a question. Never synthesises an
    /// answer - that's the calling model's job.
    Query(QueryCommandArgs),
    /// Load a domain's root-MOC section plus its `current` reference pages
    /// in one call. A load, not a question - see `kaibo domains` for the
    /// available domain names.
    Doctrine(DoctrineCommandArgs),
    /// List the root MOC's domain inventory as structured data: domain,
    /// owner, topics, summary.
    Domains,
    /// Run the rule registry over the corpus, or over the given paths.
    /// Structural violations exit non-zero; heuristic ones only annotate.
    Lint(LintCommandArgs),
    /// The write side: plan a placement, or apply an already-resolved one.
    #[command(subcommand)]
    Contribute(ContributeCommands),
    /// Place the skills this binary carries where Claude Code finds them,
    /// or remove them again. Idempotent either way.
    Install(InstallCommandArgs),
}

#[derive(Args, Debug)]
struct InstallCommandArgs {
    /// Remove the installed skills instead of placing them.
    #[arg(long)]
    uninstall: bool,
}

#[derive(Subcommand, Debug)]
enum ContributeCommands {
    /// Surface placement candidates for a piece of knowledge. Read-only:
    /// never writes, never prompts, never decides content type or domain.
    Plan(ContributePlanArgs),
    /// Write, lint-gate, branch, commit, push (direct or via a verified
    /// fork), open a PR against the configured repo, and watch CI.
    Apply(ContributeApplyArgs),
}

#[derive(Args, Debug)]
struct ContributePlanArgs {
    /// The gist of the knowledge to place.
    gist: String,

    /// Resolved content type, if already known. Classification is the
    /// calling agent's judgement call, never this verb's.
    #[arg(long)]
    r#type: Option<String>,

    /// Resolved domain folder, if already known.
    #[arg(long)]
    domain: Option<String>,
}

#[derive(Args, Debug)]
struct ContributeApplyArgs {
    /// Resolved content type (a single path segment, e.g. `how-to`).
    #[arg(long)]
    r#type: String,

    /// Resolved domain folder (a single path segment).
    #[arg(long)]
    domain: String,

    /// The page title.
    #[arg(long)]
    title: String,

    /// The page body (markdown, no frontmatter).
    #[arg(long)]
    body: String,

    /// Frontmatter tags, kebab-case. Repeatable.
    #[arg(long = "tag")]
    tags: Vec<String>,

    /// Append to this existing repo-relative page instead of creating a
    /// new one. The path is corpus-shaped input, like `lint`'s path
    /// argument - never a flag naming the repo, clone, index or
    /// collection.
    #[arg(long)]
    append: Option<String>,
}

#[derive(Args, Debug)]
struct SyncCommandArgs {
    /// No-op unless the corpus is currently stale, so a caller (e.g.
    /// `query`) can ask for a self-heal without paying for a full sync
    /// round trip when there is nothing to heal.
    #[arg(long)]
    if_stale: bool,
}

#[derive(Args, Debug)]
struct QueryCommandArgs {
    /// The question to search the knowledge corpus for.
    question: String,

    /// Include draft pages in the results, each labelled as a draft. Drafts
    /// are excluded by default.
    #[arg(long)]
    include_drafts: bool,
}

#[derive(Args, Debug)]
struct DoctrineCommandArgs {
    /// The domain folder to load doctrine for. See `kaibo domains` for the
    /// available names.
    domain: String,
}

#[derive(Args, Debug)]
struct LintCommandArgs {
    /// Files or folders under the corpus to lint. Defaults to the whole
    /// corpus when none are given.
    paths: Vec<String>,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let config = match Config::resolve() {
        Ok(config) => config,
        Err(err) => return report_error(&err),
    };

    match &cli.command {
        Some(Commands::Status) => run_status(&config, &cli),
        Some(Commands::Sync(args)) => run_sync(&config, &cli, args.if_stale),
        Some(Commands::Query(args)) => {
            run_query(&config, &cli, &args.question, args.include_drafts)
        }
        Some(Commands::Doctrine(args)) => run_doctrine(&config, &cli, &args.domain),
        Some(Commands::Domains) => run_domains(&config, &cli),
        Some(Commands::Lint(args)) => run_lint(&config, &cli, args.paths.clone()),
        Some(Commands::Contribute(ContributeCommands::Plan(args))) => {
            run_contribute_plan(&config, &cli, args)
        }
        Some(Commands::Contribute(ContributeCommands::Apply(args))) => {
            run_contribute_apply(&config, &cli, args)
        }
        Some(Commands::Install(args)) => run_install(&config, &cli, args.uninstall),
        None => report_no_command(cli.json),
    }
}

/// Facts the process can see about itself. ADR 0015: never a label someone
/// declared, because anything settable is settable by a test harness.
fn observed() -> Caller {
    Caller::observed(std::io::stdout().is_terminal())
}

/// When the invocation started, in Unix milliseconds, and how long the verb
/// took. A clock reporting a start before the epoch, or an end before its own
/// start, yields zero rather than suppressing the record: one nonsensical
/// duration is worth less than the rest of the event, not more.
fn timings(clock: &dyn Clock, started: SystemTime) -> (u128, u64) {
    let at = started
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_millis())
        .unwrap_or_default();
    let took = clock
        .now()
        .duration_since(started)
        .map(|elapsed| u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX))
        .unwrap_or_default();
    (at, took)
}

/// Append one event, unless this invocation or this installation turned the
/// trail off.
///
/// A failure goes to stderr and nowhere else. stdout carries the caller's
/// answer and the exit code carries the verb's result: neither belongs to the
/// log, and `trail::append` returns no `Result` precisely so that a failure
/// here cannot be propagated into either by accident.
fn record(config: &Config, cli: &Cli, event: &Event) {
    if cli.no_log {
        return;
    }
    if let Some(path) = config.trail_path()
        && let TrailWrite::Failed { detail } = trail::append(path, event)
    {
        eprintln!("warning: {detail}");
    }

    // The file is written either way, so a machine with no collector keeps a
    // greppable trail (ADR 0002). Export is a second sink, never a
    // replacement for the first.
    #[cfg(feature = "otlp")]
    if let Some(target) = config.otlp()
        && let TrailWrite::Failed { detail } = trail::otlp::export(&target, event)
    {
        eprintln!("warning: {detail}");
    }
}

fn run_status(config: &Config, cli: &Cli) -> ExitCode {
    let verb = StatusVerb::new(config);

    if cli.explain {
        for command in verb.explain() {
            println!("{command}");
        }
        return to_process_exit_code(kaibo_core::error::ExitCode::Success);
    }

    let runner = RealCommandRunner;
    let clock = SystemClock;
    let report = verb.gather(&runner, &clock, env!("CARGO_PKG_VERSION"));

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

fn run_sync(config: &Config, cli: &Cli, if_stale: bool) -> ExitCode {
    let verb = SyncVerb::new(config);

    if cli.explain {
        for command in verb.explain() {
            println!("{command}");
        }
        return to_process_exit_code(kaibo_core::error::ExitCode::Success);
    }

    let runner = RealCommandRunner;
    let clock = SystemClock;
    let report = verb.gather(&runner, &clock, if_stale);

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

fn run_query(config: &Config, cli: &Cli, question: &str, include_drafts: bool) -> ExitCode {
    let verb = match QueryVerb::new(config, question) {
        Ok(verb) => verb,
        Err(err) => return report_error(&err),
    };

    if cli.explain {
        for command in verb.explain() {
            println!("{command}");
        }
        return to_process_exit_code(kaibo_core::error::ExitCode::Success);
    }

    let runner = RealCommandRunner;
    let clock = SystemClock;
    let started = clock.now();
    let report = verb.gather(&runner, &clock, include_drafts);
    let (at, took) = timings(&clock, started);
    record(
        config,
        cli,
        &Event::from_query(&report, at, took, observed()),
    );

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

fn run_doctrine(config: &Config, cli: &Cli, domain: &str) -> ExitCode {
    let verb = DoctrineVerb::new(config, domain);

    if cli.explain {
        for command in verb.explain() {
            println!("{command}");
        }
        return to_process_exit_code(kaibo_core::error::ExitCode::Success);
    }

    let runner = RealCommandRunner;
    let clock = SystemClock;
    let started = clock.now();
    let report = verb.gather(&runner, &clock);
    let (at, took) = timings(&clock, started);
    record(
        config,
        cli,
        &Event::from_doctrine(&report, at, took, observed()),
    );

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

fn run_domains(config: &Config, cli: &Cli) -> ExitCode {
    let verb = DomainsVerb::new(config);

    if cli.explain {
        for command in verb.explain() {
            println!("{command}");
        }
        return to_process_exit_code(kaibo_core::error::ExitCode::Success);
    }

    let runner = RealCommandRunner;
    let clock = SystemClock;
    let report = verb.gather(&runner, &clock);

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

fn run_lint(config: &Config, cli: &Cli, paths: Vec<String>) -> ExitCode {
    let verb = LintVerb::new(config, paths);

    if cli.explain {
        for command in verb.explain() {
            println!("{command}");
        }
        return to_process_exit_code(kaibo_core::error::ExitCode::Success);
    }

    let report = verb.gather();

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

fn run_contribute_plan(config: &Config, cli: &Cli, args: &ContributePlanArgs) -> ExitCode {
    let verb = ContributePlanVerb::new(
        config,
        args.gist.clone(),
        args.r#type.clone(),
        args.domain.clone(),
    );

    if cli.explain {
        for command in verb.explain() {
            println!("{command}");
        }
        return to_process_exit_code(kaibo_core::error::ExitCode::Success);
    }

    let runner = RealCommandRunner;
    let report = verb.gather(&runner);

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

fn run_contribute_apply(config: &Config, cli: &Cli, args: &ContributeApplyArgs) -> ExitCode {
    let placement = match &args.append {
        Some(path) => Placement::Append { path: path.clone() },
        None => Placement::Create,
    };
    let input = ApplyInput {
        content_type: args.r#type.clone(),
        domain: args.domain.clone(),
        title: args.title.clone(),
        body: args.body.clone(),
        tags: args.tags.clone(),
        placement,
    };
    let verb = ContributeApplyVerb::new(config, input);

    if cli.explain {
        for command in verb.explain() {
            println!("{command}");
        }
        return to_process_exit_code(kaibo_core::error::ExitCode::Success);
    }

    let runner = RealCommandRunner;
    let clock = SystemClock;
    let report = verb.apply(&runner, &clock);

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

/// `install` shells out to nothing, so it has no `PlannedCommand` list to
/// print: `--explain` runs the same planning pass and reports the
/// filesystem changes it would have made, having made none of them.
fn run_install(config: &Config, cli: &Cli, uninstall: bool) -> ExitCode {
    let mode = if uninstall {
        InstallMode::Uninstall
    } else {
        InstallMode::Install
    };
    let verb = InstallVerb::new(config, env!("CARGO_PKG_VERSION"), mode);

    let report = if cli.explain {
        verb.plan()
    } else {
        verb.apply()
    };

    if cli.json {
        println!("{}", report.render_json());
    } else {
        println!("{}", report.render_text(&RenderOptions { full: cli.full }));
    }

    to_process_exit_code(report.exit_code())
}

fn report_error(err: &(impl std::fmt::Display + ExitCoded)) -> ExitCode {
    eprintln!("error: {err}");
    to_process_exit_code(err.exit_code())
}

fn report_no_command(json: bool) -> ExitCode {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({
                "error": "no command given",
                "detail": "run `kaibo status`, or see `kaibo --help` for available verbs",
                "hint": "kaibo --help",
            })
        );
    } else {
        eprintln!(
            "kaibo: no command given. Run `kaibo status`, or `kaibo --help` for available flags."
        );
    }
    to_process_exit_code(kaibo_core::error::ExitCode::Usage)
}

fn to_process_exit_code(code: kaibo_core::error::ExitCode) -> ExitCode {
    ExitCode::from(code.code())
}

#[cfg(test)]
mod cli_definition_tests {
    use super::Cli;
    use clap::CommandFactory;

    /// clap's own internal-consistency check: duplicate argument ids,
    /// colliding short flags, a `global` flag declared on a subcommand, and
    /// similar derive mistakes. Without this test clap surfaces them by
    /// panicking on the first real invocation, which means CI stays green and
    /// the user finds the bug. `debug_assert` is a no-op in release builds,
    /// so this only ever costs test time.
    #[test]
    fn cli_definition_is_internally_consistent() {
        Cli::command().debug_assert();
    }
}

#[cfg(test)]
mod architecture_tests {
    //! Guardrail: `query`, `doctrine`, `contribute`, `sync` and `status`
    //! must never accept a flag naming a repo, clone path, or index - only
    //! `kaibo install` may (see the module doc above). Nothing in the type
    //! system enforces this - clap has no notion of kaibo's design rules -
    //! so this is a source-scanning test, the practical mechanism for a
    //! rule about *which verb* is allowed to declare *which argument*.
    //!
    //! Detection is a plain-text heuristic, not a real Rust parser: a field
    //! declaration is a trimmed line starting with an (optional `pub`)
    //! forbidden name followed by `:`; a `long` override is a substring
    //! match on `long = "<name>"`. Both are attributed to the nearest
    //! enclosing `struct`/`enum` or braced enum-variant name found by
    //! scanning upward from the match, and exempted only if that name
    //! contains "install" (case-insensitive). This is good enough for this
    //! crate's flat, unnested verb definitions; it would need to get
    //! smarter if a verb's arguments ever nest inside a field struct that
    //! itself nests inside another.

    use std::fs;
    use std::path::{Path, PathBuf};

    const FORBIDDEN_FIELDS: [&str; 5] = ["repo", "clone", "index", "collection", "api_url"];
    const FORBIDDEN_FLAG_BASES: [&str; 5] = ["repo", "clone", "index", "collection", "api-url"];

    fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
        for entry in fs::read_dir(dir).expect("read src dir") {
            let path = entry.expect("dir entry").path();
            if path.is_dir() {
                rust_files(&path, out);
            } else if path.extension().and_then(|ext| ext.to_str()) == Some("rs") {
                out.push(path);
            }
        }
    }

    /// The nearest enclosing `struct Name`, `enum Name`, or braced
    /// enum-variant `Name {` at or above `line_idx`, searched backward - a
    /// plain-text stand-in for "which type owns this line".
    fn enclosing_definition(lines: &[&str], line_idx: usize) -> Option<String> {
        for line in lines[..=line_idx].iter().rev() {
            let trimmed = line.trim();
            for prefix in ["pub struct ", "struct ", "pub enum ", "enum "] {
                if let Some(rest) = trimmed.strip_prefix(prefix) {
                    return Some(rest.to_string());
                }
            }
            if let Some(head) = trimmed.strip_suffix('{') {
                let head = head.trim();
                if !head.is_empty() && head.chars().all(|c| c.is_alphanumeric() || c == '_') {
                    return Some(head.to_string());
                }
            }
        }
        None
    }

    /// Whether `line` declares a field literally named `name` (allowing a
    /// leading `pub` / `pub(crate)`), as opposed to merely containing `name`
    /// as a substring of a longer identifier (e.g. `repository`).
    fn is_field_declaration(line: &str, name: &str) -> bool {
        let line = line.trim();
        let line = line.strip_prefix("pub(crate)").unwrap_or(line).trim_start();
        let line = line.strip_prefix("pub").unwrap_or(line).trim_start();
        line.strip_prefix(name)
            .map(|rest| rest.trim_start().starts_with(':'))
            .unwrap_or(false)
    }

    /// Guardrail: no clap-derived verb struct in `crates/kaibo/` declares a
    /// field named `repo`, `clone`, `index`, `collection`, or `api_url`, and
    /// none of `--repo`, `--clone`, `--index`, `--collection`, `--api-url`
    /// appears as a clap `long` override - outside an `install` verb, the
    /// one place allowed to name a repo, clone path, or index.
    #[test]
    fn no_verb_accepts_a_target_bearing_argument() {
        let src_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = Vec::new();
        rust_files(&src_dir, &mut files);

        let mut violations = Vec::new();

        for path in files {
            let contents = fs::read_to_string(&path).expect("read source file");
            let lines: Vec<&str> = contents.lines().collect();
            let file_name = path
                .strip_prefix(&src_dir)
                .unwrap_or(&path)
                .display()
                .to_string();

            for (idx, raw_line) in lines.iter().enumerate() {
                let owner = enclosing_definition(&lines, idx).unwrap_or_default();
                if owner.to_lowercase().contains("install") {
                    continue;
                }

                for field in FORBIDDEN_FIELDS {
                    if is_field_declaration(raw_line, field) {
                        violations.push(format!(
                            "{file_name}:{}: field `{field}` on `{owner}`",
                            idx + 1
                        ));
                    }
                }

                for base in FORBIDDEN_FLAG_BASES {
                    let bare = format!("long = \"{base}\"");
                    let dashed = format!("long = \"--{base}\"");
                    if raw_line.contains(&bare) || raw_line.contains(&dashed) {
                        violations.push(format!(
                            "{file_name}:{}: flag `--{base}` on `{owner}`",
                            idx + 1
                        ));
                    }
                }
            }
        }

        assert!(
            violations.is_empty(),
            "verb declared a target-bearing argument outside `install`: {violations:#?}"
        );
    }
}
