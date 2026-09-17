#![forbid(unsafe_code)]

//! `kaibo`: a typed CLI interface to a git-backed markdown knowledge corpus,
//! for AI coding agents.
//!
//! This binary is deliberately thin: argument parsing, binding the global
//! flags to config, and mapping a `kaibo_core` error to a process exit code.
//! All logic lives in `kaibo_core`.
//!
//! `status` is the first verb; it only reads. Later verbs (`sync`, `query`,
//! `contribute`, `doctrine`, ...) land in later changes. None of them will
//! ever accept a flag that names a repo, a clone path, or an index: that is
//! what `Config` is for.

use std::process::ExitCode;

use clap::{Parser, Subcommand};
use kaibo_core::clock::SystemClock;
use kaibo_core::config::Config;
use kaibo_core::error::ExitCoded;
use kaibo_core::explain::Explainable;
use kaibo_core::output::{Render, RenderOptions};
use kaibo_core::process::RealCommandRunner;
use kaibo_core::status::StatusVerb;

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
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Report on the health of the local clone and qmd index. Read-only.
    Status,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    let config = match Config::resolve() {
        Ok(config) => config,
        Err(err) => return report_error(&err),
    };

    match &cli.command {
        Some(Commands::Status) => run_status(&config, &cli),
        None => report_no_command(cli.json),
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
