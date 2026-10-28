import { useState } from "react";
import { ArrowUpRight, Download, FileText, Files, Images, TriangleAlert } from "lucide-react";
import { IconButton, Section, Tooltip } from "../../../shared/ui/kit";
import type { LiveArtifactIO } from "../../../shared/generated/LiveArtifactIO";
import type { SchemaWarning } from "../../../shared/generated/SchemaWarning";
import type { FileInfo } from "../../api/nodes";
import { artifactDownloadUrl, artifactUrl, portZipUrl } from "../../api/artifacts";
import ImageLightbox from "../../../shared/ui/ImageLightbox";
import { formatSize, isImageFile } from "../../model/nodeDetailFormat";
import { liveDisplayPath, liveUpdatedAgo, newestUpdate } from "../../model/liveArtifacts";

/**
 * ADR-0089: the live artifacts of the iteration on screen, polled with the I/O
 * while the node works, one row each under the Outputs, a live dot before it —
 * declared ones first (« not produced » until the agent publishes them), then
 * the ones it published freely. The frontmatter reads as tags; a field that
 * misses the declared schema is an amber tag, its rule on hover. Nothing here
 * is a verdict: a live artifact is never checked.
 */
export default function LiveSection({
  live,
  runId,
  onOpen,
}: {
  live: LiveArtifactIO[];
  runId: string;
  onOpen: (artifact: LiveArtifactIO) => void;
}) {
  const newest = newestUpdate(live);
  return (
    <div data-testid="live-section">
      <Section
        title="Live artifacts"
        count={live.length}
        flush
        aside={
          newest && (
            <span className="font-mono text-2xs text-text-3" title={newest} data-testid="live-updated">
              updated {liveUpdatedAgo(newest)}
            </span>
          )
        }
      >
        <div className="ri-ports">
          {live.map((artifact) => (
            <LiveRow
              key={`${artifact.declared ? "d" : "f"}:${artifact.name}`}
              artifact={artifact}
              runId={runId}
              onOpen={() => onOpen(artifact)}
            />
          ))}
        </div>
      </Section>
    </div>
  );
}

