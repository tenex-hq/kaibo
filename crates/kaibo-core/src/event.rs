//! One wide event per invocation: the paper trail's record shape.
//!
//! Every attribute here answers a question recorded on issue #40, and an
//! attribute serving no question does not belong. Two questions are known to
//! be unanswerable and deliberately get no field that would imply otherwise:
//! whether the answer was *useful*, and what fraction of domain entries loaded
//! doctrine. The second is the denominator finding in ADR 0015 - a tool cannot
//! record its own non-invocation - and belongs to the activation eval suite.
//!
//! The encoding is OpenTelemetry's data model (an event name, a resource, and
//! a flat attribute set) rather than byte-exact OTLP/JSON, whose array-of-typed
//! key-value form is markedly worse to read with `jq` or `grep`. ADR 0002's
//! degradation ladder applies to the trail as much as to the corpus: the file
//! has to stay legible on a machine with no collector. The OTLP exporter in
//! #69 translates this into the wire format; it does not change what is
//! recorded.
//!
//! Custom attributes are namespaced `kaibo.`, never `otel.`, which the
//! specification reserves.

use serde::Serialize;

use crate::doctrine::{DoctrineOutcome, DoctrineReport};
use crate::error::ExitCode;
use crate::query::{MocInventory, QueryOutcome, QueryReport};
use crate::sync::SyncOutcome;

/// What the invocation produced, collapsed to the distinction the trail
/// exists to count. `Gap` is the exit-3 signal and is a *result*, never a
/// failure - conflating the two is what the exit-code contract forbids.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum EventOutcome {
    Hit,
    Gap,
    Error,
}

/// Whether self-heal ran, and how it went. `git` only runs on the two
/// non-`None` arms, which is what makes this a latency explanation and not
/// just a status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum SelfHeal {
    None,
    Ok,
    Failed,
}

/// Observed facts about the caller, never a claimed identity.
///
/// ADR 0015: anything settable is settable by the eval harness, by CI, and by
/// an agent composing a command line, so a self-declared label reintroduces
/// the test-traffic contamination that sank the predecessor instrument. These
/// are things the process can see about itself without trusting anyone, and
/// the reader classifies.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Caller {
    /// `debug` builds are almost always someone's `./target/debug/kaibo` run.
    #[serde(rename = "kaibo.build_profile")]
    pub build_profile: &'static str,
    /// False under a pipe, a harness, or CI.
    #[serde(rename = "kaibo.stdout_tty")]
    pub stdout_tty: bool,
}

impl Caller {
    /// `debug_assertions` is the only build-profile signal available without
    /// a build script, and it is exactly the one that separates a dev run
    /// from an installed binary.
    pub fn observed(stdout_tty: bool) -> Caller {
        Caller {
            build_profile: if cfg!(debug_assertions) {
                "debug"
            } else {
                "release"
            },
            stdout_tty,
        }
    }
}

/// The attribute set. Field order here is the key order in the JSONL line,
/// which keeps hand-reading a file of these tolerable.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Attributes {
    #[serde(rename = "kaibo.verb")]
    pub verb: &'static str,
    /// The question, or the domain a `doctrine` load asked for.
    #[serde(rename = "kaibo.subject")]
    pub subject: String,
    /// `doctrine` has a domain by construction. `query` does not: it searches
    /// corpus-wide, so query-side gaps have nothing to group by beyond their
    /// text, and `kaibo.moc_domains` carries what the corpus did offer.
    #[serde(rename = "kaibo.domain", skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(rename = "kaibo.outcome")]
    pub outcome: EventOutcome,
    #[serde(rename = "kaibo.hit_count")]
    pub hit_count: usize,

    #[serde(
        rename = "kaibo.raw_hit_count",
        skip_serializing_if = "Option::is_none"
    )]
    pub raw_hit_count: Option<usize>,
    #[serde(
        rename = "kaibo.withheld_draft",
        skip_serializing_if = "Option::is_none"
    )]
    pub withheld_draft: Option<usize>,
    #[serde(
        rename = "kaibo.withheld_unverified",
        skip_serializing_if = "Option::is_none"
    )]
    pub withheld_unverified: Option<usize>,
    #[serde(
        rename = "kaibo.unaddressable",
        skip_serializing_if = "Option::is_none"
    )]
    pub unaddressable: Option<usize>,

    #[serde(rename = "kaibo.top_hit_path", skip_serializing_if = "Option::is_none")]
    pub top_hit_path: Option<String>,
    #[serde(
        rename = "kaibo.top_hit_score",
        skip_serializing_if = "Option::is_none"
    )]
    pub top_hit_score: Option<f64>,
    /// Only on a gap: which domains existed when the question found nothing.
    #[serde(rename = "kaibo.moc_domains", skip_serializing_if = "Option::is_none")]
    pub moc_domains: Option<Vec<String>>,

    #[serde(rename = "kaibo.self_heal")]
    pub self_heal: SelfHeal,
    #[serde(rename = "kaibo.duration_ms")]
    pub duration_ms: u64,

    #[serde(flatten)]
    pub caller: Caller,

    #[serde(rename = "process.exit.code")]
    pub exit_code: u8,
}

