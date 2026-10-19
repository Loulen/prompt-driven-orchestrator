//! The bench of the search over a Run's transcripts (`make bench-transcript-search`),
//! ignored by the suite: 15 searches at once on a realistic Run, measured
//! against the daemon's everyday reads (the list of the Runs, the opening of a
//! node: its Run, its Transcript's first page).
//!
//! The Run: [`NODES`] agent nodes side by side, each with a Claude transcript
//! of about 2.5 MB (messages, tool calls, outputs of a few kB) where the
//! session's frozen launch says. Three phases, on the same daemon:
//!
//! 1. **idle** — the everyday reads alone, the reference.
//! 2. **first search** — one search, which indexes every transcript once, then
//!    one search at a time over the indexed transcripts.
//! 3. **load** — [`SEARCHERS`] clients each searching in a loop (a new word
//!    each time, as one types) for [`LOAD`], the everyday reads running beside
//!    them.
//!
//! It prints the latencies (p50, p95, max) and the CPU the process spent, and
//! fails when the everyday reads under load leave the budget: their p95 at
//! most [`BUDGET_FACTOR`] times the idle one, plus [`BUDGET_SLACK`].

use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::common::TestDaemon;

const NODES: usize = 8;
const LINES_PER_NODE: usize = 3_000;
const SEARCHERS: usize = 15;
const LOAD: Duration = Duration::from_secs(10);
const BUDGET_FACTOR: f64 = 3.0;
const BUDGET_SLACK: Duration = Duration::from_millis(50);

const WORDS: [&str; 12] = [
    "debounce",
    "useSearch",
    "pagination",
    "error",
    "cargo test",
    "spec.md",
    "filter chips",
    "timeout",
    "retry",
    "worktree",
    "nothing-matches-this",
    "e",
];

fn pipeline() -> String {
    let mut yaml = String::from(
        "name: bench-search\nversion: \"1.0\"\nnodes:\n  - id: start\n    name: Start\n    type: start\n    outputs:\n      - name: user_prompt\n",
    );
    for i in 0..NODES {
        yaml.push_str(&format!(
            "  - id: n{i}\n    name: node {i}\n    type: agent\n    isolated_worktree: false\n    pin_harness: claude\n    inputs:\n      - name: task\n    outputs:\n      - name: out\n"
        ));
    }
    yaml.push_str(
        "  - id: end\n    name: End\n    type: end\n    inputs:\n      - name: result\nedges:\n",
    );
    for i in 0..NODES {
        yaml.push_str(&format!(
            "  - source: {{ node: start, port: user_prompt }}\n    target: {{ node: n{i}, port: task }}\n"
        ));
    }
    yaml
}

fn seed(repo: &std::path::Path) -> anyhow::Result<()> {
    let pipelines = repo.join(".pdo").join("pipelines");
    let prompts = pipelines.join("bench-search.prompts");
    std::fs::create_dir_all(&prompts)?;
    std::fs::write(pipelines.join("bench-search.yaml"), pipeline())?;
    for i in 0..NODES {
        std::fs::write(prompts.join(format!("n{i}.md")), "Work.\n")?;
    }
    let run = |args: &[&str]| -> anyhow::Result<()> {
        let out = Command::new("git").args(args).current_dir(repo).output()?;
        anyhow::ensure!(out.status.success(), "git {args:?} failed");
        Ok(())
    };
    run(&["init", "-q", "-b", "main"])?;
    run(&["config", "user.email", "t@example.com"])?;
    run(&["config", "user.name", "Test"])?;
    run(&["config", "commit.gpgsign", "false"])?;
    std::fs::write(repo.join(".gitignore"), ".pdo/runs/\n")?;
    run(&["add", "."])?;
    run(&["commit", "-q", "-m", "init"])?;
    Ok(())
}

/// A session of `lines` lines: a prompt, then agent replies, tool calls and
/// their outputs, turn after turn.
fn transcript(node: usize, lines: usize) -> String {
    let mut text = String::new();
    for i in 0..lines {
        let ts = format!("2026-10-19T10:{:02}:{:02}Z", (i / 60) % 60, i % 60);
        let line = match i % 4 {
            0 => serde_json::json!({
                "type": "user", "uuid": format!("u{node}-{i}"), "timestamp": ts,
                "message": {"role": "user", "content": format!(
                    "Turn {i} of node {node}: keep the pagination of useSearch, fix the error {i} and run cargo test."
                )}
            }),
            1 => serde_json::json!({
                "type": "assistant", "uuid": format!("a{node}-{i}"), "timestamp": ts,
                "message": {"role": "assistant", "content": [
                    {"type": "thinking", "thinking": "t".repeat(300)},
                    {"type": "text", "text": format!(
                        "The spec asks for a debounce on the input ({i}). I will wrap useSearch in the worktree and retry on timeout. {}",
                        "Lorem ipsum dolor sit amet, consectetur adipiscing elit. ".repeat(4)
                    )}
                ]}
            }),
            2 => serde_json::json!({
                "type": "assistant", "uuid": format!("t{node}-{i}"), "timestamp": ts,
                "message": {"role": "assistant", "content": [
                    {"type": "tool_use", "id": format!("call{i}"), "name": "Bash",
                     "input": {"command": format!("cargo test -p search filter_chips_{i}")}}
                ]}
            }),
            _ => serde_json::json!({
                "type": "user", "uuid": format!("r{node}-{i}"), "timestamp": ts,
                "message": {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": format!("call{}", i - 1),
                     "content": "test result: ok. 12 passed\n".repeat(110)}
                ]}
            }),
        };
        text.push_str(&line.to_string());
        text.push('\n');
    }
    text
}

