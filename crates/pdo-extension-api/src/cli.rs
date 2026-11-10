use std::path::PathBuf;

use serde::Serialize;

use crate::Actor;

/// The exit code `pdo ext <id>` leaves with when the daemon has no extension
/// `<id>`, or one without a command line. An extension should not answer it
/// itself: its caller could not tell the two apart.
pub const EXIT_NO_CLI: u8 = 127;

/// What an extension says about its command line, `pdo ext <id> <args…>`: the
/// one line `pdo ext` shows next to its id.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[non_exhaustive]
pub struct CliSpec {
    pub summary: String,
}

impl CliSpec {
    pub fn new(summary: impl Into<String>) -> Self {
        Self {
            summary: summary.into(),
        }
    }
}

/// One `pdo ext <id> <args…>` call, as [`Extension::run_cli`] receives it.
///
/// [`Extension::run_cli`]: crate::Extension::run_cli
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct CliInvocation {
    /// The arguments after the id, verbatim (`pdo ext test note list --json`
    /// gives `["note", "list", "--json"]`).
    pub args: Vec<String>,
    /// Everything the command read on its standard input when that input is a
    /// pipe or a file; empty otherwise (a terminal, `/dev/null`, the socket an
    /// agent's shell tool leaves open).
    pub stdin: Vec<u8>,
    /// Who runs the command: the agent of the node session it comes from, the
    /// local operator otherwise (or whom an extension's `identify` names).
    pub actor: Actor,
    /// The caller's working directory, to resolve a relative path it names.
    pub cwd: Option<PathBuf>,
}

impl CliInvocation {
    pub fn new(args: Vec<String>, stdin: impl Into<Vec<u8>>, actor: Actor) -> Self {
        Self {
            args,
            stdin: stdin.into(),
            actor,
            cwd: None,
        }
    }

    /// The same call, made from `cwd`.
    pub fn with_cwd(mut self, cwd: impl Into<PathBuf>) -> Self {
        self.cwd = Some(cwd.into());
        self
    }
}

/// The answer of [`Extension::run_cli`]: what `pdo ext` writes on its standard
/// output and error, byte for byte, and the code it exits with.
///
/// [`Extension::run_cli`]: crate::Extension::run_cli
#[derive(Debug, Clone, Default, PartialEq, Eq)]
#[non_exhaustive]
pub struct CliOutput {
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    pub code: u8,
}

impl CliOutput {
    /// Exit `0`, writing `stdout`.
    pub fn success(stdout: impl Into<Vec<u8>>) -> Self {
        Self {
            stdout: stdout.into(),
            ..Self::default()
        }
    }

    /// Exit `code`, writing `stderr`.
    pub fn failure(code: u8, stderr: impl Into<Vec<u8>>) -> Self {
        Self {
            stderr: stderr.into(),
            code,
            ..Self::default()
        }
    }

    /// The same answer, writing `stdout`.
    pub fn with_stdout(mut self, stdout: impl Into<Vec<u8>>) -> Self {
        self.stdout = stdout.into();
        self
    }

    /// The same answer, writing `stderr`.
    pub fn with_stderr(mut self, stderr: impl Into<Vec<u8>>) -> Self {
        self.stderr = stderr.into();
        self
    }

    /// The same answer, exiting with `code`.
    pub fn with_code(mut self, code: u8) -> Self {
        self.code = code;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builds_an_invocation_and_its_answers() {
        let call =
            CliInvocation::new(vec!["echo".into()], "in", Actor::LocalOperator).with_cwd("/tmp");
        assert_eq!(call.stdin, b"in");
        assert_eq!(call.cwd, Some(PathBuf::from("/tmp")));
        let ok = CliOutput::success("salut\n");
        assert_eq!((ok.stdout.as_slice(), ok.code), (&b"salut\n"[..], 0));
        let ko = CliOutput::failure(4, "not found\n").with_stdout("{}");
        assert_eq!((ko.stderr.as_slice(), ko.code), (&b"not found\n"[..], 4));
        assert_eq!(ko.with_code(3).code, 3);
        assert_eq!(
            serde_json::to_value(CliSpec::new("Echo")).unwrap(),
            serde_json::json!({ "summary": "Echo" })
        );
    }
}
