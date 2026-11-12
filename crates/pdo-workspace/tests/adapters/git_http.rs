//! A git remote served over HTTP with Basic authentication: `git
//! http-backend` behind a local axum server, for the tests of the credentials
//! a network operation runs with. Self-contained: the daemon's tests include
//! it too (`#[path]`).

#![allow(dead_code)]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use axum::body::{Body, Bytes};
use axum::http::{HeaderMap, Method, StatusCode, Uri};
use axum::response::Response;

/// The user the server accepts.
pub const USER: &str = "alice";

/// A served bare repository `acme/app`, and what it takes to reach it.
pub struct AuthGitServer {
    /// `http://127.0.0.1:<port>/acme/app` (an `owner/name` path, which a
    /// Source de skills also reads).
    pub url: String,
    /// The one password the server accepts.
    pub secret: String,
    _root: tempfile::TempDir,
    _task: tokio::task::JoinHandle<()>,
}

impl AuthGitServer {
    /// Serve a fresh bare repository holding one commit, accepting
    /// [`USER`] with a random password only.
    pub async fn start() -> Self {
        let root = tempfile::tempdir().unwrap();
        let seed = tempfile::tempdir().unwrap();
        for args in [
            &["init", "--quiet", "-b", "main"][..],
            &[
                "-c",
                "user.name=T",
                "-c",
                "user.email=t@t",
                "commit",
                "--quiet",
                "--allow-empty",
                "-m",
                "initial",
            ],
        ] {
            let out = std::process::Command::new("git")
                .args(args)
                .current_dir(seed.path())
                .output()
                .unwrap();
            assert!(
                out.status.success(),
                "{}",
                String::from_utf8_lossy(&out.stderr)
            );
        }
        std::fs::write(seed.path().join("README.md"), "# app\n").unwrap();
        let out = std::process::Command::new("sh")
            .args(["-c", "git add README.md && git -c user.name=T -c user.email=t@t commit --quiet -m readme"])
            .current_dir(seed.path())
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let bare = root.path().join("acme/app");
        let out = std::process::Command::new("git")
            .args(["clone", "--quiet", "--bare"])
            .arg(seed.path())
            .arg(&bare)
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let secret = format!("s3cr3t-{}", uuid_like());
        let expected = format!("Basic {}", base64(&format!("{USER}:{secret}")));
        let project_root = root.path().to_path_buf();
        let app = axum::Router::new().fallback(
            move |method: Method, uri: Uri, headers: HeaderMap, body: Bytes| {
                let expected = expected.clone();
                let project_root = project_root.clone();
                async move { serve(&project_root, &expected, method, uri, headers, body).await }
            },
        );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        let task = tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });
        Self {
            url: format!("http://127.0.0.1:{port}/acme/app"),
            secret,
            _root: root,
            _task: task,
        }
    }
}

/// What an extension would provide for an Actor: a credential helper (the
/// script `helper`, which logs to `log`) named through `GIT_CONFIG_*`, and
/// the secret under a name of its own, which the helper reads. The user's
/// global and system git configuration stay out, so no helper of theirs
/// answers first.
pub fn provided_env(helper: &Path, log: &Path, secret: &str) -> BTreeMap<String, String> {
    BTreeMap::from([
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
        ("GIT_CONFIG_COUNT".into(), "1".into()),
        ("GIT_CONFIG_KEY_0".into(), "credential.helper".into()),
        ("GIT_CONFIG_VALUE_0".into(), helper.display().to_string()),
        ("TEST_HELPER_LOG".into(), log.display().to_string()),
        ("TEST_GIT_SECRET".into(), secret.into()),
    ])
}

/// The environment of an Actor nothing is provided for, as hermetic as
/// [`provided_env`].
pub fn bare_env() -> BTreeMap<String, String> {
    BTreeMap::from([
        ("GIT_CONFIG_GLOBAL".into(), "/dev/null".into()),
        ("GIT_CONFIG_NOSYSTEM".into(), "1".into()),
    ])
}

