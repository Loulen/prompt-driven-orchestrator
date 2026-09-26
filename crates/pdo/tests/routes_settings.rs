//! Layer 4 (ADR-0080) — the settings routes no integration test called before the
//! refactor net (#914): the agent profiles CRUD and their referents (ADR-0057),
//! the instance provisioning rules, and the cancel of an in-flight skill scan.

use std::path::Path;
use std::time::Duration;

use crate::common::flow;
use crate::common::TestDaemon;

fn seed(repo: &Path) -> anyhow::Result<()> {
    flow::seed_solo(repo, &[("solo", false)])
}

async fn send(
    daemon: &TestDaemon,
    method: reqwest::Method,
    path: &str,
    body: Option<serde_json::Value>,
) -> (u16, serde_json::Value) {
    let mut req = reqwest::Client::new().request(method, format!("{}{path}", daemon.url()));
    if let Some(body) = body {
        req = req.json(&body);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap_or_default();
    (
        status,
        serde_json::from_str(&text).unwrap_or(serde_json::Value::Null),
    )
}

// --- /settings/agent-profiles ------------------------------------------------

async fn create_profile(daemon: &TestDaemon, body: serde_json::Value) -> (u16, serde_json::Value) {
    flow::post_json(daemon, "/settings/agent-profiles", body).await
}

#[tokio::test]
async fn agent_profiles_are_listed_created_read_updated_and_deleted() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let list = daemon.get_json("/settings/agent-profiles").await;
    let profiles = list["profiles"].as_array().unwrap();
    assert_eq!(profiles.len(), 1, "only the reserved floor: {list}");
    assert_eq!(profiles[0]["id"], "default");
    assert_eq!(profiles[0]["name"], "Default");
    assert_eq!(profiles[0]["harness"], "claude");

    let (status, created) = create_profile(
        &daemon,
        serde_json::json!({ "name": "Deep review", "harness": "claude", "model": "opus", "effort": "high" }),
    )
    .await;
    assert_eq!(status, 201, "{created}");
    let id = created["id"].as_str().unwrap().to_string();
    assert_ne!(id, "default");
    assert_eq!(created["name"], "Deep review");
    assert_eq!(created["model"], "opus");
    assert_eq!(created["effort"], "high");
    assert!(created["created_at"].is_string());

    let fetched = daemon
        .get_json(&format!("/settings/agent-profiles/{id}"))
        .await;
    assert_eq!(fetched, created);

    let resp = reqwest::Client::new()
        .put(format!("{}/settings/agent-profiles/{id}", daemon.url()))
        .json(&serde_json::json!({ "name": "Quick review", "harness": "claude", "effort": "low" }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    let updated: serde_json::Value = resp.json().await.unwrap();
    assert_eq!(updated["id"], id.as_str());
    assert_eq!(updated["name"], "Quick review");
    assert!(updated.get("model").is_none(), "cleared: {updated}");
    assert_eq!(updated["effort"], "low");

    let list = daemon.get_json("/settings/agent-profiles").await;
    assert_eq!(list["profiles"].as_array().unwrap().len(), 2, "{list}");

    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/agent-profiles/{id}", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 204);
    let resp = reqwest::get(format!("{}/settings/agent-profiles/{id}", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);
}

#[tokio::test]
async fn agent_profiles_refuse_blank_duplicate_unknown_and_the_reserved_floor() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let (status, body) = create_profile(
        &daemon,
        serde_json::json!({ "name": " ", "harness": "claude" }),
    )
    .await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "a profile name cannot be blank");
    let (status, body) =
        create_profile(&daemon, serde_json::json!({ "name": "X", "harness": " " })).await;
    assert_eq!(status, 400, "{body}");
    assert_eq!(body["error"], "a profile's harness cannot be blank");
    let (status, body) = create_profile(
        &daemon,
        serde_json::json!({ "name": "default", "harness": "claude" }),
    )
    .await;
    assert_eq!(status, 409, "names are unique case-insensitively: {body}");

    let (status, _) = send(
        &daemon,
        reqwest::Method::PUT,
        "/settings/agent-profiles/nope",
        Some(serde_json::json!({ "name": "Nope", "harness": "claude" })),
    )
    .await;
    assert_eq!(status, 404);
    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/agent-profiles/nope", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 404);

    let resp = reqwest::Client::new()
        .delete(format!("{}/settings/agent-profiles/default", daemon.url()))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 400, "the floor is undeletable");
    let resp = reqwest::get(format!("{}/settings/agent-profiles/default", daemon.url()))
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
}

