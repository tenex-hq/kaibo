use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::*;
use crate::config::testing::ConfigBuilder;

/// A scratch `skills_dir` plus the `Config` pointing at it. Every test in
/// this module writes only inside the returned `TempDir`.
fn sandbox() -> (tempfile::TempDir, Config) {
    let root = tempfile::tempdir().expect("create temp root");
    let skills_dir = root.path().join("claude").join("skills");
    let config = ConfigBuilder::new(root.path().join("corpus"))
        .skills_dir(&skills_dir)
        .build();
    (root, config)
}

fn install(config: &Config, version: &str) -> InstallReport {
    InstallVerb::new(config, version, InstallMode::Install).apply()
}

fn uninstall(config: &Config) -> InstallReport {
    InstallVerb::new(config, "0.0.0", InstallMode::Uninstall).apply()
}

fn actions(report: &InstallReport) -> Vec<(PathBuf, FileAction)> {
    match &report.outcome {
        InstallOutcome::Ok { changes, .. } => changes
            .iter()
            .map(|change| (change.path.clone(), change.action))
            .collect(),
        InstallOutcome::Failed { detail, .. } => panic!("expected success, got failure: {detail}"),
    }
}

fn retained(report: &InstallReport) -> Vec<PathBuf> {
    match &report.outcome {
        InstallOutcome::Ok { retained, .. } => retained.clone(),
        InstallOutcome::Failed { detail, .. } => panic!("expected success, got failure: {detail}"),
    }
}

// --- the embedded prose itself ----------------------------------------

/// Terms that must never reach this public repository: a private
/// organisation name, an internal repository name, and an individual's
/// name. Held reversed, because a guard that spells out the secret it
/// guards has already leaked it - the repository would contain the very
/// strings this test exists to keep out of it.
const FORBIDDEN_REVERSED: [&str; 4] = [
    "eciffokcab-egdelwonk",
    "thgisni-ecived",
    "ossarg",
    "oninotna",
];

fn reverse(s: &str) -> String {
    s.chars().rev().collect()
}

/// Sweeps every skill the binary carries, not a list of three names: a
/// fourth skill added to `EMBEDDED_SKILLS` is covered the moment it is
/// added.
#[test]
fn no_embedded_skill_names_a_private_organisation_repository_or_person() {
    for skill in EMBEDDED_SKILLS {
        let haystack = skill.source.to_lowercase();
        for reversed in FORBIDDEN_REVERSED {
            let forbidden = reverse(reversed);
            assert!(
                !haystack.contains(&forbidden),
                "skill `{}` contains a term this public repository must never hold \
                 (reversed: {reversed})",
                skill.name,
            );
        }
    }
}

/// The same rule one level up from literals: a destination reaches kaibo
/// through configuration, so no skill needs to spell out a handle or a
/// URL, and a skill that starts to is drifting back to naming its target
/// in prose.
#[test]
fn no_embedded_skill_names_a_handle_or_a_url() {
    for skill in EMBEDDED_SKILLS {
        assert!(
            !skill.source.contains('@'),
            "skill `{}` contains an `@`, which is either a handle or an address; \
             a destination comes from config, never from prose",
            skill.name,
        );
        assert!(
            !skill.source.contains("://"),
            "skill `{}` contains a URL; a destination comes from config, never \
             from prose",
            skill.name,
        );
    }
}

/// A `SKILL.md` added to the vendored directory but never wired into
/// `EMBEDDED_SKILLS` would ship in neither the binary nor the install, and
/// nothing else would notice.
#[test]
fn every_vendored_skill_file_is_compiled_into_the_binary() {
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("skills");
    let mut on_disk: Vec<String> = fs::read_dir(&dir)
        .expect("read vendored skills dir")
        .flatten()
        .filter(|entry| entry.path().join("SKILL.md").is_file())
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .collect();
    on_disk.sort();

    let mut embedded: Vec<String> = EMBEDDED_SKILLS
        .iter()
        .map(|skill| skill.name.to_string())
        .collect();
    embedded.sort();

    assert_eq!(
        on_disk,
        embedded,
        "every directory under {} must appear in EMBEDDED_SKILLS and vice versa",
        dir.display()
    );
}

