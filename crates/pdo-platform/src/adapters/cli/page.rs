//! `pdo page` — mount, list and unmount pages through the running daemon.

use anyhow::{Context, Result};
use pdo_kernel::actor::{SESSION_NODE_HEADER, SESSION_RUN_HEADER};
use pdo_kernel::public_url::{guided_page_url, page_url};

use super::PageAction;
use crate::domain::page_mount::PageMount;
use crate::domain::service_unit::DEFAULT_DAEMON_URL;

/// The node session this CLI runs in, when it does: both `PDO_RUN_ID` and
/// `PDO_NODE_ID`, non-blank.
fn session_env_claim() -> Option<(String, String)> {
    let clean = |v: Result<String, std::env::VarError>| {
        v.ok()
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
    };
    let run_id = clean(std::env::var("PDO_RUN_ID"))?;
    let node_id = clean(std::env::var("PDO_NODE_ID"))?;
    Some((run_id, node_id))
}

pub fn run_page(action: PageAction) -> Result<()> {
    let url = std::env::var("PDO_DAEMON_URL").unwrap_or_else(|_| DEFAULT_DAEMON_URL.to_string());
    let client = reqwest::blocking::Client::builder()
        .default_headers(pdo_kernel::env::cli_headers())
        .build()
        .unwrap_or_default();
    let session = session_env_claim();
    let with_identity = |request: reqwest::blocking::RequestBuilder| {
        let mut request = request.header("X-PDO-Actor", "cli");
        if let Some((run_id, node_id)) = &session {
            request = request
                .header(SESSION_RUN_HEADER, run_id)
                .header(SESSION_NODE_HEADER, node_id);
        }
        request
    };

    match action {
        PageAction::Mount {
            name,
            directory,
            guide,
        } => {
            let directory = directory
                .canonicalize()
                .with_context(|| format!("cannot resolve {}", directory.display()))?;
            let guide = guide
                .map(|path| {
                    std::fs::read_to_string(&path)
                        .with_context(|| format!("cannot read the guide {}", path.display()))
                })
                .transpose()?;
            let response = with_identity(client.post(format!("{url}/pages")))
                .json(&serde_json::json!({ "name": name, "directory": directory, "guide": guide }))
                .send()
                .context("failed to reach daemon")?;
            let mounted: MountedPage = daemon_response(response, "mount page")?;
            print!("{}", mounted_text(&mounted, &url));
        }
        PageAction::List => {
            let response = with_identity(client.get(format!("{url}/pages")))
                .send()
                .context("failed to reach daemon")?;
            let mounts: Vec<PageMount> = daemon_response(response, "list pages")?;
            for mount in mounts {
                let run = mount.run_id.as_deref().unwrap_or("-");
                println!(
                    "{}\t{}\t{}\t{}",
                    mount.name, mount.directory, run, mount.created_at
                );
            }
        }
        PageAction::Unmount { name } => {
            let response = with_identity(client.delete(format!("{url}/pages/{name}")))
                .send()
                .context("failed to reach daemon")?;
            let _: PageMount = daemon_response(response, "unmount page")?;
            println!("Unmounted {name}.");
        }
    }
    Ok(())
}

/// What `POST /pages` answers: the mount, and the Public URL to print its
/// links under.
#[derive(serde::Deserialize)]
struct MountedPage {
    #[serde(flatten)]
    mount: PageMount,
    #[serde(default)]
    public_url: Option<String>,
}

/// The lines `pdo page mount` prints: the full address of PDO's view with the
/// guide when there is one, of the page alone otherwise. Under the instance's
/// Public URL, else the daemon address this CLI reached.
fn mounted_text(mounted: &MountedPage, daemon_url: &str) -> String {
    let base = mounted.public_url.as_deref().unwrap_or(daemon_url);
    let name = &mounted.mount.name;
    let page = page_url(base, name);
    if mounted.mount.guide.is_some() {
        format!(
            "Mounted {name} from {}.\nWith its guide: {}\nPage alone: {page}\n",
            mounted.mount.directory,
            guided_page_url(base, name)
        )
    } else {
        format!(
            "Mounted {name} at {page} from {}.\n",
            mounted.mount.directory
        )
    }
}

/// The body of a success, or the daemon's own `error` sentence.
fn daemon_response<T: serde::de::DeserializeOwned>(
    response: reqwest::blocking::Response,
    operation: &str,
) -> Result<T> {
    let status = response.status();
    let raw = response.text().unwrap_or_default();
    if !status.is_success() {
        let parsed: serde_json::Value =
            serde_json::from_str(&raw).unwrap_or(serde_json::Value::Null);
        let detail = parsed
            .get("error")
            .and_then(|value| value.as_str())
            .unwrap_or(raw.as_str());
        anyhow::bail!("daemon refused to {operation} ({status}): {detail}");
    }
    serde_json::from_str(&raw)
        .with_context(|| format!("unreadable success body from daemon: {raw}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::page_guide::GuideEntry;

    fn mounted(guide: bool, public_url: Option<&str>) -> MountedPage {
        MountedPage {
            mount: PageMount {
                name: "demo".into(),
                directory: "/srv/demo".into(),
                run_id: None,
                node_id: None,
                created_at: "now".into(),
                guide: guide.then(|| {
                    vec![GuideEntry {
                        title: "t".into(),
                        text: String::new(),
                        link: "#a".into(),
                    }]
                }),
            },
            public_url: public_url.map(str::to_string),
        }
    }

    #[test]
    fn a_guided_page_prints_pdo_s_view_first_under_the_public_url() {
        assert_eq!(
            mounted_text(
                &mounted(true, Some("https://pdo.example.net")),
                "http://localhost:5172"
            ),
            "Mounted demo from /srv/demo.\n\
             With its guide: https://pdo.example.net/#/pages/demo\n\
             Page alone: https://pdo.example.net/pages/demo/\n"
        );
    }

    #[test]
    fn a_page_alone_prints_its_address_under_the_daemon_s_without_a_public_url() {
        assert_eq!(
            mounted_text(&mounted(false, None), "http://localhost:5172"),
            "Mounted demo at http://localhost:5172/pages/demo/ from /srv/demo.\n"
        );
    }
}
