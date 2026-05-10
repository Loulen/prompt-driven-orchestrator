# Maestro Runtime Preamble

You are node `9NOnrpKY` in pipeline `simple-bugfix`, iteration 1.

## Inputs

- `in`: read `/home/llenoir/Documents/perso/Maestro/.maestro/runs/20260510-054214-d067310/worktree/.maestro/artifacts/KHFCO0US/iter-1/out.md`
- `steps`: read `/home/llenoir/Documents/perso/Maestro/.maestro/runs/20260510-054214-d067310/worktree/.maestro/artifacts/9LvO3oid/iter-1/how_to_reproduce.md`

## Outputs

- `out`: write to `/home/llenoir/Documents/perso/Maestro/.maestro/runs/20260510-054214-d067310/worktree/.maestro/artifacts/9NOnrpKY/iter-1/out.md`
  Required YAML frontmatter:
  - `Verdict`: enum (allowed: Pass, Fail)

## Completion

When you are done, signal completion by running:
```
maestro complete
```

If you cannot complete the task, signal failure:
```
maestro fail --reason "<description of the problem>"
```

---

  Validate the implementation by following these steps
   in order.

  ## 1. Build the project

  Confirm the changes compile cleanly:
  - Rust: `cargo build --workspace --all-targets`
  (build, no test execution)
  - Frontend: `cd frontend && npm run build` if the
  diff touched frontend code

  Treat any compile error as a failure.

  ## 2. Run the user's repro

  Follow the reproduction steps written by the
  Debugger upstream and verify
  the bug no longer happens in the running app:
  - Start (or attach to) the dev server: `npm run dev`
   from the project root
  - Open the app via chrome-devtools-mcp. If a stale
  browser window is open,
    close it (`close_page`) and start fresh
  (`new_page`) before driving the UI
  - Drive the UI exactly as the user described, take a
   screenshot at the
    decisive moment, and confirm the buggy behaviour
  is gone

  Visual confirmation via chrome MCP is the primary
  signal — describe exactly
  what you saw and attach the screenshot path.
  - Rust: `cargo build --workspace --all-targets` (build, no test execution)
  - Frontend: `cd frontend && npm run build` if the diff touched frontend code

  Treat any compile error as a failure.

  ## 2. Run the user's repro

  Follow the reproduction steps written by the Debugger upstream and verify
  the bug no longer happens in the running app:
  - Start (or attach to) the dev server: `npm run dev` from the project root
  - Open the app via chrome-devtools-mcp. If a stale browser window is open,
    close it (`close_page`) and start fresh (`new_page`) before driving the UI
  - Drive the UI exactly as the user described, take a screenshot at the
    decisive moment, and confirm the buggy behaviour is gone

  Visual confirmation via chrome MCP is the primary signal — describe exactly
  what you saw and attach the screenshot path.

  ## 3. Targeted unit tests (optional, only if the diff specifically warrants)

  If the diff touches isolated logic that has unit tests, you may run:
  - `cargo test --lib --package <crate-name>` for Rust unit tests
  - `npm test -- <pattern>` for vitest specs

  ⚠️ **DO NOT run `cargo test`, `cargo test --workspace`, or any command that
  runs Maestro's integration tests** (the `tests/*.rs` binaries in
  `crates/maestro-daemon/tests/`). Those tests spawn a `TestDaemon` whose
  boot-time orphan sweep scans all `maestro-*` tmux sessions and kills the
  ones it doesn't recognise — including this very pipeline's own manager
  and worker sessions. Running them WILL crash the run mid-validation.

  ## Output

  Write `.maestro/artifacts/<this-node>/iter-<N>/out.md` with:
  - Frontmatter: `Verdict: Pass` if the bug is fixed and build is clean,
    otherwise `Verdict: Fail`
  - Body: what you did, what you observed (screenshot path, key log lines),
    and one paragraph of justification for the verdict