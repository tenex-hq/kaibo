//! `--explain` plumbing.
//!
//! A verb that would shell out to `git`, `qmd`, or `gh` declares what it
//! would run by returning [`PlannedCommand`] values, so the binary can print
//! those instead of actually running them. No verb exists yet in this PR;
//! this module is the mechanism future verbs plug into.

/// One command a verb would execute, had `--explain` not been passed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlannedCommand {
    pub program: String,
    pub args: Vec<String>,
}

impl PlannedCommand {
    pub fn new(
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
