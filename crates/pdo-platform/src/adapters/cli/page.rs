//! `pdo page` — mount, list and unmount pages through the running daemon. A
//! refusal prints the daemon's sentence, as every command does.

use anyhow::{Context, Result};
use pdo_kernel::public_url::{guided_page_url, page_url};

use super::daemon::{segment, Daemon};
use super::PageAction;
use crate::domain::page_mount::PageMount;

pub fn run_page(action: PageAction) -> Result<()> {
    let daemon = Daemon::from_env();
    match action {
        PageAction::Mount {
            name,
            directory,
            guide,
            project,
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
            let body = serde_json::json!({
                "name": name,
                "directory": directory,
                "guide": guide,
                "project_id": project,
            });
            let answer = daemon.post("/pages", Some(&body))?.ok()?;
            let mounted: MountedPage =
                serde_json::from_slice(&answer.bytes).with_context(|| {
                    format!("unreadable success body from daemon: {}", answer.text())
                })?;
            print!("{}", mounted_text(&mounted, daemon.url()));
        }
        PageAction::List => {
            let answer = daemon.get("/pages")?.ok()?;
            let mounts: Vec<PageMount> =
                serde_json::from_slice(&answer.bytes).with_context(|| {
                    format!("unreadable success body from daemon: {}", answer.text())
                })?;
            print!("{}", list_text(&mounts));
        }
        PageAction::Unmount { name } => {
            daemon
                .delete(&format!("/pages/{}", segment(&name)), None)?
                .ok()?;
            println!("Unmounted {name}.");
        }
    }
    Ok(())
}

/// The lines `pdo page list` prints: name, directory, Run, creation, Projet
/// (`-` for none), tab-separated.
fn list_text(mounts: &[PageMount]) -> String {
    mounts
        .iter()
        .map(|mount| {
            format!(
                "{}\t{}\t{}\t{}\t{}\n",
                mount.name,
                mount.directory,
                mount.run_id.as_deref().unwrap_or("-"),
                mount.created_at,
                mount.project_id.as_deref().unwrap_or("-"),
            )
        })
        .collect()
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
                owner: None,
                project_id: None,
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

    #[test]
    fn the_list_names_the_projet_of_each_page() {
        let mut in_p = mounted(false, None).mount;
        in_p.project_id = Some("p1".into());
        in_p.run_id = Some("r1".into());
        let alone = mounted(false, None).mount;
        assert_eq!(
            list_text(&[in_p, alone]),
            "demo\t/srv/demo\tr1\tnow\tp1\ndemo\t/srv/demo\t-\tnow\t-\n"
        );
    }
}
