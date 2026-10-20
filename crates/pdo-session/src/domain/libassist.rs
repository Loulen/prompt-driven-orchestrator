//! The library pipeline authoring assistant (#302 / ADR-0048, one session per
//! Actor since CORE-61): where it works, what it is told, and the **focus** —
//! which template the UI of its Actor has open, and when it last said so.

use std::path::{Path, PathBuf};

/// The `PDO_RUN_ID` / `PDO_NODE_ID` the assistant's session is env-wrapped with.
///
/// A single constant for both since #594: there is no pipeline id to pose as a
/// pseudo-run — and there never was a Run behind it. Both values are inert (the assistant never
/// self-signals via `pdo complete`); the load-bearing export is `PDO_DAEMON_URL`.
pub const LIBASSIST_ENV_ID: &str = "__libassist__";

/// The library assistant's focus: the instance pipeline the UI is editing now.
///
/// Never persisted: it is a **presence** signal, not a fact about the project, so
/// a persisted value would resurrect on the next boot a claim that someone was
/// editing. A restart forgets it, the UI re-declares within 20 s, and in between
/// the assistant is a session nobody is attached to — precisely what the sweep
/// should reap.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibassistFocus {
    pub pipeline_id: String,
    /// Absolute path of the template's YAML, or `None` when the id resolves to no
    /// file yet — an unsaved template is a legitimate focus, not an error.
    pub path: Option<PathBuf>,
    /// When the UI last declared this focus. Same clock as the sweep's `now`, so
    /// the age comparison needs no conversion.
    pub at: chrono::DateTime<chrono::Utc>,
}

/// The assistant's cwd: the repo's template directory (ADR-0051 §1).
///
/// NOT the library store: an edit tab of scope `repo`/`user` edits a file under
/// `.pdo/pipelines/`, while `library_store::pipelines::scope_dir` answers
/// `.pdo/library/pipelines/`. Confusing the two words launched the assistant in a
/// directory where the file its primer named did not exist. The file actually being
/// edited arrives by absolute path through the focus.
pub fn libassist_cwd(repo_root: &Path) -> PathBuf {
    repo_root.join(".pdo").join("pipelines")
}

/// Where the assistant's primer and hook settings live: a sibling `.libassist/`
/// of the cwd, so the user-facing templates directory stays clean.
pub fn libassist_state_dir(repo_root: &Path) -> PathBuf {
    repo_root.join(".pdo").join(".libassist")
}

/// The primer of the assistant session `session`, in [`libassist_state_dir`]:
/// `assistant.md` for the local operator's, `<session>.md` for any other
/// Actor's, so two Actors never read each other's.
pub fn libassist_primer_path(repo_root: &Path, session: &str) -> PathBuf {
    let file = if session == crate::domain::names::LIBASSIST_SESSION_NAME {
        "assistant.md".to_string()
    } else {
        format!("{session}.md")
    };
    libassist_state_dir(repo_root).join(file)
}

pub fn focus_json(
    focus: Option<&LibassistFocus>,
    now: chrono::DateTime<chrono::Utc>,
) -> serde_json::Value {
    match focus {
        None => serde_json::json!({
            "pipeline_id": null,
            "scope": null,
            "path": null,
            "age_secs": null,
        }),
        Some(f) => serde_json::json!({
            "pipeline_id": f.pipeline_id,
            "path": f.path.as_ref().map(|p| p.to_string_lossy()),
            "age_secs": now.signed_duration_since(f.at).num_seconds().max(0),
        }),
    }
}

/// The one-line rendering injected into the assistant's context on every message.
///
/// Names the save endpoint, and names it as the *only* one. Echoing the focus scope
/// back to `POST /library/pipelines` reads that word in the library store's
/// vocabulary, so a `repo` template "saved" that way lands in
/// `.pdo/library/pipelines/` as a duplicate while the edited file never moves — and
/// the assistant announces success.
pub fn focus_text(focus: Option<&LibassistFocus>) -> String {
    match focus {
        None => "PDO — aucune pipeline ouverte dans l'UI pour l'instant. Ne devine pas de \
                 template : demande à l'utilisateur laquelle il veut travailler.\n"
            .to_string(),
        Some(f) => {
            let path = match &f.path {
                Some(p) => p.to_string_lossy().to_string(),
                None => {
                    "(pas encore de fichier sur le disque — template non sauvée : ni brouillon à \
                         lire ni save possible avant un premier Save de l'utilisateur)"
                        .to_string()
                }
            };
            format!(
                "PDO — pipeline actuellement ouverte dans l'UI : `{}`, fichier : {}\n\
                 Travaille sur celle-là, sans la deviner ni la redemander. Lis son brouillon \
                 partagé par GET /sessions/libassist/draft : le fichier ne porte que la dernière \
                 version. Pour écrire, un seul endpoint : POST /sessions/libassist/save avec \
                 `{{\"yaml\": …, \"prompts\": …}}` — il écrit dans le brouillon de CETTE \
                 pipeline, en direct chez tous, sans id ni scope à fournir ; le fichier ne change \
                 qu'au Save d'un humain.\n",
                f.pipeline_id, path
            )
        }
    }
}

