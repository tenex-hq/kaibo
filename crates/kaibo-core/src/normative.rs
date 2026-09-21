//! The normative layer: the optional schema that makes a reference page
//! **binding**, so a caller can be told what it owes before it acts.
//!
//! Normativity is an axis orthogonal to Diataxis purpose, not a new content
//! type. A standard stays `type: reference` and gains four keys: `binding`,
//! `severity`, `applies_to`, and a `kaibo-checks` block in the body. Every
//! existing page is non-binding by omission, so nothing in a corpus needs
//! migrating for this module to exist.
//!
//! **This module parses and validates; it never runs a check.** The checks
//! it returns are data - a compiled contract's payload - and the code that
//! evaluates them against an artifact lives client-side, outside this
//! crate. That keeps the one hostile input here (a `pattern` authored by
//! anyone who can merge a knowledge PR) confined to a string this module
//! compiles once, to prove it compiles, and then discards. Nothing parsed
//! out of a page reaches a command, a path, or a flag.
//!
//! **Decidable only, on evidence.** The epic this implements also specified
//! a `judgment` check species: a rubric item answered by a model rather
//! than evaluated. `evals/h2` measured that species against the same
//! standards written as prose, on two model tiers, and it bought no recall
//! at 2.6x to 3.3x the cost. It is therefore not in the vocabulary, and a
//! page that writes one is told so rather than having it silently dropped.
//! See [ADR 0017](../../../docs/adr/0017-the-conformance-schema-is-decidable-only.md).
//!
//! **Normativity is all or nothing.** A page carrying any one normative key
//! must carry the whole set. Half a standard - a `severity` nobody binds, a
//! `checks` block on a page that is not binding - is the silent
//! half-load this schema exists to make impossible, and every shape of it
//! is a loud error here.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

use crate::error::{ExitCode, ExitCoded};
use crate::frontmatter::Frontmatter;
use crate::trust;

/// The fence info-string token marking the one block in a page's body that
/// carries checks. The canonical spelling is ```` ```json kaibo-checks ````
/// so GitHub still highlights it as JSON; only this token is load-bearing.
const CHECKS_MARKER: &str = "kaibo-checks";

/// How much a standard binds. Two values, deliberately: severity is the
/// budget knob a contract filters on (`must` always, `should` on request),
/// not a scale to argue about.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Must,
    Should,
}

impl Severity {
    /// The whole vocabulary, in the order an error message lists it.
    pub const ALL: [Severity; 2] = [Severity::Must, Severity::Should];

    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Must => "must",
            Severity::Should => "should",
        }
    }

    /// Public because `contribute` accepts a severity on the command line
    /// and must reject an unknown one with the same vocabulary the schema
    /// uses, rather than writing a page the lint rule then refuses.
    pub fn parse(value: &str) -> Option<Severity> {
        Severity::ALL.into_iter().find(|s| s.as_str() == value)
    }
}

/// The action a caller is about to take, which is what a contract is
/// addressed by.
///
/// A closed vocabulary, extended deliberately rather than per standard: an
/// open one is a filename glob by another name, and a glob was rejected
/// because it fires a commit-message rule on every README and cannot say
/// "a compose file *in a workload repo*".
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum ActionKind {
    FileEdit,
    CommitMessage,
    ShellCommand,
    Chat,
    Deploy,
    Adr,
}

impl ActionKind {
    pub const ALL: [ActionKind; 6] = [
        ActionKind::FileEdit,
        ActionKind::CommitMessage,
        ActionKind::ShellCommand,
        ActionKind::Chat,
        ActionKind::Deploy,
        ActionKind::Adr,
    ];

    pub fn as_str(self) -> &'static str {
        match self {
            ActionKind::FileEdit => "file-edit",
            ActionKind::CommitMessage => "commit-message",
            ActionKind::ShellCommand => "shell-command",
            ActionKind::Chat => "chat",
            ActionKind::Deploy => "deploy",
            ActionKind::Adr => "adr",
        }
    }

    pub fn parse(value: &str) -> Option<ActionKind> {
        ActionKind::ALL.into_iter().find(|a| a.as_str() == value)
    }

    pub fn vocabulary() -> String {
        ActionKind::ALL
            .iter()
            .map(|a| a.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

/// What a standard binds: at least one action kind, plus free tags that
/// narrow it. Actions address, tags filter - which is why a standard with
/// tags and no action is rejected rather than defaulted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AppliesTo {
    pub actions: Vec<ActionKind>,
    pub tags: Vec<String>,
}

/// One decidable check: an id a verdict is keyed on, and the one operation
/// that produces it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Check {
    pub id: String,
    #[serde(flatten)]
    pub kind: CheckKind,
}