/// Claude Code resolves a skill by the `name` in its own frontmatter, not
/// by the directory it sits in, so the two drifting apart would install a
/// skill under a name nothing invokes.
#[test]
fn each_embedded_skill_declares_its_directory_name_in_its_frontmatter() {
    for skill in EMBEDDED_SKILLS {
        assert!(
            skill.source.starts_with("---\n"),
            "skill `{}` does not open with a frontmatter fence",
            skill.name
        );
        assert!(
            skill
                .source
                .lines()
                .any(|line| line == format!("name: {}", skill.name)),
            "skill `{}` does not declare `name: {}` in its frontmatter",
            skill.name,
            skill.name
        );
    }
}

#[test]
fn the_manifest_names_the_plugin_and_the_binary_version() {
    let parsed: Value =
        serde_json::from_str(&manifest_contents("1.2.3")).expect("manifest is valid JSON");

    assert_eq!(parsed["name"], "kaibo");
    assert_eq!(parsed["version"], "1.2.3");
}

// --- install ----------------------------------------------------------

#[test]
fn a_first_install_creates_the_manifest_and_every_skill() {
    let (root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");

    let report = install(&config, "0.1.0");

    assert_eq!(report.exit_code(), ExitCode::Success);
    assert!(
        actions(&report)
            .iter()
            .all(|(_, action)| *action == FileAction::Created),
        "every path is new on a first install, got: {:?}",
        actions(&report)
    );
    assert_eq!(
        fs::read_to_string(layout.manifest_path()).expect("manifest was written"),
        manifest_contents("0.1.0")
    );
    for skill in EMBEDDED_SKILLS {
        assert_eq!(
            fs::read_to_string(layout.skill_path(skill.name)).expect("skill was written"),
            skill.source,
            "skill `{}` was not written verbatim",
            skill.name
        );
    }
    drop(root);
}

/// The install path is a plugin directory, not a bare skill directory:
/// the manifest beside `skills/` is what keeps `/kaibo:query` from
/// degrading to `/query`.
#[test]
fn the_installed_tree_is_a_plugin_directory_carrying_a_manifest() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");

    install(&config, "0.1.0");

    assert_eq!(
        layout.manifest_path(),
        config
            .skills_dir()
            .expect("the sandbox config has a skills dir")
            .join("kaibo")
            .join(".claude-plugin")
            .join("plugin.json")
    );
    assert_eq!(
        layout.skill_path("query"),
        config
            .skills_dir()
            .expect("the sandbox config has a skills dir")
            .join("kaibo")
            .join("skills")
            .join("query")
            .join("SKILL.md")
    );
}

#[test]
fn installing_twice_reports_every_file_unchanged() {
    let (_root, config) = sandbox();

    install(&config, "0.1.0");
    let second = install(&config, "0.1.0");

    assert!(
        actions(&second)
            .iter()
            .all(|(_, action)| *action == FileAction::Unchanged),
        "a re-install of the same version changes nothing, got: {:?}",
        actions(&second)
    );
}

#[test]
fn a_hand_edited_skill_file_is_restored_by_a_re_install() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.1.0");
    fs::write(layout.skill_path("query"), "hand-edited\n").expect("edit the installed skill");

    let report = install(&config, "0.1.0");

    let query_action = actions(&report)
        .into_iter()
        .find(|(path, _)| *path == layout.skill_path("query"))
        .expect("the query skill is one of the paths install reports")
        .1;
    assert_eq!(query_action, FileAction::Updated);
    assert_eq!(
        fs::read_to_string(layout.skill_path("query")).expect("skill is readable"),
        EMBEDDED_SKILLS
            .iter()
            .find(|skill| skill.name == "query")
            .expect("query is embedded")
            .source
    );
}

#[test]
fn an_upgrade_rewrites_the_manifest_with_the_new_version() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.1.0");

    let report = install(&config, "0.2.0");

    let manifest_action = actions(&report)
        .into_iter()
        .find(|(path, _)| *path == layout.manifest_path())
        .expect("the manifest is one of the paths install reports")
        .1;
    assert_eq!(manifest_action, FileAction::Updated);
    assert_eq!(
        fs::read_to_string(layout.manifest_path()).expect("manifest is readable"),
        manifest_contents("0.2.0")
    );
}