function LiveRow({
  artifact,
  runId,
  onOpen,
}: {
  artifact: LiveArtifactIO;
  runId: string;
  onOpen: () => void;
}) {
  const [lightbox, setLightbox] = useState<{ images: string[]; index: number } | null>(null);
  // #796: a file older than this execution is a previous one's, not this
  // node's live work.
  const current = artifact.files.filter((f) => f.exists && !f.inherited);
  const produced = current.length > 0;
  const isFolder = artifact.path.endsWith("/");
  const images = current.filter((f) => isImageFile(f.path));
  const single: FileInfo | undefined = !isFolder ? current[0] : undefined;
  const totalSize = current.reduce((sum, f) => sum + (f.size ?? 0), 0);
  const frontmatter = single?.frontmatter ?? null;

  let subline = liveDisplayPath(artifact.path);
  if (isFolder && produced) {
    const count = current.length;
    const noun = images.length === count ? "image" : "file";
    const counted = `${count} ${noun}${count === 1 ? "" : "s"}`;
    // A folder of files names them, as a `files` output does.
    subline =
      artifact.port_type === "files"
        ? `${counted} · ${current.map((f) => f.path.split("/").pop() ?? f.path).join(", ")}`
        : `${subline} · ${counted}`;
  }
  // A single file downloads as itself, a folder as its zip.
  let downloadHref: string | null = null;
  if (single) downloadHref = artifactDownloadUrl(runId, single.path);
  else if (isFolder) downloadHref = portZipUrl(runId, artifact.path.replace(/\/$/, ""));

  if (!produced) {
    return (
      <div
        data-testid="live-row"
        data-live={artifact.name}
        data-produced="false"
        data-pending
        className="ri-port"
        title={`${artifact.name} · not produced`}
      >
        <div className="ri-port__line">
          <span className="ri-dot" aria-hidden />
          <span className="ri-port__name">{artifact.name}</span>
          <span className="ri-port__where" data-testid="live-not-produced">
            not produced
          </span>
        </div>
      </div>
    );
  }

  const Icon = isFolder ? (images.length > 0 ? Images : Files) : FileText;
  return (
    <div data-testid="live-row" data-live={artifact.name} data-produced="true" className="ri-port">
      <div className="ri-port__line">
        <span className="ri-dot" data-live aria-label="live" role="img" />
        <Icon size={14} strokeWidth={1.5} className="ri-port__ic" aria-hidden />
        <button
          type="button"
          onClick={onOpen}
          aria-label={`Open ${artifact.name}`}
          data-testid="live-row-open"
          className="ri-port__name"
        >
          {artifact.name}
        </button>
        {!artifact.declared && (
          <span className="d-chip d-chip--mono" title="Published by the node without being declared" data-testid="live-free-badge">
            free
          </span>
        )}
        <span className="ri-port__where" title={`${artifact.name} · ${artifact.path}`}>
          {subline}
        </span>
        {totalSize > 0 && <span className="ri-port__size">{formatSize(totalSize)}</span>}
        <span className="ri-port__tools">
          {downloadHref && (
            <Tooltip label="Download">
              <a
                href={downloadHref}
                download
                className="d-icon d-icon--sm"
                aria-label={`Download ${artifact.name}`}
                data-testid="live-download"
              >
                <Download size={12} strokeWidth={1.5} aria-hidden />
              </a>
            </Tooltip>
          )}
          <IconButton icon={ArrowUpRight} size="sm" label={`Open ${artifact.name}`} onClick={onOpen} />
        </span>
      </div>

      <FrontmatterTags frontmatter={frontmatter} warnings={artifact.schema_warnings} />

      {images.length > 0 && (
        <div className="ri-thumbs" data-testid="live-thumbnails">
          {images.slice(0, 4).map((f, i) => (
            <button
              key={f.path}
              type="button"
              className="ri-thumb"
              title={f.path.split("/").pop() ?? ""}
              onClick={() => setLightbox({ images: images.map((im) => artifactUrl(runId, im.path)), index: i })}
            >
              <img src={artifactUrl(runId, f.path)} alt={f.path.split("/").pop() ?? ""} />
            </button>
          ))}
          {images.length > 4 && <span className="self-center font-mono text-xs text-text-3">+{images.length - 4}</span>}
        </div>
      )}
      {lightbox && <ImageLightbox images={lightbox.images} index={lightbox.index} onClose={() => setLightbox(null)} />}
    </div>
  );
}

/** The frontmatter as `key value` tags, a field that misses the schema in
 *  amber with its rule on hover — and one amber tag per declared field absent. */
function FrontmatterTags({
  frontmatter,
  warnings,
}: {
  frontmatter: Record<string, unknown> | null;
  warnings: SchemaWarning[];
}) {
  const entries = Object.entries(frontmatter ?? {});
  const byField = new Map(warnings.map((w) => [w.field, w.reason]));
  const absent = warnings.filter((w) => !(frontmatter && w.field in frontmatter));
  if (entries.length === 0 && absent.length === 0) return null;
  return (
    <div className="ri-port__sub" data-testid="live-frontmatter">
      {entries.map(([field, value]) => {
        const reason = byField.get(field);
        return (
          <span
            key={field}
            data-testid={reason ? "live-tag-warning" : "live-tag"}
            data-st={reason ? "awaiting" : undefined}
            title={reason ? `Schema: ${reason}` : `${field}: ${display(value)}`}
            className="ri-fm__chip"
          >
            {reason ? (
              <TriangleAlert size={10} strokeWidth={1.75} aria-label="Schema not respected" />
            ) : (
              <span className="ri-fm__k">{field}</span>
            )}
            <span>{reason ? `${field} ${display(value)}` : display(value)}</span>
          </span>
        );
      })}
      {absent.map((w) => (
        <span
          key={`absent:${w.field}`}
          data-testid="live-tag-warning"
          data-st="awaiting"
          title={`Schema: ${w.reason}`}
          className="ri-fm__chip"
        >
          <TriangleAlert size={10} strokeWidth={1.75} aria-label="Schema not respected" />
          <span>{w.field === "(frontmatter)" ? "frontmatter" : `${w.field} missing`}</span>
        </span>
      ))}
    </div>
  );
}

function display(value: unknown): string {
  return typeof value === "object" ? JSON.stringify(value) : String(value);
}
