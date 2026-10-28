/**
 * Import a pipeline: a portable document (with its skills sidecar) or a Claude Code workflow `.js`.
 */

import { useState } from "react";
import { X } from "lucide-react";
import { importPipelineDocument } from "../api/document";
import { importWorkflow } from "../api/library";
import { announceSkillsChanged } from "../../skill/store/useSkillBank";
import { useEditStore } from "../store/editStore";

export default function ImportWorkflowModal({
  onClose,
}: {
  onClose: () => void;
}) {
  const [mode, setMode] = useState<"pdo" | "claude">("pdo");
  const [file, setFile] = useState<File | null>(null);
  const [content, setContent] = useState("");
  // #673: the skills sidecar (`<pipeline>.skills.zip`) picked beside the YAML,
  // sent as base64. Optional: without it, unknown skill ids import with a
  // "skill absent" warning instead of failing.
  const [sidecar, setSidecar] = useState<{ name: string; base64: string } | null>(null);
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[] | null>(null);
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);

  const looksLikePdo = /^\s*pdo_pipeline\s*:/m.test(content);
  const looksLikeClaude =
    /\b(?:agent|pipeline|parallel)\s*\(/.test(content) ||
    /\bmeta\s*:\s*\{/.test(content) ||
    /^\s*(?:export\s+default|const\s+\w+\s*=)/m.test(content);
  const mismatch =
    content.trim() &&
    ((mode === "pdo" && looksLikeClaude && !looksLikePdo) ||
      (mode === "claude" && looksLikePdo));

  async function handleImport() {
    if (!content.trim() || submitting || mismatch) return;
    setSubmitting(true);
    setError(null);
    setWarnings(null);
    try {
      const result =
        mode === "pdo"
          ? await importPipelineDocument(content, sidecar?.base64)
          : await importWorkflow(file?.name ?? "workflow.js", content);
      await loadPipelines();
      await openPipeline(result.id);
      if (mode === "pdo") {
        // The import may have added skills to the bank (#673): the Skills panel,
        // the selectors and the missing-skill warnings all re-read it.
        announceSkillsChanged();
      }
      const w = "warnings" in result ? result.warnings ?? [] : [];
      if (w.length > 0) {
        // Surface the diagnostics rather than silently closing: for a Claude
        // workflow the lossy-translation annotation is the onboarding tutorial
        // (ADR-0001); for a PDO document it says what the import dropped.
        setWarnings(w);
      } else {
        onClose();
      }
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setSubmitting(false);
    }
  }

  async function loadFile(next: File | null) {
    setFile(next);
    setContent(next ? await next.text() : "");
    setError(null);
    setWarnings(null);
  }

  /// PDO mode: the picker takes the YAML and, beside it, the skills sidecar
  /// (`.zip`). Either may come alone; a later pick replaces its own kind only.
  async function loadPdoFiles(files: FileList | null) {
    setError(null);
    setWarnings(null);
    for (const next of Array.from(files ?? [])) {
      if (/\.zip$/i.test(next.name)) {
        setSidecar({ name: next.name, base64: await fileToBase64(next) });
      } else {
        setFile(next);
        setContent(await next.text());
      }
    }
  }

  return (
    <div className="fixed inset-0 z-50 flex items-center justify-center bg-scrim">
      <div
        className="w-[400px] rounded-lg border border-line bg-surface-2 p-4"
        style={{ fontSize: "12px" }}
        data-testid="import-workflow-modal"
      >
        <div className="mb-1 font-medium text-text">Import a pipeline</div>
        <div className="mb-3 grid grid-cols-2 rounded border border-line-strong bg-surface-2 p-0.5">
          {([
            ["pdo", "Pipeline PDO"],
            ["claude", "Workflow Claude Code"],
          ] as const).map(([value, label]) => (
            <button
              key={value}
              onClick={() => {
                setMode(value);
                setError(null);
                setWarnings(null);
              }}
              className={`rounded px-2 py-1.5 ${
                mode === value ? "bg-surface-2 font-medium text-text" : "text-text-3"
              }`}
              aria-pressed={mode === value}
              data-testid={`import-mode-${value}`}
            >
              {label}
            </button>
          ))}
        </div>

        {mode === "pdo" ? (
          <>
            <p className="mb-2 text-text-3" style={{ fontSize: "11px" }}>
              Paste a portable PDO pipeline document or load a YAML file. If the pipeline
              selects skills, add its <code>.skills.zip</code> sidecar so the bank recreates them.
            </p>
            <textarea
              value={content}
              onChange={(event) => {
                setContent(event.target.value);
                setError(null);
              }}
              className="h-48 w-full resize-y rounded border border-line-strong bg-surface-2 p-2 font-mono text-text outline-none focus:border-primary"
              data-testid="pipeline-document-input"
              autoFocus
            />
            <div className="mb-3 mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1">
              <label className="inline-block cursor-pointer text-primary hover:underline">
                load files…
                <input
                  type="file"
                  accept=".yaml,.yml,.zip"
                  multiple
                  className="hidden"
                  data-testid="pipeline-document-files"
                  onChange={(event) => void loadPdoFiles(event.target.files)}
                />
              </label>
              {sidecar ? (
                <span className="flex items-center gap-1 text-text-3" data-testid="skills-sidecar-chip">
                  skills sidecar: <code>{sidecar.name}</code>
                  <button
                    className="text-text-3 hover:text-text"
                    onClick={() => setSidecar(null)}
                    aria-label="Remove the skills sidecar"
                  >
                    <X size={10} />
                  </button>
                </span>
              ) : (
                <span className="text-text-3">no skills sidecar</span>
              )}
            </div>
          </>
        ) : (
          <>
            <p className="mb-2 text-text-3" style={{ fontSize: "11px" }}>
              Decompile a Claude Code workflow into a draft. Unmapped idioms are
              annotated instead of executed.
            </p>
            <input
              type="file"
              accept=".js"
              data-testid="workflow-file-input"
              onChange={(event) => void loadFile(event.target.files?.[0] ?? null)}
              className="mb-3 w-full rounded border border-line-strong bg-surface-2 px-2 py-1.5 text-text outline-none file:mr-2 file:rounded file:border-0 file:bg-surface-2 file:px-2 file:py-0.5 file:text-text-3"
            />
          </>
        )}

        {mismatch && (
          <div className="mb-3 rounded border border-st-await/40 bg-st-await/10 px-2 py-1.5 text-text-2">
            This content looks like a {mode === "pdo" ? "Claude Code workflow" : "PDO pipeline"}.
            <button
              className="ml-1 text-primary hover:underline"
              onClick={() => setMode(mode === "pdo" ? "claude" : "pdo")}
            >
              Switch mode
            </button>
          </div>
        )}

        {error && (
          <div
            className="mb-3 rounded border border-st-failed/40 bg-st-failed/10 px-2 py-1.5 text-st-failed"
            style={{ fontSize: "11px" }}
            data-testid="import-workflow-error"
          >
            {error}
          </div>
        )}

        {warnings && (
          <div
            className="mb-3 max-h-40 overflow-y-auto rounded border border-st-await/40 bg-st-await/10 px-2 py-1.5 text-text-2"
            style={{ fontSize: "11px" }}
            data-testid="import-workflow-warnings"
          >
            <div className="mb-1 font-medium text-st-await">
              Imported with {warnings.length} warning
              {warnings.length === 1 ? "" : "s"}:
            </div>
            <ul className="list-disc pl-4">
              {warnings.map((w, i) => (
                <li key={i}>{w}</li>
              ))}
            </ul>
          </div>
        )}

        <div className="flex justify-end gap-2">
          <button
            onClick={onClose}
            className="rounded border border-line-strong bg-surface-2 px-3 py-1 text-text-3 transition-colors hover:text-text"
          >
            {warnings ? "Done" : "Cancel"}
          </button>
          {!warnings && (
            <button
              onClick={handleImport}
              disabled={!content.trim() || submitting || !!mismatch}
              className="rounded bg-primary px-3 py-1 font-medium text-surface-0 transition-colors hover:bg-primary-hi disabled:opacity-50"
            >
              {submitting ? "Importing…" : "Import"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

/// #110 — confirm dialog for the one destructive run-level control. Retry-all
/// archives the current run and starts a fresh run of the same pipeline, so it
/// gets a confirm gate (Pause/Resume don't — they're cheap and reversible).

/// The base64 of a picked file (the skills sidecar zip, #673) — what the import
/// request carries; the daemon decodes and unzips it.
async function fileToBase64(file: File): Promise<string> {
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < bytes.length; i += chunk) {
    binary += String.fromCharCode(...bytes.subarray(i, i + chunk));
  }
  return btoa(binary);
}

/// Import a Claude Code workflow `.js` as a draft library pipeline (#155). The
/// file is read client-side and POSTed as text — the daemon parses it to an AST
/// (never executes it) and returns a draft plus lossy-translation warnings.
