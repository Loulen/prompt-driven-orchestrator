//! The sections of the preamble past a node's inputs and outputs: an
//! interrupted attempt's partial output, the secondary repositories, where the
//! node edits source code, how it completes, the pipeline's variables.

use super::AugmentContext;

/// The partial output an interrupted attempt left behind (ADR-0049).
pub(super) fn push_partial_outputs(preamble: &mut String, ctx: &AugmentContext<'_>) {
    // The partial output an INTERRUPTED attempt left behind (ADR-0049). Empty on
    // a first spawn, so this section only appears on a recovery.
    if !ctx.partial_outputs.is_empty() {
        preamble.push_str("## Partial output from an interrupted attempt\n\n");
        preamble.push_str(
            "A previous attempt at THIS node was interrupted (an infra incident, not a \
             failure — ADR-0049) and its partial output survived on disk. It is provided \
             here as **input**: read it first and continue from it. Do **not** blindly \
             overwrite it or start from scratch — keeping this work is the whole point of \
             the restart.\n\n",
        );
        for path in ctx.partial_outputs {
            preamble.push_str(&format!("- read `{}`\n", path.display()));
        }
        preamble.push('\n');
    }
}

/// The Run's secondary repositories, by absolute path (ADR-0042/0047).
pub(super) fn push_secondary_repos(preamble: &mut String, ctx: &AugmentContext<'_>) {
    // Secondary repositories, injected by ABSOLUTE path because a node's
    // sub-worktree does not inherit the snapshot files. Writable by default
    // (ADR-0047); a `read_only` opt-in makes a tracked-file write trip the
    // `secondary_repo_dirtied` guard (409).
    if !ctx.secondary_repos.is_empty() {
        let (writable, read_only): (Vec<_>, Vec<_>) =
            ctx.secondary_repos.iter().partition(|s| !s.read_only);

        preamble.push_str("## Secondary repositories\n\n");
        preamble.push_str(
            "These repositories are associated with this Run (#465). Read and reach them \
             by **absolute path** — your sub-worktree does not contain them. Each is a \
             worktree pinned to a fixed commit.\n\n",
        );

        if !writable.is_empty() {
            preamble.push_str(
                "**Writable** (ADR-0047) — you **MAY** modify, commit, and deliver these \
                 repositories. `git` works inside them (their `.git` is mounted rw in the \
                 sandbox). PDO does **not** deliver them for you: do it yourself from the \
                 repository's own directory (e.g. `git checkout -b …`, commit, then \
                 `gh pr create` / `git push`), exactly as you would the primary. \
                 Uncommitted changes are lost when the Run is torn down. Their \
                 absolute paths are listed below.\n\n",
            );
            for sec in &writable {
                preamble.push_str(&format!(
                    "- `{}` (writable, pinned @ `{}`): `{}`\n",
                    sec.alias, sec.sha, sec.abs_path
                ));
            }
            preamble.push('\n');
        }

        if !read_only.is_empty() {
            preamble.push_str(
                "**Read-only** — these are read-only **context** only. Do **not** modify, \
                 commit in, or open MRs against them; writing to a tracked file will be \
                 refused (`secondary_repo_dirtied`, 409).\n\n",
            );
            for sec in &read_only {
                preamble.push_str(&format!(
                    "- `{}` (read-only, pinned @ `{}`): `{}`\n",
                    sec.alias, sec.sha, sec.abs_path
                ));
            }
            preamble.push('\n');
        }
    }
}