/// Write the credential helper into `dir`: on `get`, it logs `called`, scans
/// the command line of every process for the secret (logging `leaked: <the
/// command line>` on a match, with shell builtins only, so the scan puts the
/// secret in no argv of its own) and answers [`USER`] and the secret.
pub fn write_helper(dir: &Path) -> (PathBuf, PathBuf) {
    let helper = dir.join("helper.sh");
    let log = dir.join("helper.log");
    std::fs::write(
        &helper,
        r#"#!/bin/sh
test "$1" = get || exit 0
echo called >> "$TEST_HELPER_LOG"
for f in /proc/[0-9]*/cmdline; do
  c=$(tr '\0' ' ' < "$f" 2>/dev/null)
  case "$c" in
    *"$TEST_GIT_SECRET"*) echo "leaked: $c" >> "$TEST_HELPER_LOG" ;;
  esac
done
printf 'username=alice\npassword=%s\n' "$TEST_GIT_SECRET"
"#,
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&helper, std::fs::Permissions::from_mode(0o755)).unwrap();
    (helper, log)
}

/// What the helper logged.
pub fn helper_log(log: &Path) -> String {
    std::fs::read_to_string(log).unwrap_or_default()
}

async fn serve(
    project_root: &Path,
    expected: &str,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let authorized = headers
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| v == expected);
    if !authorized {
        return Response::builder()
            .status(StatusCode::UNAUTHORIZED)
            .header("WWW-Authenticate", "Basic realm=\"git\"")
            .body(Body::empty())
            .unwrap();
    }
    let header = |name: &str| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_string()
    };
    let mut cmd = tokio::process::Command::new("git");
    cmd.arg("http-backend")
        .env("GIT_PROJECT_ROOT", project_root)
        .env("GIT_HTTP_EXPORT_ALL", "1")
        .env("PATH_INFO", uri.path())
        .env("QUERY_STRING", uri.query().unwrap_or(""))
        .env("REQUEST_METHOD", method.as_str())
        .env("CONTENT_TYPE", header("content-type"))
        .env("CONTENT_LENGTH", body.len().to_string())
        .env("HTTP_CONTENT_ENCODING", header("content-encoding"))
        .env("HTTP_GIT_PROTOCOL", header("git-protocol"))
        .env("REMOTE_USER", USER)
        .env("REMOTE_ADDR", "127.0.0.1")
        .stdin(std::process::Stdio::piped())
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null());
    let mut child = cmd.spawn().unwrap();
    {
        use tokio::io::AsyncWriteExt;
        let mut stdin = child.stdin.take().unwrap();
        stdin.write_all(&body).await.unwrap();
    }
    let out = child.wait_with_output().await.unwrap();
    cgi_response(&out.stdout)
}

/// The CGI output of `git http-backend` as an HTTP response.
fn cgi_response(raw: &[u8]) -> Response {
    let split = raw
        .windows(4)
        .position(|w| w == b"\r\n\r\n")
        .map(|i| (i, 4))
        .or_else(|| raw.windows(2).position(|w| w == b"\n\n").map(|i| (i, 2)));
    let Some((at, gap)) = split else {
        return Response::builder()
            .status(StatusCode::INTERNAL_SERVER_ERROR)
            .body(Body::empty())
            .unwrap();
    };
    let head = String::from_utf8_lossy(&raw[..at]);
    let mut response = Response::builder();
    for line in head.lines() {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        let value = value.trim();
        if name.eq_ignore_ascii_case("status") {
            let code = value.split_whitespace().next().unwrap_or("200");
            response = response.status(code.parse::<u16>().unwrap_or(200));
        } else {
            response = response.header(name.trim(), value);
        }
    }
    response.body(Body::from(raw[at + gap..].to_vec())).unwrap()
}

fn uuid_like() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    format!("{nanos:x}{:x}", std::process::id())
}

fn base64(text: &str) -> String {
    const TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let bytes = text.as_bytes();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for i in 0..4 {
            if i <= chunk.len() {
                out.push(TABLE[((n >> (18 - 6 * i)) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

#[test]
fn base64_encodes_as_the_rfc_says() {
    assert_eq!(base64("alice:pw"), "YWxpY2U6cHc=");
    assert_eq!(base64("ab"), "YWI=");
    assert_eq!(base64("abc"), "YWJj");
}
