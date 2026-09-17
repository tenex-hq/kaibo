//! `--explain` plumbing.
//!
//! A verb that would shell out to `git` or `qmd` declares what it would run
//! by returning [`PlannedCommand`] values, so the binary can print those
//! instead of actually running them.

/// One command a verb would execute, had `--explain` not been passed.
///
/// Code outside this crate cannot construct one directly: the fields and
/// [`PlannedCommand::new`] are crate-private, so a second face linking
/// `kaibo-core` (an MCP server, a hosted API handler) can only obtain a
/// `PlannedCommand` through the command builders this crate exposes, e.g.
/// [`crate::qmd::QmdCommand`] - which is what guarantees every qmd command
/// it hands out carries `--index`.
///
/// ```compile_fail
/// // Outside `kaibo_core`, `PlannedCommand::new` is not visible - the only
/// // way to obtain one is through this crate's own command builders.
/// let index_free_qmd_update = "qmd";
/// let _ = kaibo_core::explain::PlannedCommand::new(index_free_qmd_update, ["update"]);
/// ```
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCommand {
    pub(crate) program: String,
    pub(crate) args: Vec<String>,
}

impl PlannedCommand {
    pub(crate) fn new(
        program: impl Into<String>,
        args: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            program: program.into(),
            args: args.into_iter().map(Into::into).collect(),
        }
    }
}

impl std::fmt::Display for PlannedCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.program)?;
        for arg in &self.args {
            write!(f, " {arg}")?;
        }
        Ok(())
    }
}

/// A verb that can explain what it would shell out to, instead of doing it.
pub trait Explainable {
    fn explain(&self) -> Vec<PlannedCommand>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn displays_as_a_shell_command() {
        let cmd = PlannedCommand::new("git", ["clone", "org/repo"]);
        assert_eq!(cmd.to_string(), "git clone org/repo");
    }
}