/// The four decidable operations. Each produces **facts** - a match, its
/// offset, its text - with no inference anywhere, which is why a client can
/// run them with no model at all.
///
/// `pattern`, `if_present` and `require` are regular expressions in the
/// `regex` crate's syntax, already proven to compile by [`parse`]. They are
/// carried as source strings rather than compiled values so a `Check` stays
/// plain, comparable, serializable data: this crate has no use for a
/// compiled regex it never runs, and the client that does run one builds it
/// from the contract it received, not from a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CheckKind {
    /// The artifact's text must not match.
    ForbidRegex { pattern: String },
    /// The artifact's text must match at least once.
    RequireRegex { pattern: String },
    /// Conditional: where `if_present` matches, `require` must match too.
    RequireIfPresent { if_present: String, require: String },
    /// The artifact's **path** must not match.
    ForbidPath { pattern: String },
}

/// A page's complete normative layer. Constructed only by [`parse`], and
/// only from a page that carried every part of it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Standard {
    pub severity: Severity,
    pub applies_to: AppliesTo,
    /// Possibly empty: a standard whose rule no regex can express is still
    /// binding, and still belongs in a contract as prose plus a severity.
    pub checks: Vec<Check>,
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum NormativeError {
    #[error(
        "`binding` is {0:?}, but it is a boolean: write `binding: true` to make this page a \
         standard, or remove the key"
    )]
    BindingNotABoolean(String),

    #[error(
        "this page carries {key} but is not `binding: true`; add `binding: true` to make it a \
         standard, or remove {key}"
    )]
    NormativeKeyWithoutBinding { key: &'static str },

    #[error("a `binding: true` page needs `{key}`; add it, or remove `binding: true`")]
    MissingNormativeKey { key: &'static str },

    #[error("`severity` is {0:?}; write one of: must, should")]
    UnknownSeverity(String),

    #[error(
        "`applies_to` is malformed: {detail}; it is a mapping with `actions` and optional `tags`"
    )]
    MalformedAppliesTo { detail: String },

    #[error(
        "`applies_to.actions` is empty; a standard no action names can never reach a caller, so \
         name at least one of: file-edit, commit-message, shell-command, chat, deploy, adr"
    )]
    NoActions,

    #[error("`applies_to.actions` names {0:?}, which is not an action kind; use one of: {vocabulary}", vocabulary = ActionKind::vocabulary())]
    UnknownAction(String),

    #[error(
        "the `{CHECKS_MARKER}` block is not a list of checks: {detail}; it is a JSON array, and \
         `template/CONVENTIONS.md` has the shape of each kind"
    )]
    MalformedChecks { detail: String },

    #[error("the `{CHECKS_MARKER}` block is never closed; add a closing ``` fence")]
    UnterminatedChecksBlock,

    #[error(
        "this page has more than one `{CHECKS_MARKER}` block; merge them into one, so which \
         block a check came from is never a question"
    )]
    MultipleChecksBlocks,

    #[error(
        "check {id:?} has the pattern {pattern:?}, which is not a valid regular expression: \
         {detail}"
    )]
    InvalidPattern {
        id: String,
        pattern: String,
        detail: String,
    },

    #[error("two checks share the id {0:?}; a verdict is keyed on it, so give each one its own")]
    DuplicateCheckId(String),

    #[error("a check has an empty `id`; a verdict is keyed on it, so give every check one")]
    EmptyCheckId,
}

impl ExitCoded for NormativeError {
    fn exit_code(&self) -> ExitCode {
        // Malformed corpus content is bad input, the same mapping
        // `FrontmatterError` already uses.
        ExitCode::Usage
    }
}

