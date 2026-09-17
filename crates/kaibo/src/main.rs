#![forbid(unsafe_code)]

//! `kaibo`: a typed CLI interface to a git-backed markdown knowledge corpus,
//! for AI coding agents.
//!
//! This binary is deliberately thin: argument parsing, binding the global
//! flags to config, and mapping a `kaibo_core` error to a process exit code.
//! All logic lives in `kaibo_core`.
//!
//! This build ships only the shared foundation from a larger design: config
//! resolution, the frontmatter model, and the output/error contracts. No
//! verbs (`query`, `contribute`, `sync`, `doctrine`, `status`, ...) exist
//! yet - they land in a later change once this foundation has been
//! reviewed on its own. Notably, none of them will ever accept a flag that
//! names a repo, a clone path, or an index: that is what `Config` is for.

use std::process::ExitCode;

use clap::Parser;
use kaibo_core::config::Config;
use kaibo_core::error::ExitCoded;

/// A typed CLI interface to a git-backed markdown knowledge corpus.
#[derive(Parser, Debug)]
#[command(name = "kaibo", version, about, long_about = None)]
struct Cli {
    /// Emit machine-readable JSON instead of the default compact text.
    #[arg(long, global = true)]
    json: bool,

    /// Show wide output. Default schemas are narrow (3-4 fields per item).
    #[arg(long, global = true)]
    full: bool,

    /// Print the underlying git/qmd/gh commands a verb would run, instead of running them.
    #[arg(long, global = true)]
    explain: bool,
}

fn main() -> ExitCode {
    let cli = Cli::parse();

    // No verb exists yet to consume these; referencing them here keeps that
    // an intentional, documented no-op instead of a silent dead-code lint
    // suppression. The first verb removes this line entirely.
    let _ = (cli.full, cli.explain);

    let config = match Config::resolve() {
        Ok(config) => config,
        Err(err) => return report_error(&err),
    };

    // Foundation-only build: there is no verb to run yet, so there is
    // nothing `config` can be used for. Say so plainly (respecting --json)
    // rather than pretending to have succeeded at something.
    let _ = &config;
    report_no_command(cli.json)
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
                "detail": "verbs are not implemented yet",
                "hint": "kaibo --help",
            })
        );
    } else {
        eprintln!(
            "kaibo: no command given (verbs are not implemented yet). Run `kaibo --help` for available flags."
        );
    }
    to_process_exit_code(kaibo_core::error::ExitCode::Usage)
}

fn to_process_exit_code(code: kaibo_core::error::ExitCode) -> ExitCode {
    ExitCode::from(code.code())
}
