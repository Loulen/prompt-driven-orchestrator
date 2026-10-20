//! Layer 4 (ADR-0080) — the Projet and page-mount routes no integration test
//! called before the refactor net (#914): list and delete a Projet, detach a
//! member, a Projet's provisioning rules, and the trailing-slash page listing.

use std::path::Path;

use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &Path) -> anyhow::Result<()> {
    flow::git_init(repo)
}

async fn create_project(daemon: &TestDaemon, name: &str) -> serde_json::Value {
    let (status, body) =
        flow::post_json(daemon, "/projects", serde_json::json!({ "name": name })).await;
    assert_eq!(status, 201, "{body}");
    body
}

async fn remove_member(
    daemon: &TestDaemon,
    project_id: &str,
    path: &str,
) -> (u16, serde_json::Value) {
    let resp = reqwest::Client::new()
        .delete(format!("{}/projects/{project_id}/members", daemon.url()))
        .json(&serde_json::json!({ "path": path }))
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

#[tokio::test]
async fn projects_are_listed_then_deleted_with_their_members() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    assert_eq!(daemon.get_json("/projects").await, serde_json::json!([]));

    let alpha = create_project(&daemon, "Alpha").await;
    let beta = create_project(&daemon, "Beta").await;
    let alpha_id = alpha["id"].as_str().unwrap().to_string();
    let (status, _) = flow::post_json(
        &daemon,
        &format!("/projects/{alpha_id}/members"),
        serde_json::json!({ "path": "/srv/alpha" }),
    )
    .await;
    assert_eq!(status, 200);

    let list = daemon.get_json("/projects").await;
    let names: Vec<&str> = list
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|p| p["name"].as_str())
        .collect();
    assert_eq!(names.len(), 2, "{list}");
    assert!(
        names.contains(&"Alpha") && names.contains(&"Beta"),
        "{list}"
    );
    let listed_alpha = list
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["id"] == alpha_id.as_str())
        .unwrap();
    assert_eq!(listed_alpha["members"], serde_json::json!(["/srv/alpha"]));

    let resp = reqwest::Client::new()
        .delete(format!("{}/projects/{alpha_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let list = daemon.get_json("/projects").await;
    assert_eq!(list.as_array().unwrap().len(), 1, "{list}");
    assert_eq!(list[0]["id"], beta["id"]);

    // The detached path is free again for another Projet.
    let beta_id = beta["id"].as_str().unwrap();
    let (status, body) = flow::post_json(
        &daemon,
        &format!("/projects/{beta_id}/members"),
        serde_json::json!({ "path": "/srv/alpha" }),
    )
    .await;
    assert_eq!(status, 200, "{body}");

    let resp = reqwest::Client::new()
        .delete(format!("{}/projects/{alpha_id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404, "already gone");
}

#[tokio::test]
async fn removing_a_member_detaches_it_only_from_its_own_project() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let alpha = create_project(&daemon, "Alpha").await;
    let beta = create_project(&daemon, "Beta").await;
    let alpha_id = alpha["id"].as_str().unwrap();
    let beta_id = beta["id"].as_str().unwrap();
    for (id, path) in [
        (alpha_id, "/srv/a1"),
        (alpha_id, "/srv/a2"),
        (beta_id, "/srv/b1"),
    ] {
        let (status, _) = flow::post_json(
            &daemon,
            &format!("/projects/{id}/members"),
            serde_json::json!({ "path": path }),
        )
        .await;
        assert_eq!(status, 200);
    }

    let (status, body) = remove_member(&daemon, alpha_id, "/srv/a1").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["members"], serde_json::json!(["/srv/a2"]));

    // A path owned by another Projet is left alone: idempotent, answers the
    // unchanged Projet.
    let (status, body) = remove_member(&daemon, alpha_id, "/srv/b1").await;
    assert_eq!(status, 200, "{body}");
    assert_eq!(body["members"], serde_json::json!(["/srv/a2"]));
    let beta_now = daemon.get_json(&format!("/projects/{beta_id}")).await;
    assert_eq!(beta_now["members"], serde_json::json!(["/srv/b1"]));

    let (status, _) = remove_member(&daemon, "no-such-project", "/srv/zz").await;
    assert_eq!(status, 404);
}

#[tokio::test]
async fn naming_a_project_with_members_attaches_them_even_when_shared() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (status, alpha) = flow::post_json(
        &daemon,
        "/projects",
        serde_json::json!({ "name": "Alpha", "members": ["/srv/a1", "/srv/a2"] }),
    )
    .await;
    assert_eq!(status, 201, "{alpha}");
    assert_eq!(alpha["members"], serde_json::json!(["/srv/a1", "/srv/a2"]));

    // ADR-0087: a member of Alpha joins Beta too; Alpha keeps it.
    let (status, beta) = flow::post_json(
        &daemon,
        "/projects",
        serde_json::json!({ "name": "Beta", "members": ["/srv/a2"] }),
    )
    .await;
    assert_eq!(status, 201, "{beta}");
    assert_eq!(beta["members"], serde_json::json!(["/srv/a2"]));
    let alpha = daemon
        .get_json(&format!("/projects/{}", alpha["id"].as_str().unwrap()))
        .await;
    assert_eq!(alpha["members"], serde_json::json!(["/srv/a1", "/srv/a2"]));
    assert_eq!(
        alpha["primary_repo"], "/srv/a1",
        "the first member by default"
    );
}

#[tokio::test]
async fn project_provisioning_rules_round_trip_and_need_an_existing_project() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let project = create_project(&daemon, "Alpha").await;
    let id = project["id"].as_str().unwrap();

    let empty = daemon
        .get_json(&format!("/projects/{id}/provisioning"))
        .await;
    assert_eq!(
        empty,
        serde_json::json!({ "copy": [], "hardlink": [], "symlink": [] })
    );

    let rules = serde_json::json!({ "copy": [".env"], "hardlink": ["data/*.db"], "symlink": [] });
    let resp = reqwest::Client::new()
        .put(format!("{}/projects/{id}/provisioning", daemon.url()))
        .json(&rules)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.json::<serde_json::Value>().await.unwrap(), rules);
    assert_eq!(
        daemon
            .get_json(&format!("/projects/{id}/provisioning"))
            .await,
        rules
    );
    // The instance tier is a different scope, untouched.
    assert_eq!(
        daemon.get_json("/settings/provisioning").await,
        serde_json::json!({ "copy": [], "hardlink": [], "symlink": [] })
    );

    let resp = reqwest::Client::new()
        .put(format!(
            "{}/projects/no-such-project/provisioning",
            daemon.url()
        ))
        .json(&rules)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
    let body: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(body["error"], "project not found");
}

#[tokio::test]
async fn the_trailing_slash_page_listing_is_the_page_listing() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    assert_eq!(daemon.get_json("/pages/").await, serde_json::json!([]));

    let pages = tempfile::tempdir().unwrap();
    std::fs::write(pages.path().join("index.html"), "<h1>Home</h1>").unwrap();
    let (status, body) = flow::post_json(
        &daemon,
        "/pages",
        serde_json::json!({ "name": "report", "directory": pages.path() }),
    )
    .await;
    assert!(status == 200 || status == 201, "{status} {body}");

    let with_slash = daemon.get_json("/pages/").await;
    assert_eq!(with_slash, daemon.get_json("/pages").await);
    assert_eq!(with_slash.as_array().unwrap().len(), 1, "{with_slash}");
    assert_eq!(with_slash[0]["name"], "report");
}
