import { useEffect, useMemo, useState } from "react";
import { Copy, Download, Check } from "lucide-react";
import type { NodeDef } from "../types";
import { exportNodeAsYaml } from "../stores/editStore";
import { highlightYaml } from "./PipelineInfoPanel";

/**
 * Export a single node's definition as YAML (#345). Pure front-end: the YAML is
 * serialized from the in-memory node (no daemon round-trip), shown syntax-
 * highlighted, and offered for copy or `.yaml` download. The output is the
 * node-library shape — directly re-importable via `Add node from YAML…`.
 *
 * The YAML is rendered as plain text in a `<pre>` (never markdown/HTML), so this
 * stays outside the `dangerouslySetInnerHTML` boundary (ADR-0013/0018).
 */
export default function ExportNodeYamlModal({
  node,
  prompt,
  onClose,
}: {
  node: NodeDef;
  prompt: string;
  onClose: () => void;
}) {
  const yaml = useMemo(() => exportNodeAsYaml(node, prompt), [node, prompt]);
  const [copied, setCopied] = useState(false);

  useEffect(() => {
    function handleKey(e: KeyboardEvent) {
      if (e.key === "Escape") onClose();
    }
    document.addEventListener("keydown", handleKey);
    return () => document.removeEventListener("keydown", handleKey);
  }, [onClose]);

  useEffect(() => {
    if (!copied) return;
    const t = setTimeout(() => setCopied(false), 1500);
    return () => clearTimeout(t);
  }, [copied]);

  async function handleCopy() {
    try {
      await navigator.clipboard.writeText(yaml);
      setCopied(true);
    } catch {
      // Clipboard denied (permissions / insecure context) — no toast; the user
      // can still select the <pre> text manually.
    }
  }

  function handleDownload() {
    const blob = new Blob([yaml], { type: "text/yaml" });
    const url = URL.createObjectURL(blob);
    const a = document.createElement("a");
    a.href = url;
    a.download = `${slug(node.name ?? node.id)}.yaml`;
    document.body.appendChild(a);
    a.click();
    a.remove();
    URL.revokeObjectURL(url);
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center bg-black/60"
      data-testid="export-node-backdrop"
      onClick={onClose}
    >
      <div
        className="flex max-h-[80vh] w-[560px] max-w-[90vw] flex-col rounded-lg border border-line bg-bg-2 shadow-lg"
        style={{ fontSize: "12px" }}
        data-testid="export-node-modal"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <div className="min-w-0">
            <h3 className="font-medium text-fg" style={{ fontSize: "13px" }}>
              Export as YAML
            </h3>
            <p className="mt-0.5 truncate text-fg-4" style={{ fontSize: "11px" }}>
              {node.name ?? node.id}
            </p>
          </div>
        </div>

        <pre
          className="min-h-0 flex-1 overflow-auto border-b border-line bg-bg-1 p-3 font-mono text-fg-3 select-text"
          style={{ fontSize: "11px", lineHeight: "1.6", tabSize: 2 }}
          data-testid="export-node-yaml"
        >
          {highlightYaml(yaml)}
        </pre>

        <div className="flex justify-end gap-2 px-4 py-3">
          <button
            onClick={handleCopy}
            data-testid="export-node-copy"
            className="flex items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            {copied ? <Check size={13} className="text-st-done" /> : <Copy size={13} />}
            {copied ? "Copied!" : "Copy"}
          </button>
          <button
            onClick={handleDownload}
            data-testid="export-node-download"
            className="flex items-center gap-1.5 rounded-md border border-line-strong bg-bg-3 px-3 py-1.5 text-fg-2 transition-colors hover:bg-bg-4"
            style={{ fontSize: "11.5px" }}
          >
            <Download size={13} />
            Download .yaml
          </button>
          <button
            onClick={onClose}
            data-testid="export-node-close"
            className="rounded-md bg-acc px-3 py-1.5 font-medium text-bg-0 transition-colors hover:bg-acc-dim"
            style={{ fontSize: "11.5px" }}
          >
            Close
          </button>
        </div>
      </div>
    </div>
  );
}

/**
 * Slug for the download filename — mirror of the daemon's `slugify`: lowercase,
 * keep `[a-z0-9_-]`, spaces → `-`, everything else dropped; empty ⇒ `node`.
 */
function slug(name: string): string {
  let out = "";
  for (const ch of name) {
    if (/[a-zA-Z0-9_-]/.test(ch)) out += ch.toLowerCase();
    else if (ch === " ") out += "-";
  }
  return out || "node";
}
