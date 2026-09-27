//! The update journal on disk, under `<home>/.pdo/update/`: `check.json` (the
//! last check), `last-attempt.json`, and one `<attempt>.sh` / `<attempt>.log`
//! per attempt. Records are written atomically (temp + rename).

use std::path::{Path, PathBuf};

use serde::{de::DeserializeOwned, Serialize};

use crate::application::ports::UpdateJournal;
use crate::domain::attempt::{UpdateAttempt, ATTEMPT_SCHEMA};
use crate::domain::update_check::{UpdateCache, CACHE_SCHEMA};

pub struct FsUpdateJournal {
    /// `None` when there is no home directory: nothing can be journaled.
    home: Option<PathBuf>,
}

impl FsUpdateJournal {
    /// The journal of the home `home` (the per-daemon override, else `$HOME`).
    pub fn new(home: Option<PathBuf>) -> Self {
        Self { home }
    }

    fn dir(&self) -> PathBuf {
        self.home
            .clone()
            .unwrap_or_default()
            .join(".pdo")
            .join("update")
    }

    fn cache_path(&self) -> PathBuf {
        self.dir().join("check.json")
    }
}

/// Read a record, `None` when absent, unreadable or of a foreign schema (a
/// corrupt record is simply "none yet", never an error).
fn read_record<T: DeserializeOwned>(
    path: &Path,
    schema_of: impl Fn(&T) -> &str,
    schema: &str,
) -> Option<T> {
    let text = std::fs::read_to_string(path).ok()?;
    let record: T = serde_json::from_str(&text).ok()?;
    (schema_of(&record) == schema).then_some(record)
}

/// Write a record atomically (temp + rename), creating the directory.
fn write_record<T: Serialize>(path: &Path, record: &T) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    let tmp = path.with_extension("json.tmp");
    let text = serde_json::to_string_pretty(record).map_err(|e| e.to_string())?;
    std::fs::write(&tmp, text).map_err(|e| e.to_string())?;
    std::fs::rename(&tmp, path).map_err(|e| e.to_string())
}

impl UpdateJournal for FsUpdateJournal {
    fn is_available(&self) -> bool {
        self.home.is_some()
    }

    fn read_cache(&self) -> Option<UpdateCache> {
        read_record(
            &self.cache_path(),
            |c: &UpdateCache| &c.schema,
            CACHE_SCHEMA,
        )
    }

    fn write_cache(&self, cache: &UpdateCache) -> Result<(), String> {
        write_record(&self.cache_path(), cache)
    }

    fn read_attempt(&self) -> Option<UpdateAttempt> {
        read_record(
            &self.attempt_path(),
            |a: &UpdateAttempt| &a.schema,
            ATTEMPT_SCHEMA,
        )
    }

    fn write_attempt(&self, attempt: &UpdateAttempt) -> Result<(), String> {
        write_record(&self.attempt_path(), attempt)
    }

    fn attempt_path(&self) -> PathBuf {
        self.dir().join("last-attempt.json")
    }

    fn log_path(&self, attempt_id: &str) -> PathBuf {
        self.dir().join(format!("{attempt_id}.log"))
    }

    fn write_script(&self, attempt_id: &str, script: &str) -> Result<PathBuf, String> {
        let path = self.dir().join(format!("{attempt_id}.sh"));
        std::fs::create_dir_all(self.dir()).map_err(|e| e.to_string())?;
        std::fs::write(&path, script)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?;
        Ok(path)
    }