/// Parse a page's normative layer, or `Ok(None)` when it declares none.
///
/// `frontmatter` and `body` are corpus content and are treated as such:
/// every value that reaches an error message is stripped of control
/// characters first, and nothing parsed here is ever used to build a path
/// or a command.
pub fn parse(frontmatter: &Frontmatter, body: &str) -> Result<Option<Standard>, NormativeError> {
    let binding = match frontmatter.extra.get("binding") {
        None => None,
        Some(value) => match value.as_bool() {
            Some(b) => Some(b),
            None => return Err(NormativeError::BindingNotABoolean(render(value))),
        },
    };

    let checks_block = find_checks_block(body)?;

    if binding != Some(true) {
        // All or nothing: the other keys mean nothing without the one that
        // binds, so a page carrying them says so out loud instead of
        // loading as an ordinary page that happens to have extra keys.
        for (key, label) in [("severity", "`severity`"), ("applies_to", "`applies_to`")] {
            if frontmatter.extra.contains_key(key) {
                return Err(NormativeError::NormativeKeyWithoutBinding { key: label });
            }
        }
        if checks_block.is_some() {
            return Err(NormativeError::NormativeKeyWithoutBinding {
                key: "a `kaibo-checks` block",
            });
        }
        return Ok(None);
    }

    let severity = parse_severity(frontmatter)?;
    let applies_to = parse_applies_to(frontmatter)?;
    let checks = match checks_block {
        None => Vec::new(),
        Some(json) => parse_checks(&json)?,
    };

    Ok(Some(Standard {
        severity,
        applies_to,
        checks,
    }))
}

fn parse_severity(frontmatter: &Frontmatter) -> Result<Severity, NormativeError> {
    let value = frontmatter
        .extra
        .get("severity")
        .ok_or(NormativeError::MissingNormativeKey { key: "severity" })?;

    value
        .as_str()
        .and_then(Severity::parse)
        .ok_or_else(|| NormativeError::UnknownSeverity(render(value)))
}

/// `applies_to` as it is written, before the action strings are checked
/// against the closed vocabulary. Unknown keys are refused rather than
/// ignored: a typo'd `action:` that silently applied to nothing would be a
/// standard that never fires and never says why.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawAppliesTo {
    #[serde(default)]
    actions: Vec<String>,
    #[serde(default)]
    tags: Vec<String>,
}

fn parse_applies_to(frontmatter: &Frontmatter) -> Result<AppliesTo, NormativeError> {
    let value = frontmatter
        .extra
        .get("applies_to")
        .ok_or(NormativeError::MissingNormativeKey { key: "applies_to" })?;

    let raw: RawAppliesTo = serde_yaml_ng::from_value(value.clone()).map_err(|err| {
        NormativeError::MalformedAppliesTo {
            detail: clean(&err),
        }
    })?;

    if raw.actions.is_empty() {
        return Err(NormativeError::NoActions);
    }

    let mut actions = Vec::with_capacity(raw.actions.len());
    for action in &raw.actions {
        let kind = ActionKind::parse(action)
            .ok_or_else(|| NormativeError::UnknownAction(trust::strip_control_chars(action)))?;
        actions.push(kind);
    }

    Ok(AppliesTo {
        actions,
        tags: raw
            .tags
            .iter()
            .map(|tag| trust::strip_control_chars(tag))
            .collect(),
    })
}

/// A check as written, with the kind tag flattened alongside its id.
#[derive(Deserialize)]
struct RawCheck {
    id: String,
    #[serde(flatten)]
    kind: CheckKindRaw,
}

#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CheckKindRaw {
    ForbidRegex { pattern: String },
    RequireRegex { pattern: String },
    RequireIfPresent { if_present: String, require: String },
    ForbidPath { pattern: String },
}

