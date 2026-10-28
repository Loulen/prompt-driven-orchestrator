/**
 * The YAML tab: the document as the daemon would save it, copy and portable export.
 */

import { useState, useMemo, useRef, useEffect } from "react";
import { Check, Copy, Download, EyeOff, FileCode } from "lucide-react";
import { Banner, Button, Chip, EmptyState, IconButton } from "../../shared/ui/kit";
import "./inspect.css";
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
    // The line height of `.pi-yaml`: 11 px type at 1.6.
    const lineHeight = 11 * 1.6;
    const scrollTop = Math.max(0, (scrollToLine - 3) * lineHeight);
    preRef.current.scrollTop = scrollTop;
  }, [scrollToLine]);

  if (!pipeline) {
    return (
      <div className="pi-center">
        <EmptyState icon={FileCode}>No pipeline</EmptyState>
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
    <div className="pi-body">
      <div className="pi-docbar" data-testid="portable-document-bar">
        <span className="ei-trunc" title="The document as Save writes it, portable to another instance">
          Portable document · v1
        </span>
        {runId && <Chip title="This is the pipeline that ran, not the Run: no runtime value">ran</Chip>}
        <span className="ei-row__end">
          <IconButton icon={copied ? Check : Copy} label={copied ? "Copied" : "Copy"} onClick={copyDocument} disabled={yaml == null} />
          <IconButton icon={Download} label="Download" onClick={downloadDocument} disabled={yaml == null} />
          {skillCount > 0 && (
            <Button
              size="sm"
              tone="ghost"
              icon={Download}
              onClick={() => void downloadSkillsSidecar()}
              title={`The ${skillCount} skill${skillCount === 1 ? "" : "s"} this pipeline selects, as ${pipelineName}.skills/<id>/… to unpack next to the YAML`}
              data-testid="download-skills-sidecar"
            >
              Skills ({skillCount})
            </Button>
          )}
          <IconButton
            icon={EyeOff}
            label="Not included"
            pressed={showExcluded}
            onClick={() => setShowExcluded((value) => !value)}
            data-testid="yaml-not-included"
          />
        </span>
      </div>
      {copyError && (
        <div>
          <Banner tone="failed" title="Clipboard denied" text="Use Download" />
        </div>
      )}
      {sidecarError && (
        <div data-testid="skills-sidecar-error">
          <Banner tone="failed" title="Skills" text={sidecarError} />
        </div>
      )}
      {skillCount > 0 && (
        <div
          className="pi-docnote ei-dim"
          title="Skills are content: import the sidecar with the YAML so the bank recreates them"
          data-testid="skills-sidecar-note"
        >
          <span className="ei-trunc">
            {skillCount} skill{skillCount === 1 ? "" : "s"} by id · sidecar <span className="d-mono">{pipelineName}.skills/</span>
          </span>
        </div>
      )}
      {dirty && (
        <div
          className="pi-docnote ei-warn"
          title="Copy and Download export the last saved document until you Save"
          data-testid="yaml-unsaved-note"
        >
          <span className="ei-trunc">Unsaved edits shown · exports the last save</span>
        </div>
      )}
      {showExcluded && (
        <div className="pi-docnote ei-dim" data-testid="yaml-excluded">
          <span className="ei-trunc" title="Named agent profiles become Inherit; shared nodes become ordinary nodes; skills travel in the sidecar">
            Not included: secrets, environment, runtime values, instance configuration
          </span>
        </div>
      )}
      <pre ref={preRef} className="pi-yaml" data-testid="info-yaml-content">
        {highlightYaml(shownYaml, scrollToLine)}
      </pre>
    </div>
  );
}