/// The `curl` arguments that send the session's token, when it has one: the
/// one the `UserPromptSubmit` hook sends too.
pub const AUTH_HEADER: &str = r#"${PDO_TOKEN:+-H "Authorization: Bearer $PDO_TOKEN"}"#;

/// Runtime preamble for the library pipeline authoring assistant (ADR-0048,
/// reshaped by ADR-0051).
///
/// Prepended to `prompts/builtin/library-assistant.md`. Same discipline as the
/// manager: we own the session prompt, so the endpoints are documented in plain
/// text rather than shipping a custom MCP. The **write-on-save** rule is restated
/// here so it holds even if the static role file is trimmed.
///
/// **Every call carries the session's token** ([`AUTH_HEADER`]): one assistant
/// per Actor (CORE-61) means the daemon reads whose focus to use, and whose
/// template to save, off the caller's identity; without the token an
/// assistant would be read as the local operator's.
///
/// **It names no pipeline.** One assistant serves every template of its Actor, so a pipeline
/// id frozen at spawn would be wrong from the second template onwards. Which one
/// is open arrives per message via the `UserPromptSubmit` hook; the instruction to
/// fetch it is kept here in plain text because that hook only exists on a harness
/// exposing `--settings` (ADR-0051 §3).
pub fn build_library_assistant_preamble(daemon_url: &str) -> String {
    let auth = AUTH_HEADER;
    format!(
        r#"# Pipeline Assistant Runtime Preamble

You author PDO pipeline templates in natural language, on the user's behalf. You
are **the assistant of the person who opened you**: they switch between
templates without restarting you, so the template you work on changes over time
and your conversation history does not.

## Which pipeline is open

A line beginning `PDO — pipeline actuellement ouverte` is injected into your
context at **every** user message. It names the pipeline id and the **absolute
path** of its YAML file. Work on that pipeline — never on a sibling you guessed
from the working directory.

## You work in its shared draft

Everyone who has the pipeline open edits one **shared draft**, live; the file
holds only the last version a person saved. **Read the draft**, not the file:
`curl -s {auth} {daemon_url}/sessions/libassist/draft` answers
`{{"pipeline_id", "yaml", "prompts": {{"<node-id>": "<prompt markdown>"}}}}`. Your
save lands in that draft as you, live for everyone; only a person turns the
draft into a version. Read it again before each change: someone may have
changed it since.

If that line is missing (some harnesses cannot inject it), fetch the same fact
yourself **before acting**: `curl -s {auth} {daemon_url}/sessions/libassist/focus`. If it
answers no pipeline, ask the user which one — do not pick one.

**Every call to the daemon carries `{auth}`**, exactly as written: it sends
the token of your session when it has one. The daemon knows whose assistant you
are from it (whose template is open, where your save goes); a call without it is
read as somebody else's.

Your working directory is the repo's templates folder; read the `*.yaml` files in
it for real, in-house examples of the format. It is a source of examples, **not**
the location of the file you are editing: that is the absolute path the focus
gives you, and it may live in another folder entirely.

- Daemon base URL: `{daemon_url}`
- Which pipeline is open: `curl -s {auth} {daemon_url}/sessions/libassist/focus`
- Read its draft: `curl -s {auth} {daemon_url}/sessions/libassist/draft`
- List every template: `curl {auth} {daemon_url}/library/pipelines`
- Validate a single node's YAML before you save: `curl {auth} -X POST {daemon_url}/nodes/parse -H 'Content-Type: application/json' -d '{{"yaml":"<node yaml>"}}'`
- Persist the open template: `curl {auth} -X POST {daemon_url}/sessions/libassist/save -H 'Content-Type: application/json' -d '{{"yaml":"<full pipeline yaml>","prompts":{{"<node-id>":"<prompt markdown>"}}}}'`

**Saving takes no id and no scope.** The daemon writes into the draft of the
pipeline the focus names, because it is the only party that knows which one. Do **not** save
through `POST /library/pipelines`: it writes into the *library store*, a different
tree from the `.pdo/pipelines/` an edit tab opens — you would leave a duplicate
there, leave the edited file untouched, and report a save that did not happen.

## Write on save, never on every edit

Do **not** touch files as you reason. When the user asks for a change: describe
what you will change, **show a diff**, and write **only after the user says OK**.
Validate node YAML via `POST /nodes/parse` first; then persist the full template
via `POST /sessions/libassist/save`. Everyone's canvas shows your change the
moment you save — so send the whole template at once, never a half-edited one,
and never write the file with `Write`/`Edit`/`sed` (that is an edit outside PDO,
kept as a version of its own instead of landing in the draft).

You drive no Run and issue no run commands: your only durable effect is the YAML
the user reviews. Read first, propose second, write last.

"#
    )
}

