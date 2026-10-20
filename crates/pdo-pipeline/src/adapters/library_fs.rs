//! The node library on disk: one `<slug>.yaml` per entry under
//! `~/.pdo/library`.

use std::path::{Path, PathBuf};

use crate::application::ports::NodeLibrary;
use crate::domain::library::{parse_entry, slugify, LibraryEntry};

fn resolve_slug(dir: &Path, name: &str) -> String {
    let base = slugify(name);
    if !dir.join(format!("{base}.yaml")).exists() {
        return base;
    }
    let mut suffix = 2u32;
    loop {
        let candidate = format!("{base}-{suffix}");
        if !dir.join(format!("{candidate}.yaml")).exists() {
            return candidate;
        }
        suffix += 1;
    }
}

fn list_in(dir: Option<PathBuf>) -> Vec<LibraryEntry> {
    let Some(dir) = dir else {
        return Vec::new();
    };
    let Ok(read_dir) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut entries: Vec<LibraryEntry> = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Some(entry) = parse_entry(&contents) {
                entries.push(entry);
            }
        }
    }
    entries.sort_by_key(|a| a.name.to_lowercase());
    entries
}

fn get_in(dir: Option<PathBuf>, name: &str) -> Option<LibraryEntry> {
    let dir = dir?;
    let slug = slugify(name);
    let path = dir.join(format!("{slug}.yaml"));
    // Don't return early on a slug-file miss: a collision parks the entry under a
    // `-N` suffix, and an unparseable slug file (0-byte remnant of an interrupted
    // write) must not shadow it — `list` would surface a name `get` 404s on.
    if let Ok(contents) = std::fs::read_to_string(&path) {
        if let Some(entry) = parse_entry(&contents) {
            if entry.name == name {
                return Some(entry);
            }
        }
    }
    find_by_name(&dir, name)
}

fn find_by_name(dir: &Path, name: &str) -> Option<LibraryEntry> {
    let read_dir = std::fs::read_dir(dir).ok()?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Some(lib_entry) = parse_entry(&contents) {
                if lib_entry.name == name {
                    return Some(lib_entry);
                }
            }
        }
    }
    None
}

fn save_in(dir: Option<PathBuf>, entry: &LibraryEntry) -> Result<(), String> {
    let dir = dir.ok_or("HOME not set")?;
    std::fs::create_dir_all(&dir).map_err(|e| format!("failed to create library dir: {e}"))?;

    let existing_path = find_path_by_name(&dir, &entry.name);
    let path = existing_path.unwrap_or_else(|| {
        let slug = resolve_slug(&dir, &entry.name);
        dir.join(format!("{slug}.yaml"))
    });

    // #655: the file on disk states where the entry works, including at the
    // type's default — the same discipline `normalize_node_value` applies to a
    // pipeline Document, so a starred node and its library file read alike.
    let mut entry = entry.clone();
    entry.stamp_isolation();
    let yaml = serde_yaml::to_string(&entry).map_err(|e| format!("serialization error: {e}"))?;
    std::fs::write(&path, yaml).map_err(|e| format!("write error: {e}"))?;
    Ok(())
}

fn delete_in(dir: Option<PathBuf>, name: &str) -> Result<bool, String> {
    let dir = dir.ok_or("HOME not set")?;
    if let Some(path) = find_path_by_name(&dir, name) {
        std::fs::remove_file(&path).map_err(|e| format!("delete error: {e}"))?;
        Ok(true)
    } else {
        Ok(false)
    }
}