#[test]
fn an_explain_run_reports_the_actions_it_would_take_and_writes_nothing() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");

    let report = InstallVerb::new(&config, "0.1.0", InstallMode::Install).plan();

    assert!(!report.applied);
    assert!(
        actions(&report)
            .iter()
            .all(|(_, action)| *action == FileAction::Created),
        "an explain run on a clean machine plans four creations, got: {:?}",
        actions(&report)
    );
    assert!(
        !layout.root().exists(),
        "an explain run must not create {}",
        layout.root().display()
    );
}

/// Errors are instructions: a path kaibo cannot write is reported with the
/// command that fixes it, never a panic.
#[test]
fn a_file_where_the_manifest_directory_belongs_is_reported_not_panicked() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    let manifest_dir = layout
        .manifest_path()
        .parent()
        .expect("the manifest has a parent")
        .to_path_buf();
    fs::create_dir_all(layout.root()).expect("create the plugin root");
    fs::write(&manifest_dir, "not a directory\n").expect("occupy the manifest dir path");

    let report = install(&config, "0.1.0");

    assert_eq!(report.exit_code(), ExitCode::Usage);
    assert!(
        matches!(report.outcome, InstallOutcome::Failed { .. }),
        "expected a failure outcome, got {:?}",
        report.outcome
    );
    let (_, fix) = report
        .findings()
        .into_iter()
        .next()
        .expect("a failure carries a finding");
    assert_eq!(
        fix,
        Some(format!(
            "make {} writable, then re-run `kaibo install`",
            layout.root().display()
        ))
    );
}

// --- uninstall --------------------------------------------------------

#[test]
fn uninstall_removes_every_installed_file_and_the_directories_holding_them() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.1.0");

    let report = uninstall(&config);

    assert!(
        actions(&report)
            .iter()
            .all(|(_, action)| *action == FileAction::Removed),
        "every installed path is removed, got: {:?}",
        actions(&report)
    );
    assert!(
        !layout.root().exists(),
        "{} is pruned once the last kaibo file under it is gone",
        layout.root().display()
    );
    assert!(
        config
            .skills_dir()
            .expect("the sandbox config has a skills dir")
            .exists(),
        "the user's own skills directory is not kaibo's to remove"
    );
    assert!(retained(&report).is_empty());
}

#[test]
fn uninstall_on_a_machine_that_never_installed_reports_everything_absent() {
    let (_root, config) = sandbox();

    let report = uninstall(&config);

    assert_eq!(report.exit_code(), ExitCode::Success);
    assert!(
        actions(&report)
            .iter()
            .all(|(_, action)| *action == FileAction::Absent),
        "nothing was installed, so nothing is removed, got: {:?}",
        actions(&report)
    );
}

#[test]
fn uninstalling_twice_is_the_same_as_uninstalling_once() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.1.0");
    uninstall(&config);

    let second = uninstall(&config);

    assert_eq!(second.exit_code(), ExitCode::Success);
    assert!(
        actions(&second)
            .iter()
            .all(|(_, action)| *action == FileAction::Absent),
        "the second uninstall finds nothing left, got: {:?}",
        actions(&second)
    );
    assert!(!layout.root().exists());
}

/// Stop and report, never discard: a directory holding someone else's file
/// survives an uninstall, and the report says so.
#[test]
fn uninstall_leaves_a_directory_holding_a_file_kaibo_did_not_install() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.1.0");
    let stranger = layout
        .skill_path("query")
        .parent()
        .expect("the skill has a parent dir")
        .join("NOTES.md");
    fs::write(&stranger, "a human put this here\n").expect("write the stranger's file");

    let report = uninstall(&config);

    assert!(stranger.is_file(), "the stranger's file survives");
    assert!(
        retained(&report).contains(&stranger.parent().expect("has a parent").to_path_buf()),
        "the retained directory is reported, got: {:?}",
        retained(&report)
    );
    assert!(
        layout.root().exists(),
        "a root above a retained directory stays too"
    );
    assert!(
        !layout.manifest_path().exists(),
        "kaibo's own files are still removed"
    );
}