/// Where the node edits source code, for either isolation (#654 / ADR-0060).
pub(super) fn push_source_edits(preamble: &mut String, ctx: &AugmentContext<'_>) {
    // Source code edits. Two isolations, one section, and NO git instruction in
    // either (#654 / ADR-0060): the Node edits files, the runtime delivers what it
    // leaves. What differs is only *where* the NodeRun works.
    if let Some(shared_wt) = ctx.shared_worktree_dir {
        preamble.push_str("## Source code edits\n\n");
        preamble.push_str(&format!(
            "Your working directory `{}` is the **run's shared worktree**: other \
             nodes of this run may work in it at the same time. Make **all** \
             source code edits there — do not `cd` elsewhere to edit files.\n\n\
             You do not have to run any git command. When this node finishes, PDO \
             keeps whatever you committed yourself and commits everything else you \
             left behind onto the run's branch. Anything the repository's \
             `.gitignore` covers stays out, and so does PDO's own \
             `.pdo/artifacts/`; everything else goes in.\n\n",
            shared_wt.display()
        ));
    }
    if let Some(sub_wt) = ctx.source_worktree_dir {
        preamble.push_str("## Source code edits\n\n");
        preamble.push_str(&format!(
            "Your working directory `{}` is a **dedicated git worktree** of \
             the project, on its own branch. Make **all** source code edits \
             there — do not `cd` elsewhere to edit files. Read with relative \
             paths or paths under this directory.\n\n\
             The input/output artefact paths above live in the *pipeline \
             worktree* (a different directory, shared with other nodes). \
             Treat those paths as read-only/write-only for artefacts; never \
             edit source code there.\n\n\
             You do not have to run any git command. When this node finishes, PDO \
             keeps whatever you committed yourself, commits everything else you \
             left behind, then merges this worktree back into the pipeline \
             worktree. Edits made outside this directory are not part of that \
             delivery.\n\n",
            sub_wt.display()
        ));

        // The interrupted-git-op notice goes into the re-spawned node's OWN
        // preamble, not only the response the manager sees. Both parts are
        // conditional, so a fresh cut (the common case) gets neither.
        if ctx.reused_sub_worktree {
            preamble.push_str(
                "> **This worktree was REUSED from a previous attempt.** A prior agent may \
                 have left uncommitted work here. Inspect what is already in the working \
                 directory (`git status`, read the changed files) BEFORE you start over — do \
                 not blindly reset or redo work that is already done.\n\n",
            );
        }
        if !ctx.interrupted_git_ops.is_empty() {
            let listed = ctx
                .interrupted_git_ops
                .iter()
                .map(|op| format!("`{op}`"))
                .collect::<Vec<_>>()
                .join(", ");
            preamble.push_str(&format!(
                "> ⚠ **An interrupted git operation was left in this worktree:** {listed}\n>\n"
            ));
            // Vector order is load-bearing: `index.lock` leads, and its
            // instruction ("remove it before anything else") depends on that.
            for op in ctx.interrupted_git_ops {
                let line = match op.as_str() {
                    "index.lock" => "> - `index.lock`: first confirm no git process is running \
                         here, then remove `.git/index.lock` **before anything else** — the \
                         `--abort` / `--continue` commands below themselves need the index lock \
                         free to run.\n"
                        .to_string(),
                    "MERGE_HEAD" => "> - `MERGE_HEAD`: a merge is in progress. Inspect it (`git \
                         status`, `git diff`) — it may carry conflicts a previous agent already \
                         resolved (work worth keeping). Decide deliberately: finish it (`git \
                         commit`) **or** abandon it (`git merge --abort`). Never `--abort` \
                         blindly.\n"
                        .to_string(),
                    "rebase-merge" | "rebase-apply" => "> - a rebase is in progress. `git \
                         status` to inspect; `git rebase --continue` to finish or `git rebase \
                         --abort` to abandon.\n"
                        .to_string(),
                    other => format!("> - `{other}`: resolve this git state before completing.\n"),
                };
                preamble.push_str(&line);
            }
            preamble.push_str(
                ">\n> Do NOT run `pdo complete` until the worktree is in a clean git state — \
                 otherwise the merge-back may record a merge commit nobody intended, \
                 **silently**.\n\n",
            );
        }
    }
}