fn find_path_by_name(dir: &Path, name: &str) -> Option<PathBuf> {
    let slug = slugify(name);
    let direct = dir.join(format!("{slug}.yaml"));
    if direct.exists() {
        if let Ok(contents) = std::fs::read_to_string(&direct) {
            if let Some(entry) = parse_entry(&contents) {
                if entry.name == name {
                    return Some(direct);
                }
            }
        }
    }
    let read_dir = std::fs::read_dir(dir).ok()?;
    for entry in read_dir.flatten() {
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) != Some("yaml") {
            continue;
        }
        if let Ok(contents) = std::fs::read_to_string(&path) {
            if let Some(lib_entry) = parse_entry(&contents) {
                if lib_entry.name == name {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Where the library lives.
enum Root {
    /// `~/.pdo/library`, `$HOME` read at every call.
    Home,
    /// A fixed directory.
    At(PathBuf),
}

/// The node library of the daemon's user.
pub struct FsNodeLibrary {
    root: Root,
}

impl FsNodeLibrary {
    /// `~/.pdo/library`.
    pub fn home() -> Self {
        Self { root: Root::Home }
    }

    /// The library kept in `dir`.
    pub fn at(dir: impl Into<PathBuf>) -> Self {
        Self {
            root: Root::At(dir.into()),
        }
    }

    fn dir(&self) -> Option<PathBuf> {
        match &self.root {
            Root::Home => {
                std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".pdo").join("library"))
            }
            Root::At(dir) => Some(dir.clone()),
        }
    }
}

impl NodeLibrary for FsNodeLibrary {
    fn list(&self) -> Vec<LibraryEntry> {
        list_in(self.dir())
    }

    fn get(&self, name: &str) -> Option<LibraryEntry> {
        get_in(self.dir(), name)
    }

    fn save(&self, entry: &LibraryEntry) -> Result<(), String> {
        save_in(self.dir(), entry)
    }

    fn delete(&self, name: &str) -> Result<bool, String> {
        delete_in(self.dir(), name)
    }
}

#[cfg(test)]
mod tests {
    //! Stage 3 (ADR-0080): the library against a real directory. The free
    //! functions below point every call of a test at its own directory.
    use std::cell::RefCell;
    use std::collections::HashMap;

    use super::*;
    use crate::domain::library::{entry_from_node, SyncState};

    thread_local! {
        static DIR: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
    }

    fn with_temp_home<F: FnOnce()>(f: F) {
        let tmp = tempfile::tempdir().unwrap();
        DIR.with(|d| *d.borrow_mut() = Some(tmp.path().join(".pdo").join("library")));
        f();
        DIR.with(|d| *d.borrow_mut() = None);
    }

    fn library() -> FsNodeLibrary {
        FsNodeLibrary::at(library_dir().unwrap())
    }

    fn library_dir() -> Option<PathBuf> {
        DIR.with(|d| d.borrow().clone())
    }

    fn save(entry: &LibraryEntry) -> Result<(), String> {
        library().save(entry)
    }

    fn get(name: &str) -> Option<LibraryEntry> {
        library().get(name)
    }

    fn list() -> Vec<LibraryEntry> {
        library().list()
    }

    fn delete(name: &str) -> Result<bool, String> {
        library().delete(name)
    }

    fn sync_state(node: &crate::domain::pipeline::NodeDef, prompt: &str) -> SyncState {
        crate::domain::library::sync_state(get(&node.name).as_ref(), node, prompt)
    }

    fn set_claude_entry(
        node: &mut crate::domain::pipeline::NodeDef,
        model: Option<&str>,
        effort: Option<&str>,
    ) {
        node.harnesses.insert(
            pdo_harness::registry::CLAUDE.to_string(),
            pdo_harness::resolver::HarnessEntry {
                model: model.map(str::to_string),
                effort: effort.map(str::to_string),
            },
        );
    }

    fn make_node(name: &str) -> crate::domain::pipeline::NodeDef {
        crate::domain::pipeline::NodeDef {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            skills: Vec::new(),
            isolated_worktree: None,
            id: "test-id".to_string(),
            name: name.to_string(),
            node_type: crate::domain::pipeline::NodeType::Agent,
            inputs: vec![crate::domain::pipeline::Port {
                name: "in".to_string(),
                port_type: crate::domain::pipeline::PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            outputs: vec![crate::domain::pipeline::Port {
                name: "out".to_string(),
                port_type: crate::domain::pipeline::PortType::Markdown,
                frontmatter: None,
                when: None,
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: false,
            view: None,
            max_iter: None,
            over: None,
            pin_harness: None,
            harnesses: Default::default(),
            agent_choice: None,
            auto_fail: None,
            orchestrator: false,
            on_demand: false,
        }
    }

    #[test]
    fn slugify_basic() {
        assert_eq!(slugify("My Node"), "my-node");
        assert_eq!(slugify("A_B-C"), "a_b-c");
        assert_eq!(slugify("!!!"), "node");
        assert_eq!(slugify("Review Bot 2"), "review-bot-2");
    }

    #[test]
    fn crud_round_trip() {
        with_temp_home(|| {
            let node = make_node("Reviewer");
            let entry = entry_from_node(&node, "You are a reviewer.");
            save(&entry).unwrap();

            let got = get("Reviewer").unwrap();
            assert_eq!(got, entry);

            let all = list();
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].name, "Reviewer");

            let deleted = delete("Reviewer").unwrap();
            assert!(deleted);

            assert!(get("Reviewer").is_none());
            assert!(list().is_empty());
        });
    }

    #[test]
    fn delete_nonexistent_returns_false() {
        with_temp_home(|| {
            let result = delete("ghost").unwrap();
            assert!(!result);
        });
    }

    #[test]
    fn slug_collision_suffix() {
        with_temp_home(|| {
            let dir = library_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();

            let entry1 = LibraryEntry {
                free_live_artifacts: false,
                live_artifacts: Vec::new(),
                on_demand: false,
                name: "Alpha".to_string(),
                node_type: crate::domain::pipeline::NodeType::Agent,
                isolated_worktree: Some(true),
                inputs: vec![],
                outputs: vec![],
                interactive: false,
                model: None,
                effort: None,
                max_iter: None,
                branches: None,
                prompt: "first".to_string(),
            };
            save(&entry1).unwrap();
            assert!(dir.join("alpha.yaml").exists());

            // Write a different file at the slug path to force collision
            std::fs::write(
                dir.join("beta.yaml"),
                serde_yaml::to_string(&LibraryEntry {
                    on_demand: false,
                    name: "Beta Original".to_string(),
                    ..entry1.clone()
                })
                .unwrap(),
            )
            .unwrap();

            let entry2 = LibraryEntry {
                on_demand: false,
                name: "Beta".to_string(),
                ..entry1
            };
            save(&entry2).unwrap();
            // beta.yaml is taken by "Beta Original", so "Beta" goes to beta-2.yaml
            assert!(dir.join("beta-2.yaml").exists());
        });
    }

    #[test]
    fn get_scans_past_unparseable_slug_file() {
        // An unparseable file squatting the direct slug path must not shadow an
        // entry parked under a collision `-N` slug: `get` has to agree with `list`.
        with_temp_home(|| {
            let dir = library_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();

            let entry = LibraryEntry {
                free_live_artifacts: false,
                live_artifacts: Vec::new(),
                on_demand: false,
                name: "Typed Reviewer".to_string(),
                node_type: crate::domain::pipeline::NodeType::Agent,
                isolated_worktree: Some(true),
                inputs: vec![],
                outputs: vec![],
                interactive: false,
                model: None,
                effort: None,
                max_iter: None,
                branches: None,
                prompt: "hi".to_string(),
            };
            std::fs::write(
                dir.join("typed-reviewer-2.yaml"),
                serde_yaml::to_string(&entry).unwrap(),
            )
            .unwrap();
            std::fs::write(dir.join("typed-reviewer.yaml"), "").unwrap();

            let got = get("Typed Reviewer").expect("get must scan past the poison file");
            assert_eq!(got.name, "Typed Reviewer");
            assert_eq!(got.prompt, "hi");
        });
    }

    #[test]
    fn sync_state_outline_when_missing() {
        with_temp_home(|| {
            let node = make_node("Ghost");
            assert_eq!(sync_state(&node, "prompt"), SyncState::Outline);
        });
    }

    #[test]
    fn sync_state_synced_when_equal() {
        with_temp_home(|| {
            let node = make_node("Reviewer");
            let entry = entry_from_node(&node, "You review code.");
            save(&entry).unwrap();

            assert_eq!(sync_state(&node, "You review code."), SyncState::Synced);
        });
    }

    #[test]
    fn sync_state_diverged_when_different() {
        with_temp_home(|| {
            let node = make_node("Reviewer");
            let entry = entry_from_node(&node, "Original prompt.");
            save(&entry).unwrap();

            assert_eq!(sync_state(&node, "Changed prompt."), SyncState::Diverged);
        });
    }

    #[test]
    fn sync_state_diverged_when_only_the_live_artifacts_differ() {
        // ADR-0089: semantic — a node that gained a live artifact or the free
        // publication is not the starred node any more, and the entry carries them.
        with_temp_home(|| {
            let node = make_node("Reviewer");
            save(&entry_from_node(&node, "p")).unwrap();

            let mut live = node.clone();
            live.live_artifacts
                .push(crate::domain::pipeline::LiveArtifact {
                    name: "progress".into(),
                    port_type: crate::domain::pipeline::PortType::Markdown,
                    instructions: None,
                    frontmatter: None,
                });
            assert_eq!(sync_state(&live, "p"), SyncState::Diverged);
            let mut free = node.clone();
            free.free_live_artifacts = true;
            assert_eq!(sync_state(&free, "p"), SyncState::Diverged);

            live.free_live_artifacts = true;
            save(&entry_from_node(&live, "p")).unwrap();
            assert_eq!(sync_state(&live, "p"), SyncState::Synced);
            let entry = get("Reviewer").unwrap();
            assert_eq!(entry.live_artifacts[0].name, "progress");
            assert!(entry.free_live_artifacts);
        });
    }

    #[test]
    fn an_entry_starred_with_an_intermediate_output_reads_it_as_a_live_artifact() {
        let entry = crate::domain::library::parse_entry(
            "name: Writer\ntype: agent\noutputs:\n  - name: adr\n  - name: draft\n    intermediate: true\n",
        )
        .unwrap();
        assert_eq!(entry.outputs.len(), 1);
        assert_eq!(entry.live_artifacts[0].name, "draft");
    }

    #[test]
    fn yaml_round_trip_lossless() {
        let entry = LibraryEntry {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            on_demand: false,
            name: "Complex Node".to_string(),
            node_type: crate::domain::pipeline::NodeType::Agent,
            isolated_worktree: Some(true),
            // #345/#296: a per-node model must round-trip losslessly.
            model: Some("opus".to_string()),
            effort: Some("low".to_string()),
            inputs: vec![
                crate::domain::pipeline::Port {
                    name: "plan".to_string(),
                    port_type: crate::domain::pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                },
                crate::domain::pipeline::Port {
                    name: "reviews".to_string(),
                    port_type: crate::domain::pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                },
            ],
            outputs: vec![crate::domain::pipeline::Port {
                name: "summary".to_string(),
                when: None,
                port_type: crate::domain::pipeline::PortType::Markdown,
                frontmatter: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "verdict".to_string(),
                        crate::domain::pipeline::FrontmatterFieldDecl {
                            field_type: "enum".to_string(),
                            allowed: Some(vec!["PASS".to_string(), "FAIL".to_string()]),
                        },
                    );
                    m
                }),
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: true,
            max_iter: Some(5),
            branches: Some(3),
            prompt: "You are an implementer.\nMulti-line prompt.".to_string(),
        };

        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(yaml.contains("model: opus"), "yaml missing model:\n{yaml}");
        let parsed: LibraryEntry = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, entry);
    }

    #[test]
    fn yaml_round_trip_model_absent_is_omitted() {
        // A node on the account default must not emit a `model:` key: an entry
        // that does reads `diverged` against a library file that never had one.
        let entry = LibraryEntry {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            on_demand: false,
            name: "Plain".to_string(),
            node_type: crate::domain::pipeline::NodeType::Agent,
            isolated_worktree: Some(true),
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            model: None,
            effort: None,
            max_iter: None,
            branches: None,
            prompt: "hi".to_string(),
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(!yaml.contains("model:"), "model: leaked into yaml:\n{yaml}");
        let parsed: LibraryEntry = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, entry);
        assert_eq!(parsed.model, None);
    }

    #[test]
    fn entry_from_node_carries_model() {
        let mut node = make_node("Modelled");
        set_claude_entry(&mut node, Some("sonnet"), None);
        let entry = entry_from_node(&node, "prompt");
        assert_eq!(entry.model, Some("sonnet".to_string()));
    }

    #[test]
    fn yaml_round_trip_effort_absent_is_omitted() {
        // Same invariant as `model`: an unset node must stay byte-identical to a
        // library entry written before the key existed.
        let entry = LibraryEntry {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            on_demand: false,
            name: "Plain".to_string(),
            node_type: crate::domain::pipeline::NodeType::Agent,
            isolated_worktree: Some(true),
            inputs: vec![],
            outputs: vec![],
            interactive: false,
            model: None,
            effort: None,
            max_iter: None,
            branches: None,
            prompt: "hi".to_string(),
        };
        let yaml = serde_yaml::to_string(&entry).unwrap();
        assert!(
            !yaml.contains("effort:"),
            "effort: leaked into yaml:\n{yaml}"
        );
        let parsed: LibraryEntry = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(parsed, entry);
        assert_eq!(parsed.effort, None);
    }

    #[test]
    fn entry_from_node_carries_effort() {
        // The two axes must not interfere.
        let mut node = make_node("Effortful");
        set_claude_entry(&mut node, Some("sonnet"), Some("xhigh"));
        let entry = entry_from_node(&node, "prompt");
        assert_eq!(entry.effort, Some("xhigh".to_string()));
        assert_eq!(entry.model, Some("sonnet".to_string()));
    }

    #[test]
    fn sync_state_diverges_on_effort_change() {
        // The starred-node verdict must see the effort: a library entry blind to
        // the field reads `synced` while the two actually differ.
        with_temp_home(|| {
            let mut node = make_node("Effortful");
            set_claude_entry(&mut node, None, Some("low"));
            save(&entry_from_node(&node, "prompt")).unwrap();
            assert_eq!(sync_state(&node, "prompt"), SyncState::Synced);

            set_claude_entry(&mut node, None, Some("high"));
            assert_eq!(sync_state(&node, "prompt"), SyncState::Diverged);

            set_claude_entry(&mut node, None, None);
            assert_eq!(sync_state(&node, "prompt"), SyncState::Diverged);
        });
    }

    /// ADR-0086: « on demand » is part of what gets starred — it survives the
    /// disk, and toggling it makes the node diverge from its entry.
    #[test]
    fn on_demand_round_trips_and_diverges() {
        with_temp_home(|| {
            let mut node = make_node("Reviewer");
            node.on_demand = true;
            save(&entry_from_node(&node, "prompt")).unwrap();
            assert!(get("Reviewer").unwrap().on_demand);
            assert_eq!(sync_state(&node, "prompt"), SyncState::Synced);

            node.on_demand = false;
            assert_eq!(sync_state(&node, "prompt"), SyncState::Diverged);
        });
    }

    /// #655/ADR-0060: an entry states where it works, and the statement survives
    /// the disk. Not "carries the field": a saved Agent parked in the Run's
    /// worktree must come back parked there, or every instantiation would fork.
    #[test]
    fn isolation_round_trips_through_disk() {
        with_temp_home(|| {
            for (node_type, isolated) in [
                (crate::domain::pipeline::NodeType::Agent, false),
                (crate::domain::pipeline::NodeType::Agent, true),
                (crate::domain::pipeline::NodeType::Script, true),
                (crate::domain::pipeline::NodeType::Script, false),
            ] {
                let mut node = make_node("Traveller");
                node.node_type = node_type.clone();
                node.isolated_worktree = Some(isolated);
                save(&entry_from_node(&node, "p")).unwrap();
                assert_eq!(
                    get("Traveller").unwrap().isolated_worktree,
                    Some(isolated),
                    "{node_type:?} isolated={isolated}"
                );
            }
        });
    }

    /// The entry states its choice even when the node was silent about it — the
    /// #345 discipline applied to isolation. A silence on disk would make the very
    /// node that produced the entry read `diverged` against it.
    #[test]
    fn entry_from_node_stamps_the_type_default() {
        let mut agent = make_node("Silent Agent");
        assert_eq!(agent.isolated_worktree, None);
        assert_eq!(
            entry_from_node(&agent, "p").isolated_worktree,
            Some(true),
            "a silent agent stars as isolated"
        );

        agent.node_type = crate::domain::pipeline::NodeType::Script;
        assert_eq!(
            entry_from_node(&agent, "p").isolated_worktree,
            Some(false),
            "a silent script stars on the Run's worktree"
        );

        // A type that carries no isolation drops a stray value rather than
        // round-tripping a line nobody may edit (mirrors `stamp_isolation_default`).
        agent.node_type = crate::domain::pipeline::NodeType::Switch;
        agent.isolated_worktree = Some(false);
        assert_eq!(entry_from_node(&agent, "p").isolated_worktree, None);
    }

    /// A starred Start/End/Switch writes no isolation line at all.
    #[test]
    fn isolation_key_is_absent_for_types_that_carry_none() {
        let mut node = make_node("Router");
        node.node_type = crate::domain::pipeline::NodeType::Switch;
        let yaml = serde_yaml::to_string(&entry_from_node(&node, "p")).unwrap();
        assert!(
            !yaml.contains("isolated_worktree"),
            "isolation leaked onto a Switch entry:\n{yaml}"
        );
    }

    /// #655: two nodes that differ only by where they work are NOT the same
    /// content — the star must say so. Same trap as #345/#424, one field later.
    #[test]
    fn sync_state_diverges_on_isolation_change() {
        with_temp_home(|| {
            let mut node = make_node("Worker");
            node.isolated_worktree = Some(false);
            save(&entry_from_node(&node, "prompt")).unwrap();
            assert_eq!(sync_state(&node, "prompt"), SyncState::Synced);

            node.isolated_worktree = Some(true);
            assert_eq!(sync_state(&node, "prompt"), SyncState::Diverged);

            // Going silent is going back to the Agent default (isolated), which
            // still differs from the entry's explicit `false`.
            node.isolated_worktree = None;
            assert_eq!(sync_state(&node, "prompt"), SyncState::Diverged);
        });
    }

    /// A pre-#655 entry file carries no isolation line. Reading it as a silence
    /// would flip every already-starred node to `diverged` on upgrade, so the
    /// read stamps the type's default instead — the same migration-free posture
    /// `compute_drift` takes for pre-#395 hashes.
    #[test]
    fn pre_655_entry_reads_as_its_type_default() {
        with_temp_home(|| {
            let mut node = make_node("Legacy");
            node.isolated_worktree = None;

            // The pre-#655 file: today's entry with the isolation line removed.
            let today = serde_yaml::to_string(&entry_from_node(&node, "p")).unwrap();
            let legacy: String = today
                .lines()
                .filter(|l| !l.starts_with("isolated_worktree"))
                .map(|l| format!("{l}\n"))
                .collect();
            assert_ne!(legacy, today, "the fixture must actually drop the line");

            let dir = library_dir().unwrap();
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("legacy.yaml"), legacy).unwrap();

            assert_eq!(get("Legacy").unwrap().isolated_worktree, Some(true));
            assert_eq!(list()[0].isolated_worktree, Some(true));
            assert_eq!(sync_state(&node, "p"), SyncState::Synced);
        });
    }

    #[test]
    fn list_is_sorted_alphabetically() {
        with_temp_home(|| {
            let base = LibraryEntry {
                free_live_artifacts: false,
                live_artifacts: Vec::new(),
                on_demand: false,
                name: String::new(),
                node_type: crate::domain::pipeline::NodeType::Agent,
                isolated_worktree: Some(true),
                inputs: vec![],
                outputs: vec![],
                interactive: false,
                model: None,
                effort: None,
                max_iter: None,
                branches: None,
                prompt: "p".to_string(),
            };

            for name in ["Zebra", "Alpha", "middle"] {
                let mut e = base.clone();
                e.name = name.to_string();
                save(&e).unwrap();
            }

            let names: Vec<String> = list().into_iter().map(|e| e.name).collect();
            assert_eq!(names, vec!["Alpha", "middle", "Zebra"]);
        });
    }

    #[test]
    fn save_overwrites_existing_entry() {
        with_temp_home(|| {
            let node = make_node("Worker");
            let entry1 = entry_from_node(&node, "version 1");
            save(&entry1).unwrap();
            assert_eq!(get("Worker").unwrap().prompt, "version 1");

            let entry2 = entry_from_node(&node, "version 2");
            save(&entry2).unwrap();
            assert_eq!(get("Worker").unwrap().prompt, "version 2");
            assert_eq!(list().len(), 1);
        });
    }

    #[test]
    fn json_round_trip_preserves_all_port_fields() {
        let entry = LibraryEntry {
            free_live_artifacts: false,
            live_artifacts: Vec::new(),
            on_demand: false,
            name: "Typed Node".to_string(),
            node_type: crate::domain::pipeline::NodeType::Agent,
            isolated_worktree: Some(true),
            model: None,
            effort: None,
            inputs: vec![
                crate::domain::pipeline::Port {
                    name: "task".to_string(),
                    port_type: crate::domain::pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                },
                crate::domain::pipeline::Port {
                    name: "reviews".to_string(),
                    port_type: crate::domain::pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                },
            ],
            outputs: vec![crate::domain::pipeline::Port {
                name: "review".to_string(),
                port_type: crate::domain::pipeline::PortType::Markdown,
                frontmatter: Some({
                    let mut m = HashMap::new();
                    m.insert(
                        "verdict".to_string(),
                        crate::domain::pipeline::FrontmatterFieldDecl {
                            field_type: "enum".to_string(),
                            allowed: Some(vec!["PASS".to_string(), "FAIL".to_string()]),
                        },
                    );
                    m.insert(
                        "score".to_string(),
                        crate::domain::pipeline::FrontmatterFieldDecl {
                            field_type: "int".to_string(),
                            allowed: None,
                        },
                    );
                    m
                }),
                when: Some(serde_yaml::Value::Mapping({
                    let mut m = serde_yaml::Mapping::new();
                    m.insert(
                        serde_yaml::Value::String("verdict".to_string()),
                        serde_yaml::Value::Mapping({
                            let mut inner = serde_yaml::Mapping::new();
                            inner.insert(
                                serde_yaml::Value::String("eq".to_string()),
                                serde_yaml::Value::String("PASS".to_string()),
                            );
                            inner
                        }),
                    );
                    m
                })),
                description: None,
                instructions: None,
                required: false,
            }],
            interactive: true,
            max_iter: Some(5),
            branches: None,
            prompt: "You are a reviewer.".to_string(),
        };

        let json = serde_json::to_string(&entry).unwrap();
        let parsed: LibraryEntry = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, entry);

        let output = &parsed.outputs[0];
        assert!(output.frontmatter.is_some());
        let fm = output.frontmatter.as_ref().unwrap();
        assert_eq!(fm["verdict"].field_type, "enum");
        assert_eq!(
            fm["verdict"].allowed,
            Some(vec!["PASS".to_string(), "FAIL".to_string()])
        );
        assert_eq!(fm["score"].field_type, "int");

        assert!(output.when.is_some());
    }

    #[test]
    fn json_round_trip_via_disk_preserves_port_fields() {
        with_temp_home(|| {
            let entry = LibraryEntry {
                free_live_artifacts: false,
                live_artifacts: Vec::new(),
                on_demand: false,
                name: "Schema Node".to_string(),
                node_type: crate::domain::pipeline::NodeType::Agent,
                isolated_worktree: Some(true),
                model: None,
                effort: None,
                inputs: vec![crate::domain::pipeline::Port {
                    name: "in".to_string(),
                    port_type: crate::domain::pipeline::PortType::Markdown,
                    frontmatter: None,
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                outputs: vec![crate::domain::pipeline::Port {
                    name: "result".to_string(),
                    port_type: crate::domain::pipeline::PortType::Markdown,
                    frontmatter: Some({
                        let mut m = HashMap::new();
                        m.insert(
                            "status".to_string(),
                            crate::domain::pipeline::FrontmatterFieldDecl {
                                field_type: "enum".to_string(),
                                allowed: Some(vec!["OK".to_string(), "ERROR".to_string()]),
                            },
                        );
                        m
                    }),
                    when: None,
                    description: None,
                    instructions: None,
                    required: false,
                }],
                interactive: false,
                max_iter: None,
                branches: None,
                prompt: "Implement changes.".to_string(),
            };

            save(&entry).unwrap();

            let loaded = get("Schema Node").unwrap();
            assert_eq!(loaded, entry);

            let fm = loaded.outputs[0].frontmatter.as_ref().unwrap();
            assert_eq!(fm["status"].field_type, "enum");
            assert_eq!(
                fm["status"].allowed,
                Some(vec!["OK".to_string(), "ERROR".to_string()])
            );
        });
    }
}
