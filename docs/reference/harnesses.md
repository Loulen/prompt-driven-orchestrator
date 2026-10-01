# Harness support

A harness is the program that runs the agent inside a node (`claude`, `opencode`, `copilot`,
`pi`, …). PDO launches it, attaches to it and kills it; it never ships or provides one. Every
first-party harness can launch, attach, resume and complete a node. Everything beyond that is a
capability written harness by harness, and the table below says which one has which.

## Support table

<!-- support-table:begin -->
<!-- Generated from crates/pdo-daemon/src/harness_probes.rs. Do not edit by hand: run `make support-table`. `make check` fails if this block has drifted. -->

PDO can launch, attach, resume, and complete nodes with every built-in harness.

| Capability | What PDO does with it | `claude` 2.1.246 | `opencode` 1.18.18 | `copilot` 1.0.80 | `pi` 0.85.1 | `vibe` 2.25.8 |
| --- | --- | --- | --- | --- | --- | --- |
| **Cost** | Show the Run cost | ✅ derived: per-message token usage × the price table | ❌ | ✅ reported: the harness's own billing unit × a published constant | ✅ reported: the harness's own billing unit × a published constant | ✅ reported: the harness's own billing unit × a published constant |
| **Transcript** | Find the session transcript | ✅ the JSONL transcript, keyed by working directory | ❌ | ✅ the event journal, keyed by the session identity PDO imposed | ✅ the session JSONL in the working directory's folder, keyed by the session identity PDO imposed | ✅ the session folder's meta.json under the vibe store, keyed by the session identity PDO learned (first session of the working directory after the spawn) |
| **Observed model & effort** | Show the model and effort the execution actually ran on (Stats › Cost › By model) | ✅ observed: the model per message; the effort stays requested | ❌ | ✅ observed: the model and the effort, from the source | ✅ observed: the model and the effort, from the source | ✅ observed: the session's active model; the effort stays requested |
| **End of turn** | Complete a node when its turn ends | ✅ an injected `Stop` hook, plus the transcript tail as the sweep's fallback | ❌ | ✅ the journal's explicit `assistant.turn_end` event | ✅ an injected `agent_settled` extension, plus the session tail as the sweep's fallback | ✅ an injected `post_agent` hook (`.vibe/hooks.toml` in the worktree), plus the transcript tail as the sweep's fallback |
| **Usage-limit menu** | Detect the harness usage-limit menu | ✅ the interactive "wait for limit to reset" menu, matched in a pane capture | ❌ | ❌ | ❌ | ❌ |
| **Sandbox staging set** | Stage the harness home in a sandbox and disarm its blocking dialogs | ✅ the `.claude` home: credentials and org managed settings copied, trust and permissions bypass fixed up, transcripts harvested back | ❌ | ❌ | ✅ the `.pi/agent` home: auth, settings, model catalogue, extensions, skills, prompts, themes and bin copied, sessions harvested back | ✅ the `.vibe` home: key, settings, catalogue, trust, skills, agents and hooks copied (logs and typed history left out), sessions harvested back |
| **Context usage** | Show peak context-window usage | ✅ derived: per-turn token usage from the transcript, deduplicated and maxed | ❌ | ✅ derived: the journal's cumulative usage counters, converted to a per-turn contribution and maxed | ✅ derived: per-message `usage.totalTokens` from the session, deduplicated and maxed, read against the catalogue's context window | ✅ reported: the session's `stats.context_tokens`, an absolute figure (vibe publishes no context window per model) |
| **Steering** | Count the steering messages a human typed per execution (Stats › Performance) | ✅ derived: typed user turns of the transcript, launch prompt and runtime messages excluded | ❌ | ✅ derived: the journal's `user.message` events, launch prompt and runtime messages excluded | ✅ derived: the session's user-role messages, launch prompt and runtime messages excluded | ✅ derived: the session's typed user messages (`injected: false`), launch prompt excluded |

Each header shows the last validated harness version; PDO does not enforce it. The sandbox image is not provided by PDO: it is the profile's image, and the harness binary must already be in it (ADR-0063).

Custom descriptors in `~/.pdo/harnesses/descriptors.yaml` can launch, attach, resume, and complete nodes through `pdo complete`.

<!-- support-table:end -->

## Prerequisites

| Requirement | Setup |
| --- | --- |
| Authentication | Log in with each harness before using it through PDO |
| An approved working directory | Trust the target repository root once; trust cascades to subdirectories |
| An installed version | Compare your installed harness with the last validated version in the support table |
| A model catalogue for `pi` | Keep pi's model catalogue reachable from its home (`~/.pi/agent`): pi prices each message from it, and a message it cannot price makes the node's cost read "—" rather than `$0` |
| `pi` in the sandbox image | PDO does not provide the sandbox image. For a sandboxed `pi` node your image must contain the `pi` binary: PDO checks it with a `which` at spawn, and a node whose binary is missing goes `Interrupted` with that reason (ADR-0063) |
| `vibe`'s key | Mistral Vibe reads its API key from `~/.vibe/.env` (`vibe --setup`) or the provider's env var; PDO never sets it. In a sandbox the `.env` travels with the staged `.vibe` home, like pi's `auth.json` (ADR-0063) |
| `vibe`'s model catalogue | What the picker offers for `vibe` is the `[[models]]` of its `~/.vibe/config.toml` (`$VIBE_HOME` honoured), re-read when the binary's version changes or the daemon restarts (ADR-0056). A `vibe` that was never launched has no file: the picker falls back to free text |
| `vibe` and an unknown model | Measured on 2.25.8: a model absent from its catalogue does **not** fail the node — `vibe` silently runs the **first model of its configuration file** (the most expensive one on a default install) and exits 0. PDO does not guard (ADR-0053 §4); it shows the **observed** model beside the requested one in Stats › Cost › By model (ADR-0065) |
| `vibe`'s session store | PDO learns a `vibe` node's session by working directory and spawn time (ADR-0080) under `session_logging.save_dir` of `config.toml` (default `~/.vibe/logs/session`). The descriptor pins `--legacy-harness`: the "Unified Harness" store (`logs/session/unified/`) is not read. An absolute `save_dir` outside the staged `.vibe` home is not harvested back from a sandbox |
| `vibe` in the sandbox image | Same rule as `pi`: your image must contain the `vibe` binary (`uv tool install mistral-vibe`); PDO checks it with a `which` at spawn (ADR-0063) |

Outside sandboxed runs, PDO does not stage any harness's home.

Inside a sandbox, the image and profile define the available harness configuration.

## See also

- [ADR-0045](../adr/0045-un-harnais-se-declare-par-un-template-d-argv-les-capacites-remplissent-les-trous.md): a harness is declared by an argv template; capabilities fill the gaps.
- [ADR-0051](../adr/0051-une-capacite-de-harnais-est-un-point-de-dispatch-pas-une-garde-de-presence.md): a harness capability is a dispatch point, not a presence guard.
- [CLI reference](cli.md), for `pdo docs support-table`.