/// Runtime preamble + the static role prompt. Mirror of the manager's prompt
/// builder.
pub fn build_library_assistant_prompt(daemon_url: &str, role_prompt: &str) -> String {
    let preamble = build_library_assistant_preamble(daemon_url);
    format!("{preamble}{role_prompt}")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The primer names **one** write path, and names the other as a trap.
    ///
    /// On a harness with no `--settings` hole the hook never arms (ADR-0051 §3),
    /// so this text is the only thing standing between the assistant and the bug
    /// it used to be instructed into: `POST /library/pipelines` reads `scope` in
    /// the library store's vocabulary, so a `repo` template saved there became a
    /// duplicate in another tree while the edited file never moved.
    #[test]
    fn library_assistant_preamble_points_the_save_at_the_focus() {
        let p = build_library_assistant_preamble("http://localhost:1234");

        assert!(
            p.contains("/sessions/libassist/save"),
            "the primer names the focus-driven save endpoint:\n{p}"
        );
        assert!(
            !p.contains("-X POST http://localhost:1234/library/pipelines"),
            "and never offers the library store as a way to save:\n{p}"
        );
        assert!(
            p.contains(&format!(
                "curl {AUTH_HEADER} -X POST http://localhost:1234/sessions/libassist/save"
            )),
            "the save carries the session's token, so it is its Actor's (CORE-61):\n{p}"
        );
        assert!(
            p.contains("Do **not** save\nthrough `POST /library/pipelines`"),
            "the trap is named outright, so a model that knows the old endpoint is \
             warned off it:\n{p}"
        );
        assert!(
            p.contains(&format!(
                "curl -s {AUTH_HEADER} http://localhost:1234/sessions/libassist/draft"
            )),
            "it reads the shared draft, with its token (ADR-0088):\n{p}"
        );
    }
    #[test]
    fn an_absent_focus_names_nothing_and_says_so() {
        let now = chrono::Utc::now();
        assert_eq!(
            focus_json(None, now),
            serde_json::json!({"pipeline_id": null, "scope": null, "path": null, "age_secs": null})
        );
        assert!(focus_text(None).contains("aucune pipeline ouverte"));
    }

    #[test]
    fn a_focus_names_its_file_and_the_one_save_endpoint() {
        let at = chrono::Utc::now();
        let focus = LibassistFocus {
            pipeline_id: "review".into(),
            path: Some(PathBuf::from("/repo/.pdo/pipelines/review.yaml")),
            at,
        };
        let json = focus_json(Some(&focus), at + chrono::Duration::seconds(7));
        assert_eq!(json["pipeline_id"], "review");
        assert_eq!(json["path"], "/repo/.pdo/pipelines/review.yaml");
        assert_eq!(json["age_secs"], 7);
        let text = focus_text(Some(&focus));
        assert!(text.contains("`review`") && text.contains("/repo/.pdo/pipelines/review.yaml"));
        assert!(text.contains("POST /sessions/libassist/save"));
        assert!(
            text.contains("GET /sessions/libassist/draft"),
            "it reads the shared draft, not the file (ADR-0088): {text}"
        );
        let unsaved = LibassistFocus {
            path: None,
            ..focus
        };
        assert!(focus_text(Some(&unsaved)).contains("template non sauvée"));
        assert_eq!(
            libassist_cwd(Path::new("/repo")),
            PathBuf::from("/repo/.pdo/pipelines")
        );
        assert_eq!(
            libassist_state_dir(Path::new("/repo")),
            PathBuf::from("/repo/.pdo/.libassist")
        );
        assert_eq!(
            libassist_primer_path(Path::new("/repo"), "pdo-libassist-shared"),
            PathBuf::from("/repo/.pdo/.libassist/assistant.md")
        );
        assert_eq!(
            libassist_primer_path(Path::new("/repo"), "pdo-libassist-a01"),
            PathBuf::from("/repo/.pdo/.libassist/pdo-libassist-a01.md")
        );
    }
}
