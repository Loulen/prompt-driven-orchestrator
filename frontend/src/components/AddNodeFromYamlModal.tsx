import { useState } from "react";
import type { NodeDef, NodeType } from "../types";
import { parseNodeYaml, libraryPortToPortDef } from "../api";
import { useEditStore } from "../stores/editStore";
import { generateNodeId } from "../lib/nanoid";

/**
 * Add a node to the canvas from YAML (#345): paste a node definition OR upload a
 * `.yaml`, validated by the daemon (`POST /nodes/parse`), then instantiated as a
 * fresh, disconnected node — a new id, placed at the drop position, selected —
 * exactly like Duplicate / a library insert. NOT called "import" (that term is
 * reserved for the lossy foreign-workflow decompiler, ADR-0016).
 *
 * Two-channel result idiom, verbatim from `ImportWorkflowModal`: a red `error`
 * box for hard failures (blocks, modal stays open, NO node created) and an amber
 * `warnings` list for soft losses (node created, "Done"). The YAML is only ever
 * rendered inside a `<textarea>` as plain text — no markdown/HTML (ADR-0013/0018).
 */
export default function AddNodeFromYamlModal({
  getDropPosition,
  onClose,
}: {
  getDropPosition: () => { x: number; y: number };
  onClose: () => void;
}) {
  const [yaml, setYaml] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [warnings, setWarnings] = useState<string[] | null>(null);
  const addNode = useEditStore((s) => s.addNode);
  const updatePrompt = useEditStore((s) => s.updatePrompt);
  const setSelection = useEditStore((s) => s.setSelection);

  async function handleFile(file: File | null) {
    setError(null);
    setWarnings(null);
    if (!file) return;
    // The `.yaml` is read client-side and its text drops straight into the
    // textarea — the same single value feeds paste and upload (nothing is sent
    // until Submit), so the user can review/edit before creating the node.
    setYaml(await file.text());
  }

  async function handleSubmit() {
    if (!yaml.trim() || submitting) return;
    setSubmitting(true);
    setError(null);
    setWarnings(null);
    try {
      const result = await parseNodeYaml(yaml);
      // Mirror LibraryDropdown.handleAdd: fresh id, drop-position, addNode
      // (snapshots undo in one step), then the prompt sidecar. Cast covers the
      // legacy `switch`/`loop` the TS union omits (soft-warned by the daemon).
      const newId = generateNodeId();
      const node: NodeDef = {
        id: newId,
        name: result.spec.name,
        type: result.spec.type as NodeType,
        inputs: result.spec.inputs.map((p) => libraryPortToPortDef(p, "left")),
        outputs: result.spec.outputs.map((p) => libraryPortToPortDef(p, "right")),
        interactive: result.spec.interactive,
        model: result.spec.model ?? null,
        view: getDropPosition(),
      };
      addNode(node);
      updatePrompt(newId, result.prompt);
      setSelection({ kind: "node", id: newId });

      const w = result.warnings ?? [];
      if (w.length > 0) {
        // Node created, but surface the soft losses (ADR-0001) before closing.
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

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="add-node-yaml-backdrop"
      onClick={onClose}
    >
      <div
        className="w-[440px] max-w-[90vw] rounded-lg border border-line bg-bg-4 p-4"
        style={{ fontSize: "12px" }}
        data-testid="add-node-yaml-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="mb-1 font-medium text-fg">Add node from YAML</div>
        <p className="mb-3 text-fg-4" style={{ fontSize: "11px" }}>
          Paste a node definition or load a <code>.yaml</code> file. It is
          validated by the daemon, then added to the canvas with a fresh id and
          no edges — like a duplicate.
        </p>

        <label className="mb-1 block text-fg-3" style={{ fontSize: "11px" }}>
          Node YAML
        </label>
        <textarea
          value={yaml}
          onChange={(e) => {
            setYaml(e.target.value);
            setError(null);
            setWarnings(null);
          }}
          disabled={warnings != null}
          placeholder={"name: My node\ntype: doc-only\nprompt: |\n  ..."}
          data-testid="add-node-yaml-textarea"
          className="mb-2 h-40 w-full resize-y rounded border border-line-strong bg-bg-3 px-2 py-1.5 font-mono text-fg outline-none focus:border-acc disabled:opacity-60"
          style={{ fontSize: "11px", lineHeight: "1.5" }}
        />

        <input
          type="file"
          accept=".yaml,.yml"
          data-testid="add-node-yaml-file"
          disabled={warnings != null}
          onChange={(e) => handleFile(e.target.files?.[0] ?? null)}
          className="mb-3 w-full rounded border border-line-strong bg-bg-3 px-2 py-1.5 text-fg outline-none file:mr-2 file:rounded file:border-0 file:bg-bg-4 file:px-2 file:py-0.5 file:text-fg-3"
        />

        {error && (
          <div
            className="mb-3 rounded border border-st-failed/40 bg-st-failed/10 px-2 py-1.5 text-st-failed"
            style={{ fontSize: "11px" }}
            data-testid="add-node-yaml-error"
          >
            {error}
          </div>
        )}

        {warnings && (
          <div
            className="mb-3 max-h-40 overflow-y-auto rounded border border-st-await/40 bg-st-await/10 px-2 py-1.5 text-fg-2"
            style={{ fontSize: "11px" }}
            data-testid="add-node-yaml-warnings"
          >
            <div className="mb-1 font-medium text-st-await">
              Node added with {warnings.length} warning
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
            className="rounded border border-line-strong bg-bg-3 px-3 py-1 text-fg-3 transition-colors hover:text-fg"
          >
            {warnings ? "Done" : "Cancel"}
          </button>
          {!warnings && (
            <button
              onClick={handleSubmit}
              disabled={!yaml.trim() || submitting}
              data-testid="add-node-yaml-submit"
              className="rounded bg-acc px-3 py-1 font-medium text-bg-0 transition-colors hover:bg-acc-dim disabled:opacity-50"
            >
              {submitting ? "Adding…" : "Add node"}
            </button>
          )}
        </div>
      </div>
    </div>
  );
}
