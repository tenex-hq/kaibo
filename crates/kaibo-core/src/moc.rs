//! Shared root-MOC parsing: the domain inventory `kaibo domains` lists in
//! full, and the single domain section `kaibo doctrine` loads.
//!
//! Reads `<clone_path>/_index.md` and scans it for top-level `## ` headings,
//! each naming one domain folder, with three bullet fields read out of the
//! section body underneath: `- **owner:**`, `- **topics:**` (a
//! comma-separated list), and `- **summary:**`. This is a lenient
//! heading-and-bullet scan, not a strict schema parse - same "a degraded
//! fact beats a guess" spirit as `status::parse_qmd_status`: a missing or
//! unrecognised bullet leaves that field `None`/empty rather than guessed,
//! and a section with none of the three still gets its heading recorded.
//!
//! Every scalar this module extracts (`name`, `owner`, each topic,
//! `summary`) is corpus content that both `doctrine` and `domains` print
//! unfenced - a bare heading or a short label, not a retrieved snippet - so
//! each is run through [`trust::strip_control_chars`] at parse time, the
//! same ingest-time treatment `query::parse_domain_headings` already gives
//! MOC headings, so a page contributor cannot use an embedded newline or
//! other control character to forge an extra line of kaibo's own output.
//! This module does not fence anything: fencing is for retrieved page
//! *content* (a snippet, a page body), which callers of this module fence
//! themselves via [`trust::fence`] - not for a MOC section's own short
//! metadata fields.

use std::path::Path;

use crate::trust;

/// One domain section of the root MOC: its heading name, plus whichever of
/// `owner`/`topics`/`summary` bullets were present underneath it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DomainSection {
    pub name: String,
    pub owner: Option<String>,
    pub topics: Vec<String>,
    pub summary: Option<String>,
}

/// The root MOC could not be read at all - distinct from a domain simply
/// not being one of its sections, which is a gap, not a broken read. See
/// the module docs on `doctrine`/`domains` for why the two are reported
/// with different exit codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MocUnreadable {
    pub detail: String,
}

/// Read and parse `<clone_path>/_index.md`. `Err` only when the file
/// itself could not be read (missing, permissions, not valid UTF-8); a
/// file that parses but contains no `## ` headings at all is `Ok(vec![])`,
/// not an error - an empty MOC is a fact about the corpus, not a read
/// failure.
pub fn read_domain_sections(clone_path: &Path) -> Result<Vec<DomainSection>, MocUnreadable> {
    let path = clone_path.join("_index.md");
    let contents = std::fs::read_to_string(&path).map_err(|err| MocUnreadable {
        detail: err.to_string(),
    })?;
    Ok(parse_domain_sections(&contents))
}

/// Split `contents` at each top-level `## ` heading and build one
/// [`DomainSection`] per heading, in document order. A line is only ever
/// treated as a heading when it starts with exactly `## ` (a sub-heading
/// like `### ` does not open a new section); everything between one
/// heading and the next (or the end of the document) is that section's
/// body, scanned for bullet fields by [`build_section`].
fn parse_domain_sections(contents: &str) -> Vec<DomainSection> {
    let mut sections = Vec::new();
    let mut current: Option<(&str, Vec<&str>)> = None;

    for line in contents.lines() {
        if let Some(name) = line.strip_prefix("## ") {
            if let Some((name, lines)) = current.take() {
                sections.push(build_section(name, &lines));
            }
            current = Some((name.trim(), Vec::new()));
        } else if let Some((_, lines)) = current.as_mut() {
            lines.push(line);
        }
    }
    if let Some((name, lines)) = current.take() {
        sections.push(build_section(name, &lines));
    }

    sections
}

fn build_section(name: &str, lines: &[&str]) -> DomainSection {
    let mut owner = None;
    let mut topics = Vec::new();
    let mut summary = None;

    for line in lines {
        if owner.is_none()
            && let Some(value) = bullet_value(line, "owner")
        {
            owner = Some(trust::strip_control_chars(value));
            continue;
        }
        if topics.is_empty()
            && let Some(value) = bullet_value(line, "topics")
        {
            topics = parse_topics(value);
            continue;
        }
        if summary.is_none()
            && let Some(value) = bullet_value(line, "summary")
        {
            summary = Some(trust::strip_control_chars(value));
        }
    }

    DomainSection {
        name: trust::strip_control_chars(name),
        owner,
        topics,
        summary,
    }
}

/// The value of a bullet line shaped like `- **<label>:** rest of line`, or
/// `None` if `line` isn't that exact bullet (a different label, a plain
/// paragraph, ...). `label` itself is a compile-time literal this module
/// controls, never corpus content, so it needs no sanitising.
fn bullet_value<'a>(line: &'a str, label: &str) -> Option<&'a str> {
    let trimmed = line.trim();
    let rest = trimmed.strip_prefix("- ")?;
    let marker = format!("**{label}:**");
    let rest = rest.strip_prefix(marker.as_str())?;
    Some(rest.trim())
}

