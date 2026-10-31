# Measuring the dev loop

How long a ticket waits on its builds and gates, and what it costs, measured the same way before
and after a change to the tooling. Two commands, both read-only for the repository: neither
touches your branch nor your `target/`.

## Time: `make bench-gates`

```bash
df -h /                                    # ~25 GB free per cold build
uptime                                     # the load, other Runs compile on the same machine
make bench-gates REF="40e82f49 integration/<story>" OUT=target/bench-gates/<story>.md
```

For each ref of `REF`, in order, the bench creates a throwaway worktree under the system temp
folder (`pdo-bench-<sha>-…`), then measures, from a fresh worktree as a Run starts:

| Measure | Command | At a ref without it |
| --- | --- | --- |
| Frontend deps | `pnpm install --frozen-lockfile` | - |
| First structural failure | a 401-line `crates/pdo/src/bench_oversized.rs`, then `make fast` (else `make test`), on the cold `target/` | `make test` |
| Cold build | `cargo clean`, `cargo build`, under a private sccache with an empty cache | - |
| Build with a warm sccache | `cargo clean`, `cargo build` again, the same cache now full | « n/a (sccache absent) » |
| Scoped gate, front-only diff | a comment appended to `frontend/src/main.tsx`, `make scoped BASE=HEAD` | « n/a » |
| Full suite | `make gate`, else `make test` | `make test` |

The structural failure counts as **caught** only when the failing command names the injected file.
Each cell gives the duration, the outcome and the 1-minute load when the measure started; the
builds also give the sccache hits and misses. The table goes to `OUT=` (default
`target/bench-gates/bench-<date>.md`), the output of every measure to
`target/bench-gates/<ref>/<measure>.log`.

The worktree is removed when the bench ends, fails, or is interrupted (Ctrl-C, `SIGTERM`), and so
is the private sccache (its own server port and folder, so the counters are this bench's only).
Before each measure the bench checks the free disk: below `MIN_FREE_GB` (25 by default) it skips
the measures left instead of filling the disk. The measures run without the caller's `PDO_*`,
`RUSTC_WRAPPER`, `SCCACHE_*`, `CARGO_TARGET_DIR` and `MAKEFLAGS`, so a ref's own Makefile decides
whether its gates use sccache.

Compare refs measured in **one** command, or at least at a similar load: durations double when
other Runs compile at the same time.

## Money: `make run-costs`

```bash
make run-costs RUN=<run-id> [URL=http://127.0.0.1:<port>] [OUT=<file>]
```

Reads `GET /runs/<id>` and `GET /runs/<id>/children` (recursively) on the daemon of `URL=`, else
`PDO_DAEMON_URL`, with `PDO_TOKEN` as the bearer when set; it writes nothing on the daemon. One
row per Run: the story's Run (its own nodes, the orchestration), then each child Run (a ticket),
with its status, duration (`started_at` to `completed_at`, or to now for a Run still going) and
`cost.usd`. Then the total and the mean and median per ticket, a ticket counting its own children.
A Run still going, or not fully costed, is marked partial. For a single Run in one line, `pdo run
status <run-id>` gives the same cost.

## Before / after a story of the dev tooling

1. Measure the time: `make bench-gates REF="<main before the story> <the story's integration branch>"`.
2. Measure the money: `make run-costs RUN=<the reference story's Run>`, then the same on the Run of
   the first story delivered with the new tooling, once it is done (a Run in progress is partial).
3. Publish both tables, with the load and the date, as a comment on the story.
