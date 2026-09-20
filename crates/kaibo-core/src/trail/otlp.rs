//! The second sink: the same wide event, pushed to an OTLP collector.
//!
//! Compiled only under the `otlp` feature (issue #69). The file sink runs
//! either way, so a machine with no collector keeps a greppable trail - ADR
//! 0002's degradation ladder applies to the trail as much as to the corpus.
//!
//! Three choices worth knowing before changing anything here:
//!
//! - **Logs, not spans.** ADR 0016. The Rust Logs API and SDK are Stable
//!   while traces are Beta, OTel deprecated the Span Events API in March
//!   2026 in favour of log-based events, and kaibo's only edges are two
//!   subprocess calls whose timings fit on the event as attributes.
//! - **`SimpleLogProcessor`, not `BatchLogProcessor`.** kaibo exits in well
//!   under a second. A batch processor's background thread can fail to flush
//!   before the process goes away, and drops the record silently. One event
//!   per invocation makes synchronous export correct by construction.
//! - **The endpoint is the SDK's to resolve**, from the standard
//!   `OTEL_EXPORTER_OTLP_*` variables. kaibo decides only whether an exporter
//!   is built at all - see [`crate::config::Config::otlp`] for why that gate
//!   is deliberately not the environment's to open.

use std::time::{Duration, UNIX_EPOCH};

use opentelemetry::logs::{AnyValue, LogRecord, Logger, LoggerProvider, Severity};
use opentelemetry::{Key, KeyValue};
use opentelemetry_otlp::{LogExporter, Protocol, WithExportConfig};
use opentelemetry_sdk::Resource;
use opentelemetry_sdk::logs::{SdkLoggerProvider, SimpleLogProcessor};

use crate::config::OtlpTarget;
use crate::event::Event;
use crate::trail::TrailWrite;

/// Push one event and shut the provider down again.
///
/// A whole provider per invocation looks wasteful and is not: the process
/// handles exactly one event and then exits, so there is nothing for a
/// long-lived provider to amortise.
///
/// A collector that is down costs at most `target.timeout`, because that is
/// what the exporter is built with. The failure is reported, never returned
/// as an error: [`TrailWrite`] is not a `Result` precisely so that an
/// unreachable collector cannot become a verb's exit code.
pub fn export(target: &OtlpTarget, event: &Event) -> TrailWrite {
    let exporter = match LogExporter::builder()
        .with_http()
        .with_protocol(Protocol::HttpBinary)
        .with_timeout(target.timeout)
        .build()
    {
        Ok(exporter) => exporter,
        Err(source) => return failed(format!("could not build the OTLP exporter: {source}")),
    };

    let provider = SdkLoggerProvider::builder()
        .with_resource(
            Resource::builder()
                .with_service_name("kaibo")
                .with_attribute(KeyValue::new("service.version", env!("CARGO_PKG_VERSION")))
                .build(),
        )
        .with_log_processor(SimpleLogProcessor::new(exporter))
        .build();

    let logger = provider.logger("kaibo");
    let mut record = logger.create_log_record();
    record.set_event_name(event.event_name);
    record.set_timestamp(UNIX_EPOCH + duration_of(event.time_unix_ms));
    // An invocation that completed is not a problem report. `Gap` is a
    // result, not a failure, so nothing here is ever raised to Warn or
    // Error - the outcome is an attribute and the reader classifies.
    record.set_severity_number(Severity::Info);
    record.set_severity_text("INFO");

    for (key, value) in event.attribute_map() {
        if let Some(value) = any_value(value) {
            record.add_attribute(Key::new(key), value);
        }
    }
    logger.emit(record);

    match provider.shutdown_with_timeout(target.timeout) {
        Ok(()) => TrailWrite::Written,
        Err(source) => failed(format!("could not flush the OTLP exporter: {source}")),
    }
}

fn failed(detail: String) -> TrailWrite {
    TrailWrite::Failed {
        detail: format!(
            "{detail}; the local trail was written regardless, and \
             `otlp_export = false` in ~/.kaibo/config.toml stops trying"
        ),
    }
}

/// Saturating rather than wrapping: a clock far enough ahead to overflow a
/// `u64` of milliseconds should give a timestamp at the end of time, not one
/// near the epoch.
fn duration_of(time_unix_ms: u128) -> Duration {
    Duration::from_millis(u64::try_from(time_unix_ms).unwrap_or(u64::MAX))
}

/// The event's own JSON encoding is the single source of attribute names and
/// types, so this only has to carry each scalar across. `null` yields `None`:
/// an absent attribute is absent in both sinks, never present-and-empty.
fn any_value(value: serde_json::Value) -> Option<AnyValue> {
    match value {
        serde_json::Value::Null => None,
        serde_json::Value::Bool(value) => Some(AnyValue::Boolean(value)),
        serde_json::Value::String(value) => Some(AnyValue::String(value.into())),
        serde_json::Value::Number(value) => value
            .as_i64()
            .map(AnyValue::Int)
            .or_else(|| value.as_f64().map(AnyValue::Double)),
        serde_json::Value::Array(values) => Some(AnyValue::ListAny(Box::new(
            values.into_iter().filter_map(any_value).collect(),
        ))),
        // No attribute is a nested object today, and one arriving silently
        // flattened into nothing would be worse than one arriving as text.
        serde_json::Value::Object(_) => Some(AnyValue::String(value_to_string(&value).into())),
    }
}

fn value_to_string(value: &serde_json::Value) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| String::from("<unserialisable>"))
}

#[cfg(test)]
mod tests;