#[test]
fn an_explain_uninstall_reports_the_removals_and_removes_nothing() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.1.0");

    let report = InstallVerb::new(&config, "0.1.0", InstallMode::Uninstall).plan();

    assert!(
        actions(&report)
            .iter()
            .all(|(_, action)| *action == FileAction::Removed),
        "an explain uninstall plans a removal per installed file, got: {:?}",
        actions(&report)
    );
    assert!(retained(&report).is_empty());
    assert!(
        layout.manifest_path().is_file(),
        "an explain run removes nothing"
    );
}

#[test]
fn an_explain_uninstall_reports_a_directory_it_would_have_to_leave_behind() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.1.0");
    let stranger_dir = layout
        .skill_path("sync")
        .parent()
        .expect("the skill has a parent dir")
        .to_path_buf();
    fs::write(stranger_dir.join("NOTES.md"), "a human put this here\n")
        .expect("write the stranger's file");

    let report = InstallVerb::new(&config, "0.1.0", InstallMode::Uninstall).plan();

    assert_eq!(retained(&report), vec![stranger_dir]);
}

// --- what `status` reads ----------------------------------------------

#[test]
fn nothing_installed_reads_as_absent() {
    let (_root, config) = sandbox();

    assert_eq!(inspect(&config), InstalledSkills::Absent);
}

#[test]
fn a_freshly_installed_tree_reads_as_its_own_version_with_nothing_changed() {
    let (_root, config) = sandbox();
    install(&config, "0.4.2");

    assert_eq!(
        inspect(&config),
        InstalledSkills::Present {
            version: "0.4.2".to_string(),
            changed: Vec::new(),
            missing: Vec::new(),
        }
    );
}

#[test]
fn a_hand_edited_skill_file_reads_as_changed() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.4.2");
    let edited = format!(
        "{}\nA sentence a human added.\n",
        EMBEDDED_SKILLS
            .iter()
            .find(|skill| skill.name == "sync")
            .expect("sync is embedded")
            .source
    );
    fs::write(layout.skill_path("sync"), edited).expect("edit the installed skill");

    assert_eq!(
        inspect(&config),
        InstalledSkills::Present {
            version: "0.4.2".to_string(),
            changed: vec!["sync".to_string()],
            missing: Vec::new(),
        }
    );
}

/// A hand-edit is detected from content, not from a timestamp: a file
/// whose mtime moved without its bytes changing is not an edit, and a
/// mtime-based check would call it one.
#[test]
fn a_skill_file_rewritten_with_identical_bytes_reads_as_unchanged() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.4.2");
    let path = layout.skill_path("query");
    let before = fs::metadata(&path)
        .expect("stat the installed skill")
        .modified()
        .expect("mtime is available");
    std::thread::sleep(std::time::Duration::from_millis(20));
    let bytes = fs::read_to_string(&path).expect("read the installed skill");
    fs::write(&path, bytes).expect("rewrite the installed skill byte for byte");

    let after = fs::metadata(&path)
        .expect("stat the installed skill")
        .modified()
        .expect("mtime is available");
    assert_ne!(before, after, "the rewrite must actually move the mtime");
    assert_eq!(
        inspect(&config),
        InstalledSkills::Present {
            version: "0.4.2".to_string(),
            changed: Vec::new(),
            missing: Vec::new(),
        }
    );
}

#[test]
fn a_deleted_skill_file_reads_as_missing() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.4.2");
    fs::remove_file(layout.skill_path("contribute")).expect("delete the installed skill");

    assert_eq!(
        inspect(&config),
        InstalledSkills::Present {
            version: "0.4.2".to_string(),
            changed: Vec::new(),
            missing: vec!["contribute".to_string()],
        }
    );
}

#[test]
fn a_manifest_that_is_not_json_reads_as_unreadable() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.4.2");
    fs::write(layout.manifest_path(), "{not json\n").expect("corrupt the manifest");

    assert!(
        matches!(inspect(&config), InstalledSkills::ManifestUnreadable { .. }),
        "got {:?}",
        inspect(&config)
    );
}

