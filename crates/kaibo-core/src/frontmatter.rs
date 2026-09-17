//! Frontmatter model: YAML frontmatter delimited by `---` lines at the start
//! of a markdown document, plus the body that follows.
//!
//! Known keys are typed. Everything else round-trips through
//! [`Frontmatter::extra`] so a document using keys this model has never
//! heard of survives a parse-then-serialize cycle unchanged, in the same
//! order it isn't tracking. This passthrough is deliberate: a future epic
//! adds `binding`, `severity`, `applies_to` and a `checks` block additively,
//! and passthrough is what makes that a field addition instead of a
//! rewrite.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::error::{ExitCode, ExitCoded};

/// Lifecycle status of a document.
///
/// Unknown values surface as [`Status::Unknown`] rather than failing the
/// parse: a corpus is authored by people, and a typo or a not-yet-modeled
/// status in one page's frontmatter should not break every tool that reads
/// the corpus.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Status {
    Draft,
    Current,
    Deprecated,
    /// Any value that isn't one of the above, preserved verbatim.
    Unknown(String),
}

impl Serialize for Status {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = match self {
            Status::Draft => "draft",
            Status::Current => "current",
            Status::Deprecated => "deprecated",
            Status::Unknown(s) => s.as_str(),
        };
        serializer.serialize_str(s)
    }
}

impl<'de> Deserialize<'de> for Status {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(match s.as_str() {
            "draft" => Status::Draft,
            "current" => Status::Current,
            "deprecated" => Status::Deprecated,
            _ => Status::Unknown(s),
        })
    }
}

/// A calendar date in `YYYY-MM-DD` form.
///
/// Deliberately minimal: this crate only needs to read and round-trip a
/// date, not do date arithmetic, so it stores plain numeric fields instead
/// of pulling in a date/time crate for that.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u16,
    pub month: u8,
    pub day: u8,
}

impl Date {
    pub fn parse(s: &str) -> Result<Date, FrontmatterError> {
        let mut parts = s.splitn(3, '-');
        let (Some(y), Some(m), Some(d), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(FrontmatterError::InvalidDate(s.to_string()));
        };
        let invalid = || FrontmatterError::InvalidDate(s.to_string());
        Ok(Date {
            year: y.parse().map_err(|_| invalid())?,
            month: m.parse().map_err(|_| invalid())?,
            day: d.parse().map_err(|_| invalid())?,
        })
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for Date {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Date::parse(&s).map_err(serde::de::Error::custom)
    }
}

/// Frontmatter fields this model knows about, plus passthrough for the rest.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct Frontmatter {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none", default)]
    pub doc_type: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub title: Option<String>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub tags: Option<Vec<String>>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub status: Option<Status>,

    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub updated: Option<Date>,

    /// Unknown keys, preserved verbatim (including nested structures) so a
    /// parse-then-serialize round trip loses nothing this model doesn't
    /// already model explicitly.
    #[serde(flatten)]
    pub extra: BTreeMap<String, serde_yaml_ng::Value>,
}

/// A parsed markdown document: typed (+ passthrough) frontmatter, and body.
#[derive(Debug, Clone, PartialEq)]
pub struct Document {
    pub frontmatter: Frontmatter,
    pub body: String,
}

#[derive(Debug, thiserror::Error)]
pub enum FrontmatterError {
    #[error(
        "frontmatter block is not closed by a `---` line; add one after the \
         frontmatter fields, before the document body"
    )]
    Unterminated,

    #[error("invalid YAML in frontmatter: {0}")]
    InvalidYaml(#[from] serde_yaml_ng::Error),

    #[error("invalid date {0:?}: expected YYYY-MM-DD")]
    InvalidDate(String),
}

impl ExitCoded for FrontmatterError {
    fn exit_code(&self) -> ExitCode {
        // Malformed corpus content is bad input, not a kaibo bug.
        ExitCode::Usage
    }
}

