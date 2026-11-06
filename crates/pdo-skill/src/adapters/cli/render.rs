//! The text of `pdo skill`: the bank, a skill, a scan, an import, one fact a
//! line.

use serde_json::Value;

fn text(value: &Value) -> &str {
    value.as_str().unwrap_or("")
}

/// The name of the folder `id` names, from a `{"folders": […]}` answer; `-`
/// for the root.
fn folder_name<'a>(folders: &'a Value, id: &Value) -> &'a str {
    if id.is_null() {
        return "-";
    }
    folders["folders"]
        .as_array()
        .into_iter()
        .flatten()
        .find(|f| f["id"] == *id)
        .and_then(|f| f["name"].as_str())
        .unwrap_or("?")
}

/// The bank: one tab-separated line per skill (id, name, folder, `locked`
/// for a skill an extension provides).
pub(super) fn list_lines(bank: &Value) -> String {
    let mut out = String::new();
    for skill in bank["skills"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}\t{}\t{}{}\n",
            text(&skill["id"]),
            text(&skill["name"]),
            folder_name(bank, &skill["folder_id"]),
            if skill["locked"] == true {
                "\tlocked"
            } else {
                ""
            }
        ));
    }
    out
}

/// A skill: its facts, its files, then its `SKILL.md`.
pub(super) fn show_text(skill: &Value, folders: &Value) -> String {
    let mut out = format!(
        "{}\t{}\nfolder\t{}\n",
        text(&skill["id"]),
        text(&skill["name"]),
        folder_name(folders, &skill["folder_id"])
    );
    if skill["locked"] == true {
        out.push_str("locked\tprovided by an extension, read-only\n");
    }
    if let Some(description) = skill["description"].as_str().filter(|d| !d.is_empty()) {
        out.push_str(&format!("description\t{description}\n"));
    }
    if let Some(source) = skill["source"]["url"].as_str() {
        out.push_str(&format!("source\t{source}\n"));
    }
    for file in skill["files"].as_array().into_iter().flatten() {
        out.push_str(&format!("file\t{}\t{} bytes\n", text(&file["path"]), file["size"]));
    }
    if let Some(content) = skill["content"].as_str() {
        out.push('\n');
        out.push_str(content);
        if !content.ends_with('\n') {
            out.push('\n');
        }
    }
    out
}

/// A file of a skill: its text, or what it is when it is binary.
pub(super) fn file_text(file: &Value) -> String {
    match file["text"].as_str() {
        Some(text) if text.ends_with('\n') || text.is_empty() => text.to_string(),
        Some(text) => format!("{text}\n"),
        None => format!("binary file · {} bytes\n", file["size"]),
    }
}

/// A scan: one line per candidate (status, path, name), then the reason of
/// an invalid one.
pub(super) fn candidate_lines(report: &Value) -> String {
    let mut out = String::new();
    for c in report["candidates"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}\t{}\t{}",
            text(&c["status"]),
            text(&c["path"]),
            text(&c["name"])
        ));
        if let Some(existing) = c["existing"]["name"].as_str() {
            out.push_str(&format!("\t(bank: {existing})"));
        }
        if let Some(reason) = c["reason"].as_str() {
            out.push_str(&format!("\t{reason}"));
        }
        out.push('\n');
    }
    if out.is_empty() {
        out.push_str("no skill found\n");
    }
    out
}

/// What an import or an upload did: one line per skill, then the failures
/// and warnings.
pub(super) fn import_lines(report: &Value) -> String {
    let mut out = String::new();
    if let Some(folder) = report["folder"]["name"].as_str() {
        out.push_str(&format!("folder\t{folder}\n"));
    }
    for row in report["imported"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            text(&row["action"]),
            text(&row["skill"]["id"]),
            text(&row["skill"]["name"])
        ));
    }
    for (key, word) in [
        ("created", "created"),
        ("kept", "kept"),
        ("missing", "missing"),
    ] {
        for row in report[key].as_array().into_iter().flatten() {
            out.push_str(&format!(
                "{word}\t{}\t{}\n",
                text(&row["id"]),
                text(&row["name"])
            ));
        }
    }
    for row in report["renamed"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "renamed\t{}\t{} -> {}\n",
            text(&row["id"]),
            text(&row["from"]),
            text(&row["to"])
        ));
    }
    for row in report["failed"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "failed\t{}\t{}\n",
            text(&row["path"]),
            text(&row["error"])
        ));
    }
    for warning in report["warnings"].as_array().into_iter().flatten() {
        out.push_str(&format!("warning\t{}\n", text(warning)));
    }
    out
}