fn plant(daemon: &TestDaemon, run_id: &str, session_id: &str, text: &str) {
    let worktree = daemon
        .repo_root()
        .join(".pdo/runs")
        .join(run_id)
        .join("worktree");
    let enc = pdo_harness::claude_transcript::encode_working_dir(&worktree);
    let dir = daemon.repo_root().join(".claude/projects").join(enc);
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join(format!("{session_id}.jsonl")), text).unwrap();
}

/// The CPU this process spent so far (user + system), the daemon included.
fn cpu_time() -> Duration {
    let stat = std::fs::read_to_string("/proc/self/stat").unwrap_or_default();
    let after = stat.rsplit_once(')').map(|(_, rest)| rest).unwrap_or("");
    let fields: Vec<&str> = after.split_whitespace().collect();
    let ticks = |i: usize| {
        fields
            .get(i)
            .and_then(|f| f.parse::<u64>().ok())
            .unwrap_or(0)
    };
    // utime and stime are the 14th and 15th fields; `after` starts at the 3rd.
    let total = ticks(11) + ticks(12);
    Duration::from_millis(total * 10)
}

#[derive(Default)]
struct Sample(Vec<Duration>);

impl Sample {
    fn pct(&self, p: f64) -> Duration {
        let mut v = self.0.clone();
        v.sort();
        if v.is_empty() {
            return Duration::ZERO;
        }
        v[((v.len() - 1) as f64 * p).round() as usize]
    }
    fn line(&self, label: &str) -> String {
        format!(
            "{label:<34} n={:<5} p50={:>8.1?} p95={:>8.1?} max={:>8.1?}",
            self.0.len(),
            self.pct(0.5),
            self.pct(0.95),
            self.pct(1.0)
        )
    }
}

async fn timed_get(client: &reqwest::Client, url: &str) -> Duration {
    let started = Instant::now();
    let resp = client.get(url).send().await.unwrap();
    assert!(resp.status().is_success(), "GET {url}: {}", resp.status());
    resp.bytes().await.unwrap();
    started.elapsed()
}

