# Scenario — `start-node-images`

> Layer 5 (agentic) per ADR 0004. Manual trigger: an agent executes the steps
> below and emits the verdict format at the bottom. Asserts issue #145 — a run
> launched with **input images** surfaces those images both **on the Start
> node** in the canvas and in the **Start inspector**, alongside the prompt
> text. The control case (a run with no images) must render unchanged.

## Setup

- PDO daemon running on the user's repo (`pdo daemon`). Daemon URL
  defaults to `http://127.0.0.1:5172`.
- Frontend reachable in a browser. Chrome DevTools MCP preferred; Playwright
  MCP works as a fallback.
- `claude` available on `PATH`.
- A pipeline `run-minimal-scenario.yaml` exists in `.pdo/pipelines/` (the
  same trivial start → only → end pipeline used by `run-minimal`). If it isn't
  there, create it per `run-minimal.md` Setup before driving the UI.
- Two small image files to upload as input, created up front:

  ```bash
  mkdir -p /tmp/pdo-start-imgs
  # 1×1 PNGs are enough — the bytes only have to be a valid image.
  printf '\x89PNG\r\n\x1a\n' > /tmp/pdo-start-imgs/ui-bug.png
  python3 - <<'PY'
  import base64, pathlib
  png = base64.b64decode(
      "iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR4nGNgYGAAAAAEAAH2FzhVAAAAAElFTkSuQmCC"
  )
  pathlib.Path("/tmp/pdo-start-imgs/ui-bug.png").write_bytes(png)
  pathlib.Path("/tmp/pdo-start-imgs/trace.png").write_bytes(png)
  PY
  ```

  (`trace.txt` from the design mock is text, not an image — the daemon only
  stores files with image extensions in `_input/`, so this scenario uploads two
  *images* to keep the assertion about images specifically.)

## Steps the agent executes

1. Open the UI; confirm the **`Daemon: connected`** label is visible.

2. Open the **New Run** modal. Pick `run-minimal-scenario`. Provide any input
   string (e.g. `look at these`). **Attach the two images**
   (`/tmp/pdo-start-imgs/ui-bug.png`, `/tmp/pdo-start-imgs/trace.png`)
   via the modal's image upload control. Click **Launch**. Capture the
   resulting `run_id`.

3. Confirm the images were stored by the daemon:

   ```bash
   ls .pdo/runs/<run_id>/worktree/.pdo/artifacts/_input/
   ```

   Expect `output.md`, `ui-bug.png`, `trace.png`.

4. **Images on the Start node (canvas).** With the run selected, locate the
   **Start node** (green play-button card, "Run start") in the DAG canvas.
   Assert:
   - An image strip is rendered on/under the Start card
     (`[data-testid="start-node-images"]`), containing **two** image chips —
     one per uploaded image.
   - Each chip is keyed/labelled by its filename (`ui-bug.png`, `trace.png`).
   - Take a screenshot of the Start node with its image strip.

5. **Images in the Start inspector.** Click the **Start node**. The right panel
   swaps to the **StartInspector** (header "Run start", `runtime` badge).
   Assert:
   - The prompt text is still shown in the input `<pre>` block (the existing
     behavior is unchanged).
   - An **Input images** section (`[data-testid="start-inspector-images"]`) is
     present, with **two** image thumbnails. Each thumbnail's `src` resolves to
     `…/runs/<run_id>/artifact?path=_input%2F<filename>` and the image actually
     loads (HTTP 200, non-zero `naturalWidth`).
   - Clicking a thumbnail opens the **ImageLightbox**
     (`[data-testid="image-lightbox"]`); Escape or backdrop click closes it.
   - Take a screenshot of the inspector showing prompt + thumbnails.

6. **Control case — no images renders unchanged.** Launch a **second** run on
   the same pipeline with the **same input string but no image attachments**.
   Capture `run_id_2`. Assert:
   - The Start node for `run_id_2` shows **no** image strip — no
     `[data-testid="start-node-images"]` element on that card.
   - Clicking the Start node opens the StartInspector with the prompt `<pre>`
     and **no** `[data-testid="start-inspector-images"]` section.
   - This matches the pre-#145 Start node/inspector exactly (prompt only).

## Negative checks

- The image thumbnails must be **real images served by the daemon**, not broken
  `<img>` placeholders. If a thumbnail's `naturalWidth` is 0, the artifact
  endpoint isn't serving `_input/<filename>` — that's a FAIL.
- A run with no images must not render an empty image strip / empty section
  (no stray `start-node-images` or `start-inspector-images` node).

## Cleanup

- Archive both runs:

  ```bash
  curl -X POST http://127.0.0.1:5172/runs/<run_id>/commands \
    -H 'content-type: application/json' -d '{"kind":"cleanup_run"}'
  curl -X POST http://127.0.0.1:5172/runs/<run_id_2>/commands \
    -H 'content-type: application/json' -d '{"kind":"cleanup_run"}'
  ```

- Remove `/tmp/pdo-start-imgs/`.

## Verdict format

```json
{
  "verdict": "PASS" | "FAIL",
  "evidence": [
    "step 3: _input/ contains ui-bug.png and trace.png",
    "step 4: Start node shows start-node-images strip with 2 chips (screenshot)",
    "step 5: StartInspector shows prompt + 2 thumbnails, each loads (naturalWidth>0)",
    "step 5: clicking a thumbnail opens then closes the ImageLightbox",
    "step 6: control run shows Start node + inspector with no image strip/section"
  ],
  "anomalies": [
    "<optional — surprising-but-non-fatal observations>"
  ]
}
```

A single failed assertion ⇒ `verdict: "FAIL"`. Don't half-pass.
