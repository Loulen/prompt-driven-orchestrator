---
name: pdo-interactive
description: Talk with a human inside a PDO node — declare when you wait for them (`pdo wait-user`), keep the conversation open, complete once released.
skill_version: 3
---

# Talking with a human inside a PDO node

You are running inside a PDO **node session** a human can talk to, in a terminal
they open from the PDO UI. Your identity travels in the environment
(`PDO_RUN_ID`, `PDO_NODE_ID`, `PDO_NODE_ITER`, `PDO_DAEMON_URL`).

PDO does **not** know when you are waiting for them — you have to say it.

## Declare your wait

Every time you stop and need the user (a question, a choice, a review), run:

```bash
pdo wait-user --message "<your question, one line, under 100 characters>"
```

The node and its run turn **awaiting-user** in the PDO UI, with your message
on the banner: that is how the user, who may be elsewhere, learns it is their
turn. Do not skip it — a node that stays silently « running » while waiting is
the failure this skill exists to prevent.

## The banner is a notification, not the message

The banner only tells the user it is their turn; they read your actual message
in the conversation. So:

- **Before** running `pdo wait-user`, write the full question — or the full
  result you want them to review — in the conversation. The `--message` is a
  one-line summary of it, never a substitute.
- Never refer the user to « my previous message » or « the message above my
  call »: they may not have it on screen. If they ask you to repeat something,
  write it again in full.

- The wait **lifts by itself** when the user types an answer in the PDO
  terminal (their Enter), or when they release your completion. You have
  nothing to run afterwards — just continue the conversation.
- Ask again later ⇒ declare again. A repeat with the same message is a no-op;
  a new message refreshes the banner.
- `--message` is optional but always better: a bare `pdo wait-user` shows
  « the agent is waiting for you » with no hint of what about.
- The command is refused (exit 3) only when there is no live session to wait
  in — nothing to do then. Never follow a refusal with `pdo fail`.

## Never block the conversation

`pdo wait-user` returns immediately. Do not loop, sleep, or poll for the
answer; do not run a blocking command « until the user replies ». The user
answers in the same conversation you are in.

## Finishing

Your completion is **guarded**: PDO refuses `pdo complete` on this node until
the user chooses **« Auto-approve when ready »** in the **Approve** menu
of the PDO UI.

1. When the user says they are done, finish your work and write your outputs.
2. Run `pdo complete`. Two outcomes:
   - **exit 0** — the user had already released you: the node is complete.
   - **exit 3, `completion_not_released`** — not yet released. PDO has just
     declared the wait for you (the banner says you are done and wait for
     their go). Tell the user to choose **« Auto-approve when ready »** in the
     **Approve** menu, or to click **« Approve »** to take the artifacts as
     they are. Once they release the node, PDO types a `[pdo-runtime]` line in
     your terminal: then run `pdo complete` again (or sooner, if they tell you).
3. **Do NOT run `pdo fail`** after an exit 3: nothing failed, the node is still
   yours.

## Good practice

- One question at a time, in the message and in the conversation.
- Summarise the decision taken before moving on: the user may read the pane
  later, not live.
- If the user goes silent, stay declared and keep the pane readable; do not
  invent their answer.