/// One wide event, ready to serialise as a JSONL line.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Event {
    /// Milliseconds since the Unix epoch, taken from the injected clock.
    pub time_unix_ms: u128,
    /// `kaibo.query` or `kaibo.doctrine`.
    pub event_name: &'static str,
    pub attributes: Attributes,
}

impl Event {
    /// One line, no trailing newline. The caller appends the separator, so a
    /// sink that batches is not forced to strip one.
    pub fn to_jsonl(&self) -> String {
        // Every field is a plain scalar, a string, or a Vec of strings, so
        // this cannot fail on non-string map keys or non-finite floats that
        // serde_json rejects. A score is finite by the time a Hit holds it.
        serde_json::to_string(self).expect("event attributes are all plain JSON scalars")
    }

    pub fn from_query(
        report: &QueryReport,
        time_unix_ms: u128,
        duration_ms: u64,
        caller: Caller,
    ) -> Event {
        let census = report.census;
        let (outcome, hit_count, top_hit_path, top_hit_score, moc_domains) = match &report.outcome {
            QueryOutcome::Hits(hits) => (
                EventOutcome::Hit,
                hits.len(),
                hits.first().map(|hit| hit.path.clone()),
                hits.first().map(|hit| hit.score),
                None,
            ),
            QueryOutcome::NoHits { moc } => (
                EventOutcome::Gap,
                0,
                None,
                None,
                match moc {
                    MocInventory::Domains(domains) => Some(domains.clone()),
                    MocInventory::Unavailable { .. } => None,
                },
            ),
            _ => (EventOutcome::Error, 0, None, None, None),
        };

        Event {
            time_unix_ms,
            event_name: "kaibo.query",
            attributes: Attributes {
                verb: "query",
                subject: report.question.clone(),
                domain: None,
                outcome,
                hit_count,
                raw_hit_count: Some(census.raw),
                withheld_draft: Some(census.withheld_draft),
                withheld_unverified: Some(census.withheld_unverified),
                unaddressable: Some(census.unaddressable),
                top_hit_path,
                top_hit_score,
                moc_domains,
                self_heal: self_heal_of(report.self_heal.as_ref()),
                duration_ms,
                caller,
                exit_code: report.exit_code().code(),
            },
        }
    }

    pub fn from_doctrine(
        report: &DoctrineReport,
        time_unix_ms: u128,
        duration_ms: u64,
        caller: Caller,
    ) -> Event {
        // `doctrine` reads the MOC and pages directly and never calls qmd, so
        // there is no census and no score to record - the Options stay None
        // rather than being filled with a misleading zero.
        let (outcome, hit_count, moc_domains) = match &report.outcome {
            DoctrineOutcome::Loaded { pages, .. } => (EventOutcome::Hit, pages.len(), None),
            DoctrineOutcome::UnknownDomain { available_domains } => {
                (EventOutcome::Gap, 0, Some(available_domains.clone()))
            }
            DoctrineOutcome::NoCurrentPages { .. } => (EventOutcome::Gap, 0, None),
            _ => (EventOutcome::Error, 0, None),
        };

        Event {
            time_unix_ms,
            event_name: "kaibo.doctrine",
            attributes: Attributes {
                verb: "doctrine",
                subject: report.domain.clone(),
                domain: Some(report.domain.clone()),
                outcome,
                hit_count,
                raw_hit_count: None,
                withheld_draft: None,
                withheld_unverified: None,
                unaddressable: None,
                top_hit_path: None,
                top_hit_score: None,
                moc_domains,
                self_heal: self_heal_of(report.self_heal.as_ref()),
                duration_ms,
                caller,
                exit_code: report.exit_code().code(),
            },
        }
    }
}

/// `SkippedFresh` means the freshness probe ran and found nothing to do, so
/// no `git` fetch happened: that is `None`, not `Ok`, because the point of
/// the field is explaining latency.
fn self_heal_of(outcome: Option<&SyncOutcome>) -> SelfHeal {
    match outcome {
        None | Some(SyncOutcome::SkippedFresh) => SelfHeal::None,
        Some(SyncOutcome::Completed { .. }) => SelfHeal::Ok,
        Some(SyncOutcome::Stopped(_)) => SelfHeal::Failed,
    }
}

/// Unused by the constructors above, but the exit-code contract is what makes
/// `Gap` distinguishable from `Error` downstream, so it is asserted here
/// rather than assumed.
#[allow(dead_code)]
const GAP_IS_EXIT_THREE: () = assert!(ExitCode::NoHits as u8 == 3);

#[cfg(test)]
mod tests;