fn parse_checks(json: &str) -> Result<Vec<Check>, NormativeError> {
    let raw: Vec<RawCheck> =
        serde_json::from_str(json).map_err(|err| NormativeError::MalformedChecks {
            detail: clean(&err),
        })?;

    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut checks = Vec::with_capacity(raw.len());

    for check in raw {
        let id = trust::strip_control_chars(&check.id);
        if id.is_empty() {
            return Err(NormativeError::EmptyCheckId);
        }
        if !seen.insert(id.clone()) {
            return Err(NormativeError::DuplicateCheckId(id));
        }

        let kind = match check.kind {
            CheckKindRaw::ForbidRegex { pattern } => CheckKind::ForbidRegex {
                pattern: validated_pattern(&id, pattern)?,
            },
            CheckKindRaw::RequireRegex { pattern } => CheckKind::RequireRegex {
                pattern: validated_pattern(&id, pattern)?,
            },
            CheckKindRaw::RequireIfPresent {
                if_present,
                require,
            } => CheckKind::RequireIfPresent {
                if_present: validated_pattern(&id, if_present)?,
                require: validated_pattern(&id, require)?,
            },
            CheckKindRaw::ForbidPath { pattern } => CheckKind::ForbidPath {
                pattern: validated_pattern(&id, pattern)?,
            },
        };

        checks.push(Check { id, kind });
    }

    Ok(checks)
}

/// Compile `pattern` to prove it compiles, then keep the source.
///
/// The compiled value is deliberately discarded: this crate never runs a
/// check, and a pattern that only fails at judgment time fails on the
/// machine of whoever took the action, far from whoever wrote the standard.
/// Compiling here moves that failure into the corpus's own CI. The `regex`
/// crate's guarantees are the second half of why a corpus-authored pattern
/// is safe to compile at all: no backtracking, so no catastrophic input.
fn validated_pattern(id: &str, pattern: String) -> Result<String, NormativeError> {
    regex::Regex::new(&pattern).map_err(|err| NormativeError::InvalidPattern {
        id: id.to_string(),
        pattern: trust::strip_control_chars(&pattern),
        detail: clean(&err),
    })?;
    Ok(pattern)
}

/// The body's one `kaibo-checks` block, or `None`.
///
/// Fence-aware on purpose: a `kaibo-checks` fence nested inside a wider
/// fence is an **example**, not a block. A page documenting the schema is
/// the obvious case, and a scanner that merely grepped for the marker would
/// turn every such page into a broken standard.
fn find_checks_block(body: &str) -> Result<Option<String>, NormativeError> {
    let mut open: Option<(char, usize, bool)> = None;
    let mut collected: Vec<String> = Vec::new();
    let mut found: Option<String> = None;

    for line in body.lines() {
        let trimmed = line.trim_start();
        match (fence(trimmed), open) {
            (Some((ch, len, info)), None) => {
                open = Some((ch, len, info.split_whitespace().any(|t| t == CHECKS_MARKER)));
            }
            (Some((ch, len, info)), Some((open_ch, open_len, is_checks)))
                if ch == open_ch && len >= open_len && info.trim().is_empty() =>
            {
                if is_checks {
                    if found.is_some() {
                        return Err(NormativeError::MultipleChecksBlocks);
                    }
                    found = Some(collected.join("\n"));
                }
                collected.clear();
                open = None;
            }
            (_, Some((_, _, true))) => collected.push(line.to_string()),
            _ => {}
        }
    }

    if let Some((_, _, true)) = open {
        return Err(NormativeError::UnterminatedChecksBlock);
    }

    Ok(found)
}

/// `(fence char, run length, info string)` for a line that opens or closes
/// a fenced block, per CommonMark: three or more backticks or tildes.
fn fence(line: &str) -> Option<(char, usize, &str)> {
    let ch = line.chars().next().filter(|c| *c == '`' || *c == '~')?;
    let len = line.chars().take_while(|c| *c == ch).count();
    (len >= 3).then(|| (ch, len, &line[len..]))
}

/// A YAML value as it would be written, for an error message: scalars
/// verbatim, anything else as the one-line YAML it was.
fn render(value: &serde_yaml_ng::Value) -> String {
    let rendered = match value.as_str() {
        Some(s) => s.to_string(),
        None => serde_yaml_ng::to_string(value)
            .unwrap_or_default()
            .trim()
            .to_string(),
    };
    trust::strip_control_chars(&rendered)
}

/// A parser's own message, flattened to one line and stripped, before it is
/// quoted into an error of ours. The message is about corpus content and
/// can quote it back.
fn clean(err: &impl std::fmt::Display) -> String {
    trust::strip_control_chars(&err.to_string().replace('\n', " "))
}

#[cfg(test)]
mod tests;
