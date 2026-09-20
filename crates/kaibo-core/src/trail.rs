//! The paper trail's sink: one JSON line appended per invocation.
//!
//! The trail is a bystander to the verb that produced it. Its failure means
//! one missing line in a file nobody reads synchronously, and the answer the
//! caller asked for is unaffected - so a failure here must never reach the
//! exit code, which is why [`append`] returns [`TrailWrite`] and not a
//! `Result`. ADR 0016 fixes the record shape; this module only places it.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::Path;

use crate::event::Event;

/// What became of one append.
///
/// Deliberately not a `Result`: `?` must not be able to lift a trail failure
/// into a verb's error path. A caller reading only the exit code has to be
/// able to tell a knowledge gap from a failure, and an unwritable log file is
/// neither.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TrailWrite {
    Written,
    /// Already phrased for a human: the path that could not be written and
    /// the underlying reason.
    Failed {
        detail: String,
    },
}

/// Append one event as a single line.
///
/// Concurrent kaibo processes share the file by opening it `O_APPEND` and
/// writing one line each. That is not a guarantee of non-interleaving - a
/// short write inside `write_all` can still split a long line - but it needs
/// no lock file, and a torn line costs one record rather than the file.
pub fn append(path: &Path, event: &Event) -> TrailWrite {
    if let Some(parent) = path.parent()
        && let Err(source) = std::fs::create_dir_all(parent)
    {
        return failed(path, "create", &source);
    }

    let mut file = match OpenOptions::new().create(true).append(true).open(path) {
        Ok(file) => file,
        Err(source) => return failed(path, "open", &source),
    };

    // One `write_all` of line-plus-newline, not two writes: a second call
    // could land after another process's line and leave a stray newline in
    // the middle of the file.
    let mut line = event.to_jsonl();
    line.push('\n');
    match file.write_all(line.as_bytes()) {
        Ok(()) => TrailWrite::Written,
        Err(source) => failed(path, "append to", &source),
    }
}

fn failed(path: &Path, verb: &str, source: &std::io::Error) -> TrailWrite {
    TrailWrite::Failed {
        detail: format!(
            "could not {verb} the paper trail at {}: {source}; \
             set `no_log = true` in ~/.kaibo/config.toml to stop trying",
            path.display()
        ),
    }
}

#[cfg(test)]
mod tests;
