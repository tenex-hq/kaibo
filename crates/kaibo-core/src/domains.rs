//! `kaibo domains`: the root MOC's domain inventory, as structured data.
//!
//! No arguments, no question - `kaibo query`'s gap report and `kaibo
//! doctrine`'s unknown-domain gap both point a caller here to see what
//! actually exists. Each entry is exactly the shape [`crate::moc`] parses
//! out of one `## ` section of the root MOC: `name` (what `kaibo doctrine
//! <domain>` expects), `owner`, `topics`, `summary`.
//!
//! Shares its self-heal trigger and its "the MOC being unreadable is a
//! broken corpus, not a gap" distinction with `kaibo doctrine` - see that
//! module's doc comment for the reasoning, which applies unchanged here. A
//! MOC that parses but has no `## ` domain headings at all is its own gap
//! (nothing to discover), reported with the same
//! [`crate::error::ExitCode::NoHits`] signal.

use serde_json::Value;

use crate::clock::Clock;
use crate::config::Config;
use crate::error::ExitCode;
use crate::explain::{Explainable, PlannedCommand};
use crate::moc::{self, DomainSection};
use crate::output::{Render, RenderOptions};
use crate::process::CommandRunner;
use crate::self_heal;
use crate::sync::{self, SyncVerb};

/// One thing worth telling the user about, with the exact next command
/// where a fix exists. Same shape as `doctrine::Finding` / `sync::Finding`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub message: String,
    pub fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum DomainsOutcome {
    /// Self-heal was needed and the sync it ran failed.
    SelfHealFailed {
        exit_code: ExitCode,
        findings: Vec<Finding>,
    },
    /// The root MOC itself could not be read (after self-heal, if one
    /// ran) - a broken corpus, not a gap.
    MocUnavailable { detail: String },
    /// The MOC was read cleanly but contained no `## ` domain headings at
    /// all. The gap signal.
    NoDomains,
    /// At least one domain, in the order the MOC lists them.
    Listed { domains: Vec<DomainSection> },
}

#[derive(Debug, Clone, PartialEq)]
pub struct DomainsReport {
    /// `Some` iff self-heal ran.
    pub self_heal: Option<sync::SyncOutcome>,
    pub outcome: DomainsOutcome,
}

impl DomainsReport {
    /// `Usage`/`Stale` when self-heal was needed and sync itself failed;
    /// `Stale` when the root MOC could not be read; `NoHits` when it was
    /// read but named no domains; `Success` otherwise.
    pub fn exit_code(&self) -> ExitCode {
        match &self.outcome {
            DomainsOutcome::SelfHealFailed { exit_code, .. } => *exit_code,
            DomainsOutcome::MocUnavailable { .. } => ExitCode::Stale,
            DomainsOutcome::NoDomains => ExitCode::NoHits,
            DomainsOutcome::Listed { .. } => ExitCode::Success,
        }
    }

    /// Findings, each carrying the exact next command where one exists.
    pub fn findings(&self) -> Vec<Finding> {
        match &self.outcome {
            DomainsOutcome::SelfHealFailed { findings, .. } => findings.clone(),
            DomainsOutcome::MocUnavailable { detail } => vec![Finding {
                message: format!("root MOC could not be read: {detail}"),
                fix: Some("re-run `kaibo sync`, then try `kaibo domains` again".to_string()),
            }],
            DomainsOutcome::NoDomains | DomainsOutcome::Listed { .. } => Vec::new(),
        }
    }
}

/// The `kaibo domains` verb, bound to a resolved `Config`. Takes no
/// arguments: there is nothing to name, only the corpus's own inventory to
/// report.
pub struct DomainsVerb<'a> {
    config: &'a Config,
}

impl<'a> DomainsVerb<'a> {
    pub fn new(config: &'a Config) -> Self {
        Self { config }
    }

    /// Run domains for real: self-heal if needed, then read the inventory.
    pub fn gather(&self, runner: &dyn CommandRunner, clock: &dyn Clock) -> DomainsReport {
        gather(self.config, runner, clock)
    }
}

impl Explainable for DomainsVerb<'_> {
    fn explain(&self) -> Vec<PlannedCommand> {
        // `domains` never calls `qmd query` itself; the only commands it
        // could ever run are the self-heal pipeline.
        SyncVerb::new(self.config).explain()
    }
}

