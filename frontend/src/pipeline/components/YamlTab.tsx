/**
 * The YAML tab: the document as the daemon would save it, copy and portable export.
 */

import { useState, useMemo, useRef, useEffect } from "react";
import { Copy, Download, ChevronDown, ChevronRight } from "lucide-react";
import { fetchRunPipelineDocument, fetchRunPipelineSkillsSidecar } from "../../run/api/runPipeline";
import { fetchPipelineDocument, fetchPipelineSkillsSidecar } from "../api/document";
import { serializePipeline } from "../../lib/serializePipeline";
import { highlightYaml } from "./yamlHighlight";
import type { EditorPipelineDef } from "../model/pipeline";
import { useEditStore } from "../store/editStore";
import { saveBlob } from "../../shared/lib/download";
import { copyToClipboard } from "../../shared/lib/clipboard";

export function YamlTab({
  pipeline,
  pipelineId,
  runId,
  scrollToLine,
}: {
  pipeline: EditorPipelineDef | null;
  pipelineId: string | null;
  runId: string | null;
  scrollToLine?: number;
}) {
  const preRef = useRef<HTMLPreElement>(null);
  const fallbackYaml = useMemo(
    () => (pipeline ? serializePipeline(pipeline) : ""),
    [pipeline],
  );
  // #948: an edit shows here before it is saved. While the active tab holds
  // unsaved edits the view is the edit buffer serialized (what Save will write);
  // Copy / Download export the last saved version, fetched even while the draft
  // differs from it, and refetched once Save lands.
  const dirty = useEditStore(
    (s) => s.openTabs.find((t) => t.id === s.activeTabId)?.dirty ?? false,
  );
  // The last saved portable document — null until known while the draft differs.
  const [yaml, setYaml] = useState<string | null>(
    dirty && (pipelineId || runId) ? null : fallbackYaml,
  );
  const shownYaml = dirty ? fallbackYaml : (yaml ?? fallbackYaml);
  const [copied, setCopied] = useState(false);
  const [copyError, setCopyError] = useState(false);
  const [showExcluded, setShowExcluded] = useState(false);
  const [sidecarError, setSidecarError] = useState<string | null>(null);
  // #673 / ADR-0062: the skills the nodes select travel beside the YAML, in a
  // sidecar `<pipeline>.skills/<id>/…` — content, not instance configuration.
  const skillCount = useMemo(() => {
    const ids = new Set<string>();
    for (const node of pipeline?.nodes ?? []) {
      for (const skill of node.skills ?? []) ids.add(skill.id);
    }
    return ids.size;
  }, [pipeline]);

  // While the draft differs, its edits do not change the saved document: fetch it once.
  // A pipeline with no saved document exports its buffer.
  const savedKey = dirty && (pipelineId || runId) ? null : fallbackYaml;
  useEffect(() => {
    let cancelled = false;
    const request = runId
      ? fetchRunPipelineDocument(runId)
      : pipelineId
        ? fetchPipelineDocument(pipelineId)
        : Promise.resolve(savedKey);
    request
      .then((document) => {
        if (!cancelled) setYaml(document);
      })
      .catch(() => {
        if (!cancelled) setYaml(savedKey);
      });
    return () => {
      cancelled = true;
    };
  }, [savedKey, pipelineId, runId]);

  useEffect(() => {
    if (scrollToLine == null || !preRef.current) return;
    const lineHeight = 11 * 1.6;
    const scrollTop = Math.max(0, (scrollToLine - 3) * lineHeight);
    preRef.current.scrollTop = scrollTop;
  }, [scrollToLine]);

  if (!pipeline) {
    return (
      <div className="flex flex-1 items-center justify-center text-text-3" style={{ fontSize: "12px" }}>
        No pipeline loaded.
      </div>
    );
  }

  async function copyDocument() {
    if (yaml == null) return;
    setCopyError(false);
    if (await copyToClipboard(yaml)) {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 2000);
    } else {
      setCopyError(true);
    }
  }

  const pipelineName = pipeline.name || "pipeline";

  function downloadDocument() {
    if (yaml == null) return;
    saveBlob(new Blob([yaml], { type: "application/yaml" }), `${pipelineName}.pdo.yaml`);
  }

  async function downloadSkillsSidecar() {
    setSidecarError(null);
    try {
      const blob = runId
        ? await fetchRunPipelineSkillsSidecar(runId)
        : pipelineId
          ? await fetchPipelineSkillsSidecar(pipelineId)
          : null;
      if (!blob) {
        setSidecarError("No skill of this pipeline is in the bank: nothing to export.");
        return;
      }
      saveBlob(blob, `${pipelineName}.skills.zip`);
    } catch (e) {
      setSidecarError(e instanceof Error ? e.message : String(e));
    }
  }

  return (
    <div className="flex min-h-0 flex-1 flex-col overflow-auto">
      <div className="border-b border-line bg-surface-2 px-3 py-2" data-testid="portable-document-bar">
        <div className="flex items-center gap-2">
          <span className="h-1.5 w-1.5 rounded-full bg-primary" />
          <span className="font-medium text-text-2" style={{ fontSize: "11px" }}>
            Portable document · v1
          </span>
          <button
            className="ml-auto flex items-center gap-1 rounded px-1.5 py-1 text-text-3 hover:bg-hover hover:text-text"
            onClick={copyDocument}
            disabled={yaml == null}
          >
            <Copy size={11} />
            {copied ? "✓ Copied" : "Copy"}
          </button>
          <button
            className="flex items-center gap-1 rounded px-1.5 py-1 text-text-3 hover:bg-hover hover:text-text"
            onClick={downloadDocument}
            disabled={yaml == null}
          >
            <Download size={11} />
            Download
          </button>
          {skillCount > 0 && (
            <button
              className="flex items-center gap-1 rounded px-1.5 py-1 text-text-3 hover:bg-hover hover:text-text"
              onClick={() => void downloadSkillsSidecar()}
              title={`The ${skillCount} skill${skillCount === 1 ? "" : "s"} this pipeline selects, as ${pipelineName}.skills/<id>/… to unpack next to the YAML`}
              data-testid="download-skills-sidecar"
            >
              <Download size={11} />
              Skills ({skillCount})
            </button>
          )}
        </div>
        {copyError && (
          <div className="mt-1 text-st-failed" role="alert">
            Clipboard access was denied. Use Download instead.
          </div>
        )}
        {sidecarError && (
          <div className="mt-1 text-st-failed" role="alert" data-testid="skills-sidecar-error">
            {sidecarError}
          </div>
        )}
        {skillCount > 0 && (
          <p className="mt-1 text-text-3" style={{ fontSize: "10px" }} data-testid="skills-sidecar-note">
            {skillCount} skill{skillCount === 1 ? "" : "s"} referenced by id. Their content travels in
            the sidecar <code>{pipelineName}.skills/</code>; import both so the bank recreates them.
          </p>
        )}
        {dirty && (
          <p className="mt-1 text-st-await" style={{ fontSize: "10px" }} data-testid="yaml-unsaved-note">
            Unsaved edits shown. Copy and Download export the last saved document until you Save.
          </p>
        )}
        {runId && (
          <p className="mt-1 text-text-3" style={{ fontSize: "10px" }}>
            This is the pipeline that ran, not the Run. Runtime values are not included.
          </p>
        )}
        <button
          className="mt-1.5 flex items-center gap-1 text-text-3 hover:text-text-2"
          style={{ fontSize: "10.5px" }}
          onClick={() => setShowExcluded((value) => !value)}
        >
          {showExcluded ? <ChevronDown size={11} /> : <ChevronRight size={11} />}
          Not included
        </button>
        {showExcluded && (
          <p className="mt-1 text-text-3" style={{ fontSize: "10px" }}>
            Secrets, environment, runtime values, and instance configuration. Named agent
            profiles become Inherit; shared nodes become ordinary nodes. Skills are content,
            not configuration: they travel in the separate skills sidecar.
          </p>
        )}
      </div>
      <pre
        ref={preRef}
        className="flex-1 overflow-auto p-3 font-mono text-text-3 select-text"
        style={{ fontSize: "11px", lineHeight: "1.6", tabSize: 2 }}
        data-testid="info-yaml-content"
      >
        {highlightYaml(shownYaml, scrollToLine)}
      </pre>
    </div>
  );
}