/// A `- **topics:**` value is a comma-separated list; each entry is
/// trimmed and control-character-stripped independently, and an empty
/// entry (e.g. a trailing comma) is dropped rather than kept as `""`.
fn parse_topics(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(trust::strip_control_chars)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_moc(dir: &Path, contents: &str) {
        std::fs::write(dir.join("_index.md"), contents).unwrap();
    }

    #[test]
    fn reads_owner_topics_and_summary_for_each_domain_section() {
        let tmp = tempfile::tempdir().unwrap();
        write_moc(
            tmp.path(),
            "---\ntype: index\n---\n\n\
             ## kaibo\n\n\
             - **owner:** @someone\n\
             - **domain:** Kaibo itself\n\
             - **topics:** kaibo, knowledge backoffice, monorepo\n\
             - **summary:** Kaibo's own knowledge, captured in Kaibo.\n\n\
             ## observability\n\n\
             - **owner:** @someone\n\
             - **topics:** otel, tracing\n\
             - **summary:** One reference page.\n",
        );

        let sections = read_domain_sections(tmp.path()).unwrap();

        assert_eq!(
            sections,
            vec![
                DomainSection {
                    name: "kaibo".to_string(),
                    owner: Some("@someone".to_string()),
                    topics: vec![
                        "kaibo".to_string(),
                        "knowledge backoffice".to_string(),
                        "monorepo".to_string(),
                    ],
                    summary: Some("Kaibo's own knowledge, captured in Kaibo.".to_string()),
                },
                DomainSection {
                    name: "observability".to_string(),
                    owner: Some("@someone".to_string()),
                    topics: vec!["otel".to_string(), "tracing".to_string()],
                    summary: Some("One reference page.".to_string()),
                },
            ]
        );
    }

    #[test]
    fn a_section_with_no_recognised_bullets_still_records_its_heading() {
        let tmp = tempfile::tempdir().unwrap();
        write_moc(
            tmp.path(),
            "---\ntype: index\n---\n\n## empty-domain\n\nJust a paragraph, no bullets.\n",
        );

        let sections = read_domain_sections(tmp.path()).unwrap();

        assert_eq!(
            sections,
            vec![DomainSection {
                name: "empty-domain".to_string(),
                owner: None,
                topics: Vec::new(),
                summary: None,
            }]
        );
    }

    #[test]
    fn a_document_with_no_domain_headings_parses_as_an_empty_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        write_moc(
            tmp.path(),
            "---\ntype: index\n---\n\nJust prose, no headings.\n",
        );

        assert_eq!(read_domain_sections(tmp.path()).unwrap(), Vec::new());
    }

    #[test]
    fn a_missing_index_file_is_reported_as_unreadable_not_an_empty_inventory() {
        let tmp = tempfile::tempdir().unwrap();
        // Deliberately no _index.md written at all.

        let err = read_domain_sections(tmp.path()).unwrap_err();

        assert!(!err.detail.is_empty());
    }

    /// A heading carrying an embedded control character (here `\r`, which
    /// `str::lines()` does not split on, so it stays part of one physical
    /// heading line) must not reach a consumer's own output verbatim - the
    /// same forgery `query`'s MOC-heading handling defends against.
    #[test]
    fn a_control_character_in_a_heading_is_stripped_from_the_parsed_name() {
        let tmp = tempfile::tempdir().unwrap();
        write_moc(
            tmp.path(),
            "---\ntype: index\n---\n\n## kaibo\rforged: line\n\nbody\n",
        );

        let sections = read_domain_sections(tmp.path()).unwrap();

        assert_eq!(sections.len(), 1);
        assert!(!sections[0].name.contains('\r'));
    }

    /// Same forgery, in the `owner`/`summary` bullet values rather than the
    /// heading: a contributor cannot embed a control character in either
    /// field to inject an extra line into a consumer's own output.
    #[test]
    fn a_control_character_in_a_bullet_value_is_stripped() {
        let tmp = tempfile::tempdir().unwrap();
        write_moc(
            tmp.path(),
            "---\ntype: index\n---\n\n## kaibo\n\n\
             - **owner:** @someone\rforged: line\n\
             - **summary:** fine\rforged: line\n",
        );

        let sections = read_domain_sections(tmp.path()).unwrap();

        assert_eq!(sections.len(), 1);
        assert!(!sections[0].owner.as_ref().unwrap().contains('\r'));
        assert!(!sections[0].summary.as_ref().unwrap().contains('\r'));
    }

    #[test]
    fn a_trailing_comma_in_topics_does_not_produce_an_empty_topic() {
        let tmp = tempfile::tempdir().unwrap();
        write_moc(
            tmp.path(),
            "---\ntype: index\n---\n\n## kaibo\n\n- **topics:** one, two, \n",
        );

        let sections = read_domain_sections(tmp.path()).unwrap();

        assert_eq!(
            sections[0].topics,
            vec!["one".to_string(), "two".to_string()]
        );
    }
}