#[tokio::test]
async fn agent_profile_referents_name_every_tier_that_points_at_it() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (_, profile) = create_profile(
        &daemon,
        serde_json::json!({ "name": "Shared", "harness": "claude" }),
    )
    .await;
    let id = profile["id"].as_str().unwrap().to_string();
    let choice = serde_json::json!({ "mode": "profile", "profile_id": id });

    let referents = daemon
        .get_json(&format!("/settings/agent-profiles/{id}/referents"))
        .await;
    assert_eq!(
        referents,
        serde_json::json!({
            "profile_id": id,
            "instance": false,
            "projects": [],
            "triggers": [],
            "pipelines": [],
            "runs": [],
        })
    );

    // Instance, Projet, Trigger and a pipeline node all name the profile.
    let (status, _) = send(
        &daemon,
        reqwest::Method::PUT,
        "/settings",
        Some(serde_json::json!({ "agent_choice": choice })),
    )
    .await;
    assert_eq!(status, 200);
    let (status, project) =
        flow::post_json(&daemon, "/projects", serde_json::json!({ "name": "Team" })).await;
    assert_eq!(status, 201);
    let project_id = project["id"].as_str().unwrap().to_string();
    let (status, _) = send(
        &daemon,
        reqwest::Method::PATCH,
        &format!("/projects/{project_id}"),
        Some(serde_json::json!({ "agent_choice": choice })),
    )
    .await;
    assert_eq!(status, 200);
    let (status, trigger) = flow::post_json(
        &daemon,
        "/triggers",
        serde_json::json!({
            "name": "nightly",
            "pipeline_id": "solo",
            "cron": "0 3 * * *",
            "input_template": "go",
            "target_repo": daemon.target_repo(),
            "agent_choice": choice,
        }),
    )
    .await;
    assert_eq!(status, 201, "{trigger}");
    let yaml = flow::solo_pipeline_yaml("profiled", false).replace(
        "    interactive: false\n",
        &format!(
            "    interactive: false\n    agent_choice: {{ mode: profile, profile_id: {id} }}\n"
        ),
    );
    flow::write_pipeline(daemon.repo_root(), "profiled", &yaml).unwrap();

    let referents = daemon
        .get_json(&format!("/settings/agent-profiles/{id}/referents"))
        .await;
    assert_eq!(referents["instance"], true, "{referents}");
    assert_eq!(
        referents["projects"],
        serde_json::json!([{ "id": project_id, "name": "Team" }])
    );
    assert_eq!(referents["triggers"][0]["id"], trigger["id"]);
    assert_eq!(referents["triggers"][0]["pipeline_id"], "solo");
    assert_eq!(
        referents["pipelines"],
        serde_json::json!([{
            "id": "profiled",
            "name": "profiled",
            "node_id": "worker",
            "scope": "instance",
        }])
    );

    let resp = reqwest::get(format!(
        "{}/settings/agent-profiles/nope/referents",
        daemon.url()
    ))
    .await
    .unwrap();
    assert_eq!(resp.status(), 404);
}

// --- /settings/provisioning -------------------------------------------------------

#[tokio::test]
async fn instance_provisioning_rules_round_trip() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();

    let empty = daemon.get_json("/settings/provisioning").await;
    assert_eq!(
        empty,
        serde_json::json!({ "copy": [], "hardlink": [], "symlink": [] })
    );

    let rules = serde_json::json!({
        "copy": [".env", "config/*.local"],
        "hardlink": [],
        "symlink": ["node_modules"],
    });
    let resp = reqwest::Client::new()
        .put(format!("{}/settings/provisioning", daemon.url()))
        .json(&rules)
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(resp.json::<serde_json::Value>().await.unwrap(), rules);
    assert_eq!(daemon.get_json("/settings/provisioning").await, rules);

    // Omitted lists default to empty: a PUT replaces the whole set.
    let resp = reqwest::Client::new()
        .put(format!("{}/settings/provisioning", daemon.url()))
        .json(&serde_json::json!({ "hardlink": ["big.bin"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 200);
    assert_eq!(
        daemon.get_json("/settings/provisioning").await,
        serde_json::json!({ "copy": [], "hardlink": ["big.bin"], "symlink": [] })
    );
}

// --- POST /settings/skills/scan/{scan_id}/cancel -----------------------------------

#[tokio::test]
async fn cancelling_an_unknown_scan_is_a_calm_false() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let (status, body) = flow::post_json(
        &daemon,
        "/settings/skills/scan/never-started/cancel",
        serde_json::json!({}),
    )
    .await;
    assert_eq!(status, 200);
    assert_eq!(body, serde_json::json!({ "cancelled": false }));
}

/// A scan whose clone hangs (an HTTP remote that accepts and never answers) is
/// in flight until the cancel kills it; the scan request then answers
/// `cancelled`.
#[tokio::test]
async fn cancelling_an_in_flight_scan_kills_the_clone() {
    let daemon = TestDaemon::spawn(seed).await.unwrap();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let remote = format!("http://{}/team/skills.git", listener.local_addr().unwrap());
    let _hold = tokio::spawn(async move {
        let mut held = Vec::new();
        while let Ok((socket, _)) = listener.accept().await {
            held.push(socket);
        }
    });

    let scan_id = format!("net-914-{}", uuid::Uuid::new_v4().simple());
    let scan = {
        let url = daemon.url();
        let scan_id = scan_id.clone();
        tokio::spawn(async move {
            let resp = reqwest::Client::new()
                .post(format!("{url}/settings/skills/scan"))
                .json(&serde_json::json!({ "scan_id": scan_id, "source": remote }))
                .send()
                .await
                .unwrap();
            let status = resp.status().as_u16();
            (status, resp.json::<serde_json::Value>().await.unwrap())
        })
    };

    let cancel = format!("/settings/skills/scan/{scan_id}/cancel");
    let deadline = std::time::Instant::now() + Duration::from_secs(10);
    loop {
        let (status, body) = flow::post_json(&daemon, &cancel, serde_json::json!({})).await;
        assert_eq!(status, 200);
        if body["cancelled"] == true {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "the scan never went in flight"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }

    let (status, body) = tokio::time::timeout(Duration::from_secs(10), scan)
        .await
        .expect("the cancelled scan answers")
        .unwrap();
    assert_eq!(status, 499, "{body}");
    assert_eq!(body["code"], "cancelled", "{body}");

    let (_, body) = flow::post_json(&daemon, &cancel, serde_json::json!({})).await;
    assert_eq!(body["cancelled"], false, "nothing left in flight");
}
