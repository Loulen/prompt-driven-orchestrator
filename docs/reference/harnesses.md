# Harness support

A harness is the program that runs the agent inside a node (`claude`, `opencode`, `copilot`,
`pi`, …). PDO launches it, attaches to it and kills it; it never ships or provides one. Every
first-party harness can launch, attach, resume and complete a node. Everything beyond that is a
capability written harness by harness, and the table below says which one has which.

## Support table

<!-- support-table:begin -->
<!-- Generated from crates/pdo-harness/src/domain/probes/. Do not edit by hand: run `make support-table`. `make check` fails if this block has drifted. -->

PDO can launch, attach, resume, and complete nodes with every built-in harness.

| Capability | What PDO does with it | `claude` 2.1.246 | `opencode` 1.18.18 | `copilot` 1.0.80 | `pi` 0.85.1 |
| --- | --- | --- | --- | --- | --- |
| **Cost** | Show the Run cost | ✅ derived: per-message token usage × the price table | ❌ | ✅ reported: the harness's own billing unit × a published constant | ✅ reported: the harness's own billing unit × a published constant |
| **Transcript** | Find the session transcript | ✅ the JSONL transcript, keyed by working directory | ❌ | ✅ the event journal, keyed by the session identity PDO imposed | ✅ the session JSONL in the working directory's folder, keyed by the session identity PDO imposed |
| **Transcript tab** | Show the session's messages and folded tool calls in the node's Transcript tab | ✅ read: the transcript's user and assistant records, thinking dropped | ❌ | ✅ read: the journal's message and tool events, reasoning dropped | ✅ read: the session's user, assistant and tool-result messages, thinking dropped |
| **Observed model & effort** | Show the model and effort the execution actually ran on (Stats › Cost › By model) | ✅ observed: the model per message; the effort stays requested | ❌ | ✅ observed: the model and the effort, from the source | ✅ observed: the model and the effort, from the source |
| **End of turn** | Complete a node when its turn ends | ✅ an injected `Stop` hook, plus the transcript tail as the sweep's fallback | ❌ | ✅ the journal's explicit `assistant.turn_end` event | ✅ an injected `agent_settled` extension, plus the session tail as the sweep's fallback |
| **Usage-limit menu** | Detect the harness usage-limit menu | ✅ the interactive "wait for limit to reset" menu, matched in a pane capture | ❌ | ❌ | ❌ |
| **Sandbox staging set** | Stage the harness home in a sandbox and disarm its blocking dialogs. The home carries its key: see [Where a harness key lives](#where-a-harness-key-lives) | ✅ the `.claude` home: credentials and org managed settings copied, trust and permissions bypass fixed up, transcripts harvested back | ❌ | ❌ | ✅ the `.pi/agent` home: auth, settings, model catalogue, extensions, skills, prompts, themes and bin copied, sessions harvested back |
| **Context usage** | Show peak context-window usage | ✅ derived: per-turn token usage from the transcript, deduplicated and maxed | ❌ | ✅ derived: the journal's cumulative usage counters, converted to a per-turn contribution and maxed | ✅ derived: per-message `usage.totalTokens` from the session, deduplicated and maxed, read against the catalogue's context window |
| **Steering** | Count the steering messages a human typed per execution (Stats › Performance) | ✅ derived: typed user turns of the transcript, launch prompt and runtime messages excluded | ❌ | ✅ derived: the journal's `user.message` events, launch prompt and runtime messages excluded | ✅ derived: the session's user-role messages, launch prompt and runtime messages excluded |

Each header shows the last validated harness version; PDO does not enforce it. The sandbox image is not provided by PDO: it is the profile's image, and the harness binary must already be in it (ADR-0063).

Custom descriptors in `~/.pdo/harnesses/descriptors.yaml` can launch, attach, resume, and complete nodes through `pdo complete`.

<!-- support-table:end -->

## Prerequisites

| Requirement | Setup |
| --- | --- |
| Authentication | Log in with each harness before using it through PDO |
| An approved working directory | Trust the target repository root once; trust cascades to subdirectories. The autonomy flags PDO passes (`copilot`'s `--allow-all`, for one) cover tools, paths and URLs, not the « trust this folder » dialog. Until the root is trusted, a session on the host opens on its harness's folder-trust prompt (`claude`'s « Quick safety check »): a node waits in its terminal, alive and mute, and the Pipeline Manager a review « Send to manager » starts holds the comments and says so on their thread, with the way out (answer the prompt in the Run's Manager tab). PDO never answers that prompt for you and never writes the trust into your home; a sandboxed Run trusts its root in the sandbox's own staged home |
| An installed version | Compare your installed harness with the last validated version in the support table |
| A model catalogue for `pi` | Keep pi's model catalogue reachable from its home (`~/.pi/agent`): pi prices each message from it, and a message it cannot price makes the node's cost read "—" rather than `$0` |
| `pi` in the sandbox image | PDO does not provide the sandbox image. For a sandboxed `pi` node your image must contain the `pi` binary: PDO checks it with a `which` at spawn, and a node whose binary is missing goes `Interrupted` with that reason (ADR-0063) |

Outside sandboxed runs, PDO does not stage any harness's home.

## Where a harness key lives

PDO never supplies a key of its own: the harness looks for it in its home, in its
configuration, or in the environment of the session PDO launched. That environment holds the
daemon's variables plus the ones an extension adds for the session
([launch of every session](extension-api.md#the-launch-of-every-session-013)), so a key can belong
to **the person who launched the Run** rather than to the instance. Each harness reads its sources
in a fixed order and stops at the first one it finds.

**A personal key never goes in the shared home**: the `~/.claude/.credentials.json` or
`~/.pi/agent/auth.json` of the account the daemon runs as. Every session on the host reads that
home, and every sandboxed Run copies it (the *Sandbox staging set* row of the
[support table](#support-table)). A key stored there serves everyone. For `pi` it also outranks
the environment, so it silently replaces each person's key. A `/login` typed in a Run's terminal,
or a hand reconfiguration of the harness on the daemon's account, writes exactly there.

### `claude`

Claude Code picks the first credential present, in this order
([Authentication precedence](https://code.claude.com/docs/en/authentication#authentication-precedence)):

1. Cloud provider credentials, when `CLAUDE_CODE_USE_BEDROCK`, `CLAUDE_CODE_USE_VERTEX` or
   `CLAUDE_CODE_USE_FOUNDRY` is set.
2. `ANTHROPIC_AUTH_TOKEN` (a bearer token, for a gateway).
3. `ANTHROPIC_API_KEY`. An interactive session, which is how PDO runs `claude`, asks once whether
   to use the key and remembers the answer; until it is answered, the session waits on that
   prompt.
4. The output of the `apiKeyHelper` setting.
5. `CLAUDE_CODE_OAUTH_TOKEN` (a `claude setup-token` token).
6. Anthropic profile and federation credentials.
7. The `/login` credentials, in `.credentials.json` under `CLAUDE_CONFIG_DIR` (`~/.claude` by
   default).

`CLAUDE_CONFIG_DIR` moves the whole home: credentials, settings and transcripts. A session given
one logs in as that directory's account, and PDO reads its cost and liveness under
`$CLAUDE_CONFIG_DIR/projects/`. For a personal key, give the session `ANTHROPIC_API_KEY` (or
`ANTHROPIC_AUTH_TOKEN`) or a personal `CLAUDE_CONFIG_DIR`, never a `/login` on the shared home.

### `pi`

`pi` picks the first credential present, in this order (`docs/models.md` shipped with `pi`,
checked on 1.5.2):

1. `--api-key` on its command line. PDO never passes it.
2. A credential stored in `~/.pi/agent/auth.json`, written by `/login` or `pi auth`.
3. The provider's `apiKey` in `~/.pi/agent/models.json`.
4. The provider's environment variable (`OPENAI_API_KEY`, …) or ambient cloud credentials.

`auth.json` comes **before** the environment: as soon as it holds a key for a provider, every
`pi` session of the account uses that key, whatever its environment carries. Keep it empty (`{}`)
for a provider whose key is personal.

**The `!command` form.** The `apiKey` of `models.json` (and its header values), like the `key` of
an `auth.json` credential, takes a literal, a `$NAME` / `${NAME}` interpolation, or `!command`:
`pi` runs the command and uses its trimmed output. The two files do not run it the same way:

| Where | When the command runs |
| --- | --- |
| `apiKey` in `models.json` | at every request, never cached |
| `key` in `auth.json` | once per `pi` process, cached for its lifetime |

A `!command` in `models.json` therefore reads the environment of the session making the request:
in a Node, the variables given for the person who launched the Run. The same command in
`auth.json` would keep the first answer for the whole process. A per-person key through a gateway
fits in `models.json`, with an empty `auth.json`:

```json
{
  "providers": {
    "gateway": {
      "baseUrl": "https://gateway.example.com/v1",
      "api": "openai-completions",
      "apiKey": "!printenv GATEWAY_API_KEY || { echo 'GATEWAY_API_KEY is missing: create your key at https://gateway.example.com and store it as GATEWAY_API_KEY' >&2; exit 1; }",
      "models": [
        {
          "id": "some-model",
          "cost": { "input": 3, "output": 15, "cacheRead": 0.3, "cacheWrite": 3.75 }
        }
      ]
    }
  }
}
```

The other ways to write that `apiKey` each break something:

| `apiKey` | Effect |
| --- | --- |
| a literal key | outranks the environment: one shared key for everyone again |
| `$GATEWAY_API_KEY` | without the variable the model is unavailable, and the catalogue probe (**Refresh models**, `pdo settings refresh-harness pi`), which runs in the daemon's environment with nobody's key, stops listing it |
| `!printenv GATEWAY_API_KEY \|\| echo <placeholder>` | the catalogue keeps the model, but a session without the variable sends the placeholder: the gateway answers `401` with the key masked, and nothing says the key is missing |
| `!printenv GATEWAY_API_KEY \|\| { echo '<what to do>' >&2; exit 1; }` | the catalogue keeps the model, and a session without the variable fails with `Failed to resolve API key for provider "gateway" from shell command: <the command>` |

That last form is the one to use. `pi` discards the command's stderr but prints the whole command
in its error, so the instruction written in the command reaches the person who lacks the key. A
command that prints nothing, exits non-zero or still runs after 10 seconds leaves the key
unresolved.

**Pricing a custom model.** A model declared in `models.json` has no price unless it carries a
`cost` object: `input`, `output`, `cacheRead` and `cacheWrite`, in US dollars per million tokens.
With it, `pi` computes each message's `usage.cost.total`, which PDO reads for the Run cost (the
*Cost* row of the support table). Without it, the cost reads as unknown (`?` in
`pdo run costs`).

### In a sandboxed Run

The staging set copies the shared home into the sandbox: `.claude/.credentials.json` for
`claude`, and the whole `.pi/agent` for `pi`, `auth.json` and `models.json` included. A key stored
in that home reaches **every** sandboxed Run of the harness. The variables given to the session
are forwarded to the container, and a `!command` from `models.json` runs inside it, so the tools
it calls (`printenv` above) must be in the image.

## Model catalogue

The models and efforts a picker offers come from the installed binary: PDO reads them once,
then again only when the binary's `--version` changes. A binary can change what it offers
without changing its version. **Refresh models** probes the catalogue again anyway: in
Settings › Diagnostics › Harness descriptors (one row per harness, with its version, its number
of models and when they were probed), or with ↻ beside the model of an agentic profile. A refresh
asked while a probe of the same harness runs waits for that probe instead of starting another,
and every open browser reads the new list.

API: `POST /settings/harnesses/<name>/refresh` answers the harness's entry of
`GET /settings → harness_descriptors.harnesses` (`probed_at` included), `404` for a harness
PDO does not resolve. It runs the harness's binary on demand: an extension of rights authorizes it
as `instance_settings` × `refresh_catalogue`. The event stream (`/ws`) then carries
`{"type": "harness_catalogue_changed", "harness": "<name>"}`.

Inside a sandbox, the image and profile define the available harness configuration.

## See also

- [ADR-0045](../adr/0045-un-harnais-se-declare-par-un-template-d-argv-les-capacites-remplissent-les-trous.md): a harness is declared by an argv template; capabilities fill the gaps.
- [ADR-0051](../adr/0051-une-capacite-de-harnais-est-un-point-de-dispatch-pas-une-garde-de-presence.md): a harness capability is a dispatch point, not a presence guard.
- [CLI reference](cli.md), for `pdo docs support-table`.
