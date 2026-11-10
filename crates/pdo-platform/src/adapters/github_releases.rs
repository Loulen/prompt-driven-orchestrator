//! The release source over HTTP: the GitHub Releases API (or the local fixture a
//! test points the daemon at).

use std::time::Duration;

use crate::application::ports::{BoxFuture, ReleaseSource};
use crate::domain::release::{parse_release_body, parse_releases_body, Release};

/// Hard timeout on a release fetch; a slow GitHub must never hold anything.
pub const UPDATE_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// Optional GitHub token to lift the unauthenticated rate limit. Env only,
/// deliberately not a setting.
pub const GITHUB_TOKEN_ENV: &str = "PDO_GITHUB_TOKEN";

pub struct GithubReleases;

/// GET `url` with the headers GitHub requires (a `User-Agent`) and the token
/// when one is set; the body of a success.
async fn fetch(url: &str) -> Result<String, String> {
    let client = reqwest::Client::builder()
        .timeout(UPDATE_FETCH_TIMEOUT)
        .user_agent(format!("pdo/{}", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let mut req = client
        .get(url)
        .header("accept", "application/vnd.github+json");
    if let Ok(token) = std::env::var(GITHUB_TOKEN_ENV) {
        if !token.is_empty() {
            req = req.bearer_auth(token);
        }
    }
    let resp = req.send().await.map_err(|e| e.to_string())?;
    let status = resp.status();
    if !status.is_success() {
        return Err(format!("HTTP {status}"));
    }
    resp.text().await.map_err(|e| e.to_string())
}

impl ReleaseSource for GithubReleases {
    fn latest<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<String, String>> {
        Box::pin(async move { parse_release_body(&fetch(url).await?) })
    }

    fn releases<'a>(&'a self, url: &'a str) -> BoxFuture<'a, Result<Vec<Release>, String>> {
        Box::pin(async move { parse_releases_body(&fetch(url).await?) })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    /// A one-shot HTTP server answering `status` and `body` to the first request.
    async fn serve_once(status: &'static str, body: &'static str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut buf = [0u8; 4096];
            let _ = socket.read(&mut buf).await;
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            let _ = socket.write_all(response.as_bytes()).await;
        });
        format!("http://{addr}/releases/latest")
    }

    #[tokio::test]
    async fn the_latest_release_is_read_from_the_source() {
        let url = serve_once("200 OK", r#"{"tag_name":"v9.8.7"}"#).await;
        assert_eq!(GithubReleases.latest(&url).await, Ok("9.8.7".to_string()));
    }

    #[tokio::test]
    async fn a_failing_source_is_an_error_with_its_status() {
        let url = serve_once("503 Service Unavailable", "{}").await;
        assert_eq!(
            GithubReleases.latest(&url).await,
            Err("HTTP 503 Service Unavailable".to_string())
        );
    }

    #[tokio::test]
    async fn the_release_list_drops_what_is_not_a_release() {
        let url = serve_once(
            "200 OK",
            r#"[{"tag_name":"v2.0.0","body":"- a"},{"tag_name":"v2.1.0","draft":true}]"#,
        )
        .await;
        let releases = GithubReleases.releases(&url).await.unwrap();
        assert_eq!(releases.len(), 1);
        assert_eq!(releases[0].version, "2.0.0");
    }
}