#[test]
fn a_manifest_without_a_version_field_reads_as_unreadable() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");
    install(&config, "0.4.2");
    fs::write(layout.manifest_path(), "{\"name\": \"kaibo\"}\n").expect("strip the version");

    assert!(
        matches!(inspect(&config), InstalledSkills::ManifestUnreadable { .. }),
        "got {:?}",
        inspect(&config)
    );
}

// --- rendering --------------------------------------------------------

#[test]
fn the_json_report_carries_the_version_root_and_every_action() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");

    let json = install(&config, "0.1.0").render_json();

    assert_eq!(json["mode"], "install");
    assert_eq!(json["version"], "0.1.0");
    assert_eq!(json["applied"], true);
    assert_eq!(json["root"], layout.root().display().to_string());
    assert_eq!(json["exit_code"], 0);
    assert_eq!(json["outcome"]["state"], "ok");
    assert_eq!(
        json["outcome"]["changes"]
            .as_array()
            .expect("changes is an array")
            .len(),
        4
    );
    assert_eq!(json["outcome"]["changes"][0]["action"], "created");
}

#[test]
fn the_text_report_names_the_plugin_root_and_each_action() {
    let (_root, config) = sandbox();
    let layout = PluginLayout::new(&config).expect("the sandbox config has a skills dir");

    let text = install(&config, "0.1.0").render_text(&RenderOptions::default());

    assert!(text.starts_with("kaibo install\n"), "got: {text}");
    assert!(
        text.contains(&format!("plugin: kaibo at {}", layout.root().display())),
        "got: {text}"
    );
    assert!(text.contains("result: done"), "got: {text}");
    assert!(
        text.contains(&format!("created {}", layout.manifest_path().display())),
        "got: {text}"
    );
}

#[test]
fn the_uninstall_text_report_says_which_verb_ran() {
    let (_root, config) = sandbox();

    let text = uninstall(&config).render_text(&RenderOptions::default());

    assert!(
        text.starts_with("kaibo install --uninstall\n"),
        "got: {text}"
    );
}

#[test]
fn an_explain_report_says_it_only_planned() {
    let (_root, config) = sandbox();

    let text = InstallVerb::new(&config, "0.1.0", InstallMode::Install)
        .plan()
        .render_text(&RenderOptions::default());

    assert!(text.starts_with("kaibo install (explain)\n"), "got: {text}");
    assert!(text.contains("result: planned"), "got: {text}");
}

// --- nowhere to install to --------------------------------------------

/// Errors are instructions: with no home directory and no
/// `CLAUDE_CONFIG_DIR` there is no path to write to, and the report says
/// which variable to set rather than inventing a location.
#[test]
fn with_no_install_location_install_reports_the_variable_to_set() {
    let root = tempfile::tempdir().expect("create temp root");
    let config = ConfigBuilder::new(root.path().join("corpus"))
        .no_skills_dir()
        .build();

    let report = InstallVerb::new(&config, "0.1.0", InstallMode::Install).apply();

    assert_eq!(report.exit_code(), ExitCode::Usage);
    assert_eq!(report.root, None);
    let (_, fix) = report
        .findings()
        .into_iter()
        .next()
        .expect("a failure carries a finding");
    assert_eq!(
        fix,
        Some("export CLAUDE_CONFIG_DIR=/path/to/.claude, then re-run `kaibo install`".to_string())
    );
}

#[test]
fn with_no_install_location_there_is_nothing_to_inspect() {
    let root = tempfile::tempdir().expect("create temp root");
    let config = ConfigBuilder::new(root.path().join("corpus"))
        .no_skills_dir()
        .build();

    assert_eq!(inspect(&config), InstalledSkills::LocationUnknown);
}

#[test]
fn with_no_install_location_the_text_report_says_so() {
    let root = tempfile::tempdir().expect("create temp root");
    let config = ConfigBuilder::new(root.path().join("corpus"))
        .no_skills_dir()
        .build();

    let text = InstallVerb::new(&config, "0.1.0", InstallMode::Install)
        .apply()
        .render_text(&RenderOptions::default());

    assert!(
        text.contains("plugin: kaibo, install location unknown"),
        "got: {text}"
    );
    assert!(text.contains("result: failed"), "got: {text}");
}