fn gather(config: &Config, runner: &dyn CommandRunner, clock: &dyn Clock) -> DomainsReport {
    let self_heal_outcome = if self_heal::needs_self_heal(config, runner, clock) {
        let sync_report = SyncVerb::new(config).gather(runner, clock, false);
        let sync_exit = sync_report.exit_code();

        if matches!(sync_exit, ExitCode::Usage | ExitCode::Stale) {
            let findings = sync_report
                .findings()
                .into_iter()
                .map(|f| Finding {
                    message: f.message,
                    fix: f.fix,
                })
                .collect();
            return DomainsReport {
                self_heal: Some(sync_report.outcome),
                outcome: DomainsOutcome::SelfHealFailed {
                    exit_code: sync_exit,
                    findings,
                },
            };
        }

        Some(sync_report.outcome)
    } else {
        None
    };

    let domains = match moc::read_domain_sections(config.clone_path()) {
        Ok(domains) => domains,
        Err(moc::MocUnreadable { detail }) => {
            return DomainsReport {
                self_heal: self_heal_outcome,
                outcome: DomainsOutcome::MocUnavailable { detail },
            };
        }
    };

    let outcome = if domains.is_empty() {
        DomainsOutcome::NoDomains
    } else {
        DomainsOutcome::Listed { domains }
    };

    DomainsReport {
        self_heal: self_heal_outcome,
        outcome,
    }
}

fn self_heal_summary(outcome: &sync::SyncOutcome) -> String {
    match outcome {
        sync::SyncOutcome::SkippedFresh => "already fresh".to_string(),
        sync::SyncOutcome::Stopped(_) => "stopped".to_string(),
        sync::SyncOutcome::Completed {
            clone,
            collection,
            index,
        } => format!(
            "clone {}, collection {}, index {}",
            match clone {
                sync::CloneOutcome::Bootstrapped => "bootstrapped",
                sync::CloneOutcome::Pulled => "pulled",
            },
            match collection {
                sync::CollectionState::AlreadyPresent => "already present",
                sync::CollectionState::Created => "created",
                sync::CollectionState::Unavailable { .. } => "unavailable",
            },
            match index {
                crate::status::IndexStatus::Available { .. } => "available",
                crate::status::IndexStatus::Unavailable { .. } => "unavailable",
            },
        ),
    }
}

fn domain_json(domain: &DomainSection) -> Value {
    serde_json::json!({
        "name": domain.name,
        "owner": domain.owner,
        "topics": domain.topics,
        "summary": domain.summary,
    })
}

impl Render for DomainsReport {
    fn render_text(&self, options: &RenderOptions) -> String {
        let mut lines = Vec::new();
        lines.push("kaibo domains".to_string());

        lines.push(match &self.self_heal {
            None => "self-heal: not needed".to_string(),
            Some(outcome) => format!("self-heal: ran ({})", self_heal_summary(outcome)),
        });

        match &self.outcome {
            DomainsOutcome::SelfHealFailed { findings, .. } => {
                lines.push("result: self-heal failed".to_string());
                for finding in findings {
                    match &finding.fix {
                        Some(fix) => {
                            lines.push(format!("  - {} -> next: `{fix}`", finding.message))
                        }
                        None => lines.push(format!("  - {}", finding.message)),
                    }
                }
            }
            DomainsOutcome::MocUnavailable { detail } => {
                lines.push(format!("result: moc unavailable ({detail})"));
                lines.push("  - next: `kaibo sync`".to_string());
            }
            DomainsOutcome::NoDomains => {
                lines.push("result: gap, no domains listed in the root MOC".to_string());
            }
            DomainsOutcome::Listed { domains } => {
                lines.push(format!("result: {} domain(s)", domains.len()));
                for domain in domains {
                    lines.push(format!(
                        "- {} (owner {})",
                        domain.name,
                        domain.owner.as_deref().unwrap_or("unknown"),
                    ));
                    lines.push(format!(
                        "  topics: {}",
                        if domain.topics.is_empty() {
                            "none listed".to_string()
                        } else {
                            domain.topics.join(", ")
                        }
                    ));
                    lines.push(format!(
                        "  summary: {}",
                        domain.summary.as_deref().unwrap_or("none")
                    ));
                }
            }
        }

        if options.full {
            lines.push(format!("exit code: {}", self.exit_code().code()));
        }

        lines.join("\n")
    }

    fn render_json(&self) -> Value {
        let outcome = match &self.outcome {
            DomainsOutcome::SelfHealFailed { findings, .. } => serde_json::json!({
                "state": "self_heal_failed",
                "findings": findings.iter().map(|f| serde_json::json!({"message": f.message, "fix": f.fix})).collect::<Vec<_>>(),
            }),
            DomainsOutcome::MocUnavailable { detail } => serde_json::json!({
                "state": "moc_unavailable",
                "detail": detail,
            }),
            DomainsOutcome::NoDomains => serde_json::json!({
                "state": "no_domains",
            }),
            DomainsOutcome::Listed { domains } => serde_json::json!({
                "state": "listed",
                "domains": domains.iter().map(domain_json).collect::<Vec<_>>(),
            }),
        };

        serde_json::json!({
            "self_heal": self.self_heal.as_ref().map(|outcome| serde_json::json!({
                "ran": true,
                "summary": self_heal_summary(outcome),
            })),
            "outcome": outcome,
            "exit_code": self.exit_code().code(),
        })
    }
}

#[cfg(test)]
mod tests;