/// Parse a markdown document with an optional leading YAML frontmatter
/// block delimited by `---` lines.
///
/// A document that does not start with a `---` line has no frontmatter: it
/// parses as `Frontmatter::default()` with the whole input as body.
pub fn parse(input: &str) -> Result<Document, FrontmatterError> {
    let mut lines = input.split('\n');

    let Some(first) = lines.next() else {
        return Ok(Document {
            frontmatter: Frontmatter::default(),
            body: String::new(),
        });
    };

    if first.trim_end_matches('\r') != "---" {
        return Ok(Document {
            frontmatter: Frontmatter::default(),
            body: input.to_string(),
        });
    }

    let mut yaml_lines: Vec<&str> = Vec::new();
    let mut closed = false;
    for line in lines.by_ref() {
        if line.trim_end_matches('\r') == "---" {
            closed = true;
            break;
        }
        yaml_lines.push(line);
    }

    if !closed {
        return Err(FrontmatterError::Unterminated);
    }

    let yaml = yaml_lines.join("\n");
    let frontmatter: Frontmatter = if yaml.trim().is_empty() {
        Frontmatter::default()
    } else {
        serde_yaml_ng::from_str(&yaml)?
    };

    let body = lines.collect::<Vec<&str>>().join("\n");

    Ok(Document { frontmatter, body })
}

/// Serialize a document back to `---`-delimited frontmatter followed by the
/// body, the inverse of [`parse`].
pub fn serialize(doc: &Document) -> Result<String, FrontmatterError> {
    let yaml = serde_yaml_ng::to_string(&doc.frontmatter)?;
    Ok(format!("---\n{yaml}---\n{}", doc.body))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Mandatory test: unknown frontmatter keys, including nested
    /// structures, survive a parse -> serialize round trip.
    #[test]
    fn unknown_keys_round_trip_through_parse_and_serialize() {
        let input = "---\n\
title: Example\n\
type: reference\n\
tags:\n\
  - one\n\
  - two\n\
status: current\n\
updated: 2024-01-15\n\
binding: required\n\
severity: high\n\
checks:\n\
  - id: must-have-x\n\
    when: always\n\
applies_to:\n\
  - service-a\n\
  - service-b\n\
---\n\
Body text.\n\
Second line.\n";

        let doc = parse(input).unwrap();

        assert_eq!(doc.frontmatter.title.as_deref(), Some("Example"));
        assert_eq!(doc.frontmatter.doc_type.as_deref(), Some("reference"));
        assert_eq!(doc.frontmatter.status, Some(Status::Current));
        assert_eq!(
            doc.frontmatter.updated,
            Some(Date {
                year: 2024,
                month: 1,
                day: 15
            })
        );
        assert_eq!(
            doc.frontmatter
                .extra
                .get("binding")
                .and_then(|v| v.as_str()),
            Some("required")
        );
        assert!(doc.frontmatter.extra.contains_key("checks"));
        assert_eq!(doc.body, "Body text.\nSecond line.\n");

        let round_tripped = serialize(&doc).unwrap();
        let doc2 = parse(&round_tripped).unwrap();

        assert_eq!(doc.frontmatter, doc2.frontmatter);
        assert_eq!(doc.body, doc2.body);
        // Nested structures under unknown keys survive intact, not just the
        // scalar ones.
        assert_eq!(
            doc.frontmatter.extra.get("checks"),
            doc2.frontmatter.extra.get("checks")
        );
        assert_eq!(
            doc.frontmatter.extra.get("applies_to"),
            doc2.frontmatter.extra.get("applies_to")
        );
    }

    #[test]
    fn unknown_status_value_surfaces_instead_of_failing() {
        let input = "---\nstatus: experimental\n---\nbody\n";
        let doc = parse(input).unwrap();
        assert_eq!(
            doc.frontmatter.status,
            Some(Status::Unknown("experimental".to_string()))
        );
    }

    #[test]
    fn document_without_frontmatter_parses_as_plain_body() {
        let input = "just a body, no frontmatter\n";
        let doc = parse(input).unwrap();
        assert_eq!(doc.frontmatter, Frontmatter::default());
        assert_eq!(doc.body, input);
    }

    #[test]
    fn unterminated_frontmatter_is_an_error() {
        let input = "---\ntitle: no closing delimiter\n";
        let err = parse(input).unwrap_err();
        assert!(matches!(err, FrontmatterError::Unterminated));
    }

    #[test]
    fn empty_input_parses_as_empty_document() {
        let doc = parse("").unwrap();
        assert_eq!(doc.frontmatter, Frontmatter::default());
        assert_eq!(doc.body, "");
    }
}
