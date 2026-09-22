# Harness support

A harness is the program that runs the agent inside a node (`claude`, `opencode`, `copilot`,
`pi`, …). PDO launches it, attaches to it and kills it; it never ships or provides one. Every
first-party harness can launch, attach, resume and complete a node. Everything beyond that is a
capability written harness by harness, and the table below says which one has which.

## Support table

<!-- support-table:begin -->
<!-- Generated from crates/pdo-daemon/src/harness_probes.rs. Do not edit by hand: run `make support-table`. `make check` fails if this block has drifted. -->

PDO can launch, attach, resume, and complete nodes with every built-in harness.

| Capability | What PDO does with it | `claude` 2.1.246 | `opencode` 1.18.18 | `copilot` 1.0.80 | `pi` 0.85.1 |
| --- | --- | --- | --- | --- | --- |
| **Cost** | Show the Run cost | ✅ derived: per-message token usage × the price table | ❌ | ✅ reported: the harness's own billing unit × a published constant | ✅ reported: the harness's own billing unit × a published constant |
| **Transcript** | Find the session transcript | ✅ the JSONL transcript, keyed by working directory | ❌ | ✅ the event journal, keyed by the session identity PDO imposed | ✅ the session JSONL in the working directory's folder, keyed by the session identity PDO imposed |
| **Observed model & effort** | Show the model and effort the execution actually ran on (Stats › Cost › By model) | ✅ observed: the model per message; the effort stays requested | ❌ | ✅ observed: the model and the effort, from the source | ✅ observed: the model and the effort, from the source |
| **End of turn** | Complete a node when its turn ends | ✅ an injected `Stop` hook, plus the transcript tail as the sweep's fallback | ❌ | ✅ the journal's explicit `assistant.turn_end` event | ✅ an injected `agent_settled` extension, plus the session tail as the sweep's fallback |
| **Usage-limit menu** | Detect the harness usage-limit menu | ✅ the interactive "wait for limit to reset" menu, matched in a pane capture | ❌ | ❌ | ❌ |
| **Sandbox staging set** | Stage the harness home in a sandbox and disarm its blocking dialogs | ✅ the `.claude` home: credentials and org managed settings copied, trust and permissions bypass fixed up, transcripts harvested back | ❌ | ❌ | ✅ the `.pi/agent` home: auth, settings, model catalogue, extensions, skills, prompts, themes and bin copied, sessions harvested back |
| **Context usage** | Show peak context-window usage | ✅ derived: per-turn token usage from the transcript, deduplicated and maxed | ❌ | ✅ derived: the journal's cumulative usage counters, converted to a per-turn contribution and maxed | ✅ derived: per-message `usage.totalTokens` from the session, deduplicated and maxed, read against the catalogue's context window |
| **Steering** | Count the steering messages a human typed per execution (Stats › Performance) | ✅ derived: typed user turns of the transcript, launch prompt and runtime messages excluded | ❌ | ✅ derived: the journal's `user.message` events, launch prompt and runtime messages excluded | ✅ derived: the session's user-role messages, launch prompt and runtime messages excluded |

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

Outside sandboxed runs, PDO does not stage any harness's home.

Inside a sandbox, the image and profile define the available harness configuration.

## See also

- [ADR-0045](../adr/0045-un-harnais-se-declare-par-un-template-d-argv-les-capacites-remplissent-les-trous.md): a harness is declared by an argv template; capabilities fill the gaps.
- [ADR-0051](../adr/0051-une-capacite-de-harnais-est-un-point-de-dispatch-pas-une-garde-de-presence.md): a harness capability is a dispatch point, not a presence guard.
- [CLI reference](cli.md), for `pdo docs support-table`.