/// The address humans open PDO at, when the instance sets one: every link the
/// agent hands a human starts there, never at `localhost`.
pub(super) fn push_public_url(preamble: &mut String, ctx: &AugmentContext<'_>) {
    let Some(url) = ctx.public_url else {
        return;
    };
    preamble.push_str("## Public URL\n\n");
    preamble.push_str(&format!(
        "Humans open this PDO at `{url}` (also in `PDO_PUBLIC_URL`). Start every link you \
         give them there, never at `localhost`: this Run is `{url}/#/runs/{run}`, a node \
         `{url}/#/runs/{run}/nodes/<node>`, its Review `{url}/runs/{run}/review`. \
         `pdo page mount` prints the links of a mounted page under it.\n\n",
        run = ctx.run_id
    ));
}

/// How the node completes, fails, skips or waits on its user.
pub(super) fn push_completion(preamble: &mut String) {
    // #588 / ADR-0069 §2: ONE Completion block for every agentic node. The
    // preamble says nothing about interactivity or orchestration — those
    // conducts arrive through the toggle skills (`/pdo-interactive`,
    // `/pdo-orchestrate`, see `build_full_prompt`), and an interactive node
    // learns what it is from the daemon's refusal, never from a prior warning.
    preamble.push_str("## Completion\n\n");
    preamble.push_str(
        "When you are done, signal completion by running:\n\
         ```\n\
         pdo complete\n\
         ```\n\n\
         **`pdo complete` can be REFUSED**, and its exit code tells you what to do \
         next (#490):\n\
         - **0** — granted, or a legal duplicate. Nothing more to do.\n\
         - **3** — refused, *and it is still your turn*: the refusal names its cause \
         on stderr (missing outputs, a frontmatter mismatch, a completion the user \
         has not released yet, child runs still in flight, …) and tells you what to \
         do. The node is still running and nothing has failed. Do what stderr says, \
         then run `pdo complete` again. **Do NOT run `pdo fail`.**\n\
         - **4** — refused, *and the runtime has already ruled*: the failure is \
         already recorded in the run log. **Do NOT run `pdo fail`** — you would \
         record it a second time, with a wrong reason. Stop and report what \
         happened.\n\
         - **1** — the daemon could not be reached or gave no verdict. This is the \
         only case where signalling failure yourself is right.\n\n\
         If you cannot complete the task, signal failure:\n\
         ```\n\
         pdo fail --reason \"<description of the problem>\"\n\
         ```\n\n\
         If there is legitimately nothing to do — your input/pool is empty \
         through no error (e.g. the eligible items were all claimed before \
         you ran) — record a graceful no-op instead of a failure. This ends \
         the run as `skipped` (not `failed`) and short-circuits downstream:\n\
         ```\n\
         pdo skip --reason \"<why there is nothing to do>\"\n\
         ```\n\n\
         If you are blocked on a question only your user can answer, declare it \
         instead of waiting silently:\n\
         ```\n\
         pdo wait-user --message \"<your question, under 100 characters>\"\n\
         ```\n\
         The run turns awaiting-user with your question on its banner; the wait \
         lifts by itself when the user answers in the PDO terminal. The command \
         returns at once — never block or poll for the answer.\n\n",
    );
}

/// The pipeline's variables, and the lap of a collection fan-out.
pub(super) fn push_variables(preamble: &mut String, ctx: &AugmentContext<'_>) {
    if !ctx.variables.is_empty() {
        preamble.push_str("## Pipeline Variables\n\n");
        for (name, value) in ctx.variables {
            let val_str = serde_yaml::to_string(value).unwrap_or_else(|_| format!("{value:?}"));
            preamble.push_str(&format!("- `${name}` = {}\n", val_str.trim()));
        }
        preamble.push('\n');
    }

    if let Some(ref fe) = ctx.foreach_context {
        preamble.push_str("## Collection Context\n\n");
        preamble.push_str(&format!(
            "This node is running as one lap of a collection fan-out ({} of {}).\n",
            fe.current_iter, fe.total
        ));
        preamble.push_str(&format!("- `current_item`: {}\n", fe.current_item));
        preamble.push_str(&format!("- `current_iter`: {}\n", fe.current_iter));
        preamble.push_str(&format!("- `total`: {}\n\n", fe.total));
    }
}