/// The Sources imported lately.
pub(super) fn source_lines(sources: &Value) -> String {
    let mut out = String::new();
    for s in sources["sources"].as_array().into_iter().flatten() {
        out.push_str(text(&s["url"]));
        if let Some(git_ref) = s["git_ref"].as_str().or_else(|| s["ref"].as_str()) {
            out.push_str(&format!("@{git_ref}"));
        }
        if let Some(path) = s["path"].as_str().filter(|p| !p.is_empty()) {
            out.push_str(&format!("\t{path}"));
        }
        out.push('\n');
    }
    out
}

/// The folders: one tab-separated line each (id, name, parent, Source).
pub(super) fn folder_lines(folders: &Value) -> String {
    let mut out = String::new();
    for f in folders["folders"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            text(&f["id"]),
            text(&f["name"]),
            folder_name(folders, &f["parent_id"]),
            f["source"]["url"].as_str().unwrap_or("-")
        ));
    }
    out
}

/// A rescan: one line per skill (status, path, name).
pub(super) fn entry_lines(rescan: &Value) -> String {
    let mut out = String::new();
    for e in rescan["entries"].as_array().into_iter().flatten() {
        out.push_str(&format!(
            "{}\t{}\t{}\n",
            text(&e["status"]),
            text(&e["path"]),
            text(&e["name"])
        ));
    }
    if out.is_empty() {
        out.push_str("nothing at the Source\n");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn the_bank_names_folders_and_locked_skills() {
        let bank = json!({
            "skills": [
                { "id": "s1", "name": "tdd", "folder_id": null, "locked": false },
                { "id": "s2", "name": "review", "folder_id": "f1", "locked": true },
            ],
            "folders": [{ "id": "f1", "name": "Craft" }],
        });
        assert_eq!(list_lines(&bank), "s1\ttdd\t-\ns2\treview\tCraft\tlocked\n");
    }

    #[test]
    fn a_skill_shows_its_facts_files_and_content() {
        let skill = json!({
            "id": "s2", "name": "review", "folder_id": "f1", "locked": true,
            "description": "Review a diff", "files": [{ "path": "checklist.md", "size": 12 }],
            "content": "---\nname: review\n---\nBody",
        });
        let folders = json!({ "folders": [{ "id": "f1", "name": "Craft" }] });
        assert_eq!(
            show_text(&skill, &folders),
            "s2\treview\nfolder\tCraft\nlocked\tprovided by an extension, read-only\n\
             description\tReview a diff\nfile\tchecklist.md\t12 bytes\n\n---\nname: review\n---\nBody\n"
        );
    }

    #[test]
    fn a_binary_file_is_named_not_printed() {
        assert_eq!(file_text(&json!({ "text": "a" })), "a\n");
        assert_eq!(
            file_text(&json!({ "text": null, "binary": true, "size": 3 })),
            "binary file · 3 bytes\n"
        );
    }

    #[test]
    fn a_scan_lists_its_candidates() {
        let report = json!({ "candidates": [
            { "path": "skills/tdd", "name": "tdd", "status": "name_taken", "existing": { "name": "tdd" } },
            { "path": "x", "name": "x", "status": "invalid", "reason": "no SKILL.md" },
        ]});
        assert_eq!(
            candidate_lines(&report),
            "name_taken\tskills/tdd\ttdd\t(bank: tdd)\ninvalid\tx\tx\tno SKILL.md\n"
        );
    }

    #[test]
    fn an_import_and_an_upload_say_what_they_did() {
        let import = json!({
            "folder": { "name": "repo" },
            "imported": [{ "path": "a", "action": "import", "skill": { "id": "s1", "name": "a" } }],
            "failed": [{ "path": "b", "error": "duplicate", "code": "x" }],
        });
        assert_eq!(
            import_lines(&import),
            "folder\trepo\nimport\ts1\ta\nfailed\tb\tduplicate\n"
        );
        let upload = json!({
            "created": [{ "id": "s1", "name": "a" }], "kept": [], "missing": [],
            "renamed": [{ "id": "s1", "from": "a", "to": "a (2)" }], "warnings": ["w"],
        });
        assert_eq!(
            import_lines(&upload),
            "created\ts1\ta\nrenamed\ts1\ta -> a (2)\nwarning\tw\n"
        );
    }
}