/// The everyday reads, once each: the list of the Runs, then opening a node.
async fn everyday(
    client: &reqwest::Client,
    base: &str,
    run_id: &str,
    node: usize,
) -> [Duration; 3] {
    [
        timed_get(client, &format!("{base}/runs")).await,
        timed_get(client, &format!("{base}/runs/{run_id}")).await,
        timed_get(
            client,
            &format!("{base}/runs/{run_id}/nodes/n{node}/transcript?iter=1&after=0&limit=50"),
        )
        .await,
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "a bench: `make bench-transcript-search`"]
async fn bench_fifteen_concurrent_searches() {
    let daemon = TestDaemon::spawn_with_home_override(seed, Some("exec sleep 600".to_string()))
        .await
        .unwrap();
    let base = daemon.url();
    let client = reqwest::Client::new();
    let resp = client
        .post(format!("{base}/runs"))
        .json(&serde_json::json!({
            "pipeline": "bench-search", "input": "go", "target_repo": daemon.target_repo(),
        }))
        .send()
        .await
        .unwrap();
    assert_eq!(resp.status(), 201);
    let run_id = resp.json::<serde_json::Value>().await.unwrap()["run_id"]
        .as_str()
        .unwrap()
        .to_string();
    let mut bytes = 0;
    for i in 0..NODES {
        let node = format!("n{i}");
        let mut sid = None;
        for _ in 0..200 {
            let evs: serde_json::Value = client
                .get(format!("{base}/runs/{run_id}/events"))
                .send()
                .await
                .unwrap()
                .json()
                .await
                .unwrap();
            sid = evs.as_array().unwrap().iter().rev().find_map(|e| {
                (e["kind"] == "node_started" && e["node_id"] == node.as_str())
                    .then(|| e["payload"]["session_id"].as_str().map(str::to_string))
                    .flatten()
            });
            if sid.is_some() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        let text = transcript(i, LINES_PER_NODE);
        bytes += text.len();
        plant(&daemon, &run_id, &sid.expect("the node started"), &text);
    }

    // 1. idle
    let (mut list, mut run, mut open) = (Sample::default(), Sample::default(), Sample::default());
    for round in 0..3 {
        for i in 0..NODES {
            let [l, r, o] = everyday(&client, &base, &run_id, i).await;
            if round > 0 {
                list.0.push(l);
                run.0.push(r);
                open.0.push(o);
            }
        }
    }

    // 2. the first search indexes every transcript
    let started = Instant::now();
    let first: serde_json::Value = client
        .get(format!("{base}/runs/{run_id}/transcripts/search"))
        .query(&[("q", "debounce"), ("client", "first")])
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let first_latency = started.elapsed();
    assert_eq!(first["nodes"].as_array().unwrap().len(), NODES);

    // 2b. one search at a time, the transcripts indexed: what one person waits
    let mut alone = Sample::default();
    for word in WORDS {
        let started = Instant::now();
        client
            .get(format!("{base}/runs/{run_id}/transcripts/search"))
            .query(&[("q", word), ("client", "alone")])
            .send()
            .await
            .unwrap()
            .bytes()
            .await
            .unwrap();
        alone.0.push(started.elapsed());
    }

    // 3. load
    let stop = Arc::new(AtomicBool::new(false));
    let cpu_before = cpu_time();
    let load_started = Instant::now();
    let searchers: Vec<_> = (0..SEARCHERS)
        .map(|s| {
            let (client, base, run_id, stop) =
                (client.clone(), base.clone(), run_id.clone(), stop.clone());
            tokio::spawn(async move {
                let mut sample = Sample::default();
                let mut k = s;
                while !stop.load(Ordering::Relaxed) {
                    let word = WORDS[k % WORDS.len()];
                    k += 1;
                    let started = Instant::now();
                    let resp = client
                        .get(format!("{base}/runs/{run_id}/transcripts/search"))
                        .query(&[("q", word), ("client", &format!("bench-{s}"))])
                        .send()
                        .await
                        .unwrap();
                    assert!(resp.status().is_success());
                    resp.bytes().await.unwrap();
                    sample.0.push(started.elapsed());
                }
                sample
            })
        })
        .collect();
    let (mut list_l, mut run_l, mut open_l) =
        (Sample::default(), Sample::default(), Sample::default());
    let mut i = 0;
    while load_started.elapsed() < LOAD {
        let [l, r, o] = everyday(&client, &base, &run_id, i % NODES).await;
        list_l.0.push(l);
        run_l.0.push(r);
        open_l.0.push(o);
        i += 1;
    }
    stop.store(true, Ordering::Relaxed);
    let mut searches = Sample::default();
    for s in searchers {
        searches.0.extend(s.await.unwrap().0);
    }
    let wall = load_started.elapsed();
    let cpu = cpu_time().saturating_sub(cpu_before);

    let report = [
        format!(
            "Run: {NODES} agent nodes, {:.1} MB of transcripts ({LINES_PER_NODE} lines each); {SEARCHERS} concurrent searchers for {LOAD:?}",
            bytes as f64 / 1e6
        ),
        format!("first search (indexes every transcript)    {first_latency:>8.1?}"),
        alone.line("idle   search (one at a time)"),
        list.line("idle   GET /runs"),
        run.line("idle   GET /runs/{id}"),
        open.line("idle   GET node transcript (1st page)"),
        list_l.line("load   GET /runs"),
        run_l.line("load   GET /runs/{id}"),
        open_l.line("load   GET node transcript (1st page)"),
        searches.line("load   search (each)"),
        format!(
            "load   searches/s {:.1}; CPU {:.2?} over {:.2?} wall = {:.2} cores (of {})",
            searches.0.len() as f64 / wall.as_secs_f64(),
            cpu,
            wall,
            cpu.as_secs_f64() / wall.as_secs_f64(),
            std::thread::available_parallelism().map(|n| n.get()).unwrap_or(0)
        ),
    ];
    for line in &report {
        eprintln!("{line}");
    }

    for (label, idle, load) in [
        ("GET /runs", &list, &list_l),
        ("GET /runs/{id}", &run, &run_l),
        ("GET node transcript", &open, &open_l),
    ] {
        let budget = idle.pct(0.95).mul_f64(BUDGET_FACTOR) + BUDGET_SLACK;
        assert!(
            load.pct(0.95) <= budget,
            "{label}: p95 under load {:?} is past the budget {budget:?}",
            load.pct(0.95)
        );
    }
}