    fn append_log(&self, attempt_id: &str, line: &str) {
        use std::io::Write;
        let _ = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.log_path(attempt_id))
            .and_then(|mut f| writeln!(f, "{line}"));
    }

    fn read_log(&self, attempt_id: &str) -> Result<Option<String>, String> {
        match std::fs::read_to_string(self.log_path(attempt_id)) {
            Ok(text) => Ok(Some(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::attempt::AttemptStatus;
    use crate::domain::install::{InstallMethod, Supervision};
    use crate::domain::update_plan::{render_update_script, UpdatePlan};

    fn journal() -> (tempfile::TempDir, FsUpdateJournal) {
        let dir = tempfile::tempdir().unwrap();
        let journal = FsUpdateJournal::new(Some(dir.path().to_path_buf()));
        (dir, journal)
    }

    #[test]
    fn the_journal_lives_under_the_home() {
        let (dir, j) = journal();
        assert_eq!(
            j.attempt_path(),
            dir.path().join(".pdo/update/last-attempt.json")
        );
        assert_eq!(j.log_path("x"), dir.path().join(".pdo/update/x.log"));
        assert!(j.is_available());
        assert!(!FsUpdateJournal::new(None).is_available());
    }

    #[test]
    fn cache_round_trips_and_a_foreign_schema_reads_as_never_checked() {
        let (_dir, j) = journal();
        assert!(j.read_cache().is_none());
        let cache = UpdateCache {
            schema: CACHE_SCHEMA.to_string(),
            source: "http://x".into(),
            checked_at: Some(chrono::Utc::now().to_rfc3339()),
            latest_version: Some("1.59.0".into()),
            error: None,
        };
        j.write_cache(&cache).unwrap();
        assert_eq!(j.read_cache(), Some(cache));
        std::fs::write(j.cache_path(), r#"{"schema":"other"}"#).unwrap();
        assert!(j.read_cache().is_none());
    }

    fn attempt(j: &FsUpdateJournal) -> UpdateAttempt {
        UpdateAttempt {
            schema: ATTEMPT_SCHEMA.into(),
            attempt_id: "x".into(),
            status: AttemptStatus::Running,
            started_at: "2026-09-05T12:00:00Z".into(),
            finished_at: None,
            exit_code: None,
            method: InstallMethod::Homebrew,
            command: "brew update && brew upgrade Loulen/tap/pdo".into(),
            supervision: Supervision::None,
            log_path: j.log_path("x"),
            from_version: "1.60.0".into(),
        }
    }

    #[test]
    fn attempt_record_round_trips() {
        let (_dir, j) = journal();
        assert!(j.read_attempt().is_none());
        let a = attempt(&j);
        j.write_attempt(&a).unwrap();
        assert_eq!(j.read_attempt(), Some(a));
        std::fs::write(j.attempt_path(), r#"{"schema":"other"}"#).unwrap();
        assert!(j.read_attempt().is_none());
    }

    #[test]
    fn scripts_and_logs_are_kept_per_attempt() {
        let (_dir, j) = journal();
        assert_eq!(j.read_log("x"), Ok(None));
        let script = j.write_script("x", "#!/bin/sh\n").unwrap();
        assert_eq!(std::fs::read_to_string(script).unwrap(), "#!/bin/sh\n");
        j.append_log("x", "== one");
        j.append_log("x", "== two");
        assert_eq!(j.read_log("x"), Ok(Some("== one\n== two\n".to_string())));
    }

    /// The script's `record` line must produce JSON the journal reads back: run
    /// the function under a real `sh` against a tempdir.
    #[test]
    fn the_scripts_record_function_writes_a_readable_attempt() {
        let (dir, j) = journal();
        let plan = UpdatePlan {
            attempt_id: "20260905-120000-abc123".into(),
            method: InstallMethod::Homebrew,
            supervision: Supervision::None,
            exe: PathBuf::from("/home/linuxbrew/.linuxbrew/bin/pdo"),
            port: 5172,
            bind: None,
            working_dir: PathBuf::from("/home/u/.pdo/app"),
            daemon_pid: 4242,
            relaunch: vec!["pdo".into(), "daemon".into()],
            log_path: j.log_path("20260905-120000-abc123"),
            attempt_path: j.attempt_path(),
            from_version: "1.60.0".into(),
        };
        std::fs::create_dir_all(j.dir()).unwrap();
        let script = render_update_script(&plan);
        // Keep only the preamble + `record`, then call it.
        let head: String = script
            .lines()
            .take_while(|l| !l.starts_with("echo \"== pdo update"))
            .map(|l| format!("{l}\n"))
            .collect();
        let probe = format!("{head}record failed 7\n");
        let out = std::process::Command::new("sh")
            .arg("-c")
            .arg(&probe)
            .env("HOME", dir.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let a = j.read_attempt().expect("readable JSON");
        assert_eq!(a.status, AttemptStatus::Failed);
        assert_eq!(a.exit_code, Some(7));
        assert_eq!(a.attempt_id, plan.attempt_id);
        assert_eq!(a.command, "brew update && brew upgrade Loulen/tap/pdo");
        assert_eq!(a.method, InstallMethod::Homebrew);
        assert_eq!(a.log_path, plan.log_path);
        assert!(a.finished_at.is_some());
    }
}
