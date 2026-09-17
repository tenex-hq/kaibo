//! Process execution, injectable.
//!
//! [`CommandRunner`] is the trait every piece of logic that shells out to
//! `git` or `qmd` depends on, instead of calling `std::process::Command`
//! directly. [`RealCommandRunner`] is the only implementation used outside
//! tests, and the only place in the crate that spawns a real child process.
//! Tests inject `testing::FakeCommandRunner` instead, so no test in this
//! crate ever executes a real `git` or `qmd` - hermetic-in-CI is a hard rule
//! here, not a preference.

use std::process::Stdio;

use crate::explain::PlannedCommand;

/// A command's exit status, typed so a non-zero exit can never be mistaken
/// for success by a caller that only looked at captured stdout.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommandStatus {
    pub success: bool,
    pub code: Option<i32>,
}

/// Captured result of running a [`PlannedCommand`]: stdout, stderr and exit
/// status together, so reading stdout without checking `status` first is at
/// least visibly a choice, not the only option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CommandOutput {
    pub stdout: String,
    pub stderr: String,
    pub status: CommandStatus,
}

impl CommandOutput {
    pub fn success(&self) -> bool {
        self.status.success
    }
}

/// The process could not even be started (not on `PATH`, permission denied,
/// ...) - distinct from a command that ran and exited non-zero, which is a
/// successful [`CommandRunner::run`] call carrying a failing
/// [`CommandOutput`].
#[derive(Debug, thiserror::Error)]
#[error("failed to run `{program}`: {source}")]
pub struct SpawnError {
    pub program: String,
    #[source]
    pub source: std::io::Error,
}

pub trait CommandRunner {
    /// Run `command` to completion and capture its output. Implementations
    /// must never inherit stdin and must never prompt: everything this crate
    /// shells out for is a diagnostic, never something a real terminal would
    /// need to answer.
    fn run(&self, command: &PlannedCommand) -> Result<CommandOutput, SpawnError>;
}

/// The only [`CommandRunner`] used outside tests.
pub struct RealCommandRunner;

impl CommandRunner for RealCommandRunner {
    fn run(&self, command: &PlannedCommand) -> Result<CommandOutput, SpawnError> {
        let output = std::process::Command::new(&command.program)
            .args(&command.args)
            .stdin(Stdio::null())
            .output()
            .map_err(|source| SpawnError {
                program: command.program.clone(),
                source,
            })?;

        Ok(CommandOutput {
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
            status: CommandStatus {
                success: output.status.success(),
                code: output.status.code(),
            },
        })
    }
}

#[cfg(test)]
pub(crate) mod testing {
    use super::*;
    use std::cell::RefCell;

    /// Test-only [`CommandRunner`]: answers from a table of exact-match
    /// [`PlannedCommand`] -> result pairs scripted up front, and records
    /// every command it was actually asked to run, so a test can check both
    /// "what would have run" and "what the caller did with the result"
    /// without a real `git` or `qmd` anywhere near the test machine.
    #[derive(Default)]
    pub(crate) struct FakeCommandRunner {
        responses: RefCell<Vec<(PlannedCommand, Result<CommandOutput, ()>)>>,
        calls: RefCell<Vec<PlannedCommand>>,
    }

    impl FakeCommandRunner {
        pub(crate) fn new() -> Self {
            Self::default()
        }

        /// Script the next answer for a command matching `command` exactly.
        pub(crate) fn on(self, command: PlannedCommand, output: CommandOutput) -> Self {
            self.responses.borrow_mut().push((command, Ok(output)));
            self
        }

        /// Script `command` as unable to even start, e.g. "not on PATH".
        pub(crate) fn on_missing(self, command: PlannedCommand) -> Self {
            self.responses.borrow_mut().push((command, Err(())));
            self
        }

        /// Every command actually passed to `run`, in call order -
        /// `--explain` tests assert this stays empty.
        pub(crate) fn calls(&self) -> Vec<PlannedCommand> {
            self.calls.borrow().clone()
        }
    }

    impl CommandRunner for FakeCommandRunner {
        fn run(&self, command: &PlannedCommand) -> Result<CommandOutput, SpawnError> {
            self.calls.borrow_mut().push(command.clone());
            let mut responses = self.responses.borrow_mut();
            let position = responses
                .iter()
                .position(|(scripted, _)| scripted == command)
                .unwrap_or_else(|| panic!("no scripted response for `{command}`"));
            let (_, result) = responses.remove(position);
            result.map_err(|()| SpawnError {
                program: command.program.clone(),
                source: std::io::Error::from(std::io::ErrorKind::NotFound),
            })
        }
    }

    /// A successful [`CommandOutput`] with the given stdout.
    pub(crate) fn ok(stdout: impl Into<String>) -> CommandOutput {
        CommandOutput {
            stdout: stdout.into(),
            stderr: String::new(),
            status: CommandStatus {
                success: true,
                code: Some(0),
            },
        }
    }

    /// A failing (non-zero exit) [`CommandOutput`] with the given stderr.
    pub(crate) fn failed(stderr: impl Into<String>) -> CommandOutput {
        CommandOutput {
            stdout: String::new(),
            stderr: stderr.into(),
            status: CommandStatus {
                success: false,
                code: Some(1),
            },
        }
    }

    // These two tests exercise `FakeCommandRunner` itself, not any real
    // command - "some-tool" is a placeholder, deliberately not "qmd", so
    // this file stays out of the qmd-construction architecture test in
    // `crate::qmd`.
    #[test]
    fn fake_runner_answers_scripted_commands_and_records_calls() {
        let cmd = PlannedCommand::new("some-tool", ["--version"]);
        let runner = FakeCommandRunner::new().on(cmd.clone(), ok("some-tool 2.8.3\n"));

        let output = runner.run(&cmd).unwrap();
        assert_eq!(output.stdout, "some-tool 2.8.3\n");
        assert!(output.success());
        assert_eq!(runner.calls(), vec![cmd]);
    }

    #[test]
    fn fake_runner_reports_a_missing_program_as_a_spawn_error() {
        let cmd = PlannedCommand::new("some-tool", ["--version"]);
        let runner = FakeCommandRunner::new().on_missing(cmd.clone());

        let err = runner.run(&cmd).unwrap_err();
        assert_eq!(err.program, "some-tool");
    }
}
