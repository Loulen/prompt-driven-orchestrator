import { useState } from "react";
import { Download, TriangleAlert } from "lucide-react";
import type { LiveArtifactIO } from "../../../shared/generated/LiveArtifactIO";
import type { SchemaWarning } from "../../../shared/generated/SchemaWarning";
import type { FileInfo } from "../../api/nodes";
import { artifactDownloadUrl, artifactUrl, portZipUrl } from "../../api/artifacts";
import ImageLightbox from "../../../shared/ui/ImageLightbox";
import { formatSize, isImageFile } from "../../model/nodeDetailFormat";
import { liveDisplayPath, liveUpdatedAgo, newestUpdate } from "../../model/liveArtifacts";

/**
 * ADR-0089: the live artifacts of the iteration on screen, polled with the I/O
 * while the node works — declared ones first (« not produced » until the agent
 * publishes them), then the ones it published freely. The frontmatter reads as
 * tags; a field that misses the declared schema is an amber tag, its rule on
 * hover. Nothing here is a verdict: a live artifact is never checked.
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
    <div className="border-t border-line" data-testid="live-section">
      <div
        className="flex items-center gap-1.5 px-3 py-1.5 text-text-3"
        style={{ fontSize: "11px" }}
      >
        Live artifacts
        <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>
          {live.length}
        </span>
        {newest && (
          <span
            className="ml-auto text-text-3"
            style={{ fontSize: "10px" }}
            title={newest}
            data-testid="live-updated"
          >
            updated {liveUpdatedAgo(newest)}
          </span>
        )}
      </div>
      <div className="flex flex-col gap-1 px-3 pb-2">
        {live.map((artifact) => (
          <LiveRow
            key={`${artifact.declared ? "d" : "f"}:${artifact.name}`}
            artifact={artifact}
            runId={runId}
            onOpen={() => onOpen(artifact)}
          />
        ))}
      </div>
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

  const gridStyle = { gridTemplateColumns: "8px 1fr auto", fontSize: "11.5px" };

  if (!produced) {
    return (
      <div
        data-testid="live-row"
        data-live={artifact.name}
        data-produced="false"
        className="grid items-center gap-2 rounded-md border border-line bg-surface-2 px-2.5 py-2 opacity-60"
        style={gridStyle}
        title={liveDisplayPath(artifact.path)}
      >
        <div className="h-2 w-2 rounded-full bg-text-3" />
        <div className="min-w-0">
          <span className="font-mono text-text-2" style={{ fontSize: "11.5px" }}>
            {artifact.name}
          </span>
          <div
            className="mt-0.5 font-mono text-text-3"
            style={{ fontSize: "10.5px" }}
            data-testid="live-not-produced"
          >
            not produced
          </div>
        </div>
        <div />
      </div>
    );
  }

  return (
    <>
      <div
        data-testid="live-row"
        data-live={artifact.name}
        data-produced="true"
        className="port-row group relative grid w-full items-center gap-2 rounded-md border border-line bg-surface-2 px-2.5 py-2 transition-colors hover:bg-hover"
        style={gridStyle}
      >
        <button
          type="button"
          onClick={onOpen}
          aria-label={`Open ${artifact.name}`}
          data-testid="live-row-open"
          className="absolute inset-0 cursor-pointer rounded-md focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-primary"
        />
        <div className="h-2 w-2 rounded-full bg-st-running" />
        <div className="min-w-0 text-left">
          <div className="flex items-center gap-1.5">
            <span className="font-mono text-text" style={{ fontSize: "11.5px" }}>
              {artifact.name}
            </span>
            {!artifact.declared && (
              <span
                className="rounded border border-line-strong bg-surface-2 px-1 py-px font-mono text-text-3"
                style={{ fontSize: "9px" }}
                title="Published by the node without being declared"
                data-testid="live-free-badge"
              >
                free
              </span>
            )}
          </div>
          <div className="mt-0.5 truncate font-mono text-text-3" style={{ fontSize: "10.5px" }}>
            {subline}
          </div>
        </div>
        <div className="flex items-center gap-2">
          {totalSize > 0 && (
            <span className="font-mono text-text-3" style={{ fontSize: "10px" }}>
              {formatSize(totalSize)}
            </span>
          )}
          {downloadHref && (
            <a
              href={downloadHref}
              download
              onClick={(e) => e.stopPropagation()}
              className="relative text-text-3 hover:text-text"
              aria-label={`Download ${artifact.name}`}
              title="Download"
              data-testid="live-download"
            >
              <Download size={11} />
            </a>
          )}
          <span className="font-mono text-text-3" style={{ fontSize: "10.5px" }} aria-hidden="true">
            ↗
          </span>
        </div>

        <FrontmatterTags frontmatter={frontmatter} warnings={artifact.schema_warnings} />

        {images.length > 0 && (
          <div className="col-span-3 mt-1 flex gap-1 overflow-x-auto" data-testid="live-thumbnails">
            {images.slice(0, 4).map((f, i) => (
              <img
                key={f.path}
                src={artifactUrl(runId, f.path)}
                alt={f.path.split("/").pop() ?? ""}
                className="relative h-12 w-12 cursor-zoom-in rounded border border-line object-cover transition-opacity hover:opacity-80"
                onClick={(e) => {
                  e.stopPropagation();
                  setLightbox({ images: images.map((im) => artifactUrl(runId, im.path)), index: i });
                }}
              />
            ))}
            {images.length > 4 && (
              <span
                className="flex h-12 w-12 items-center justify-center rounded border border-line bg-surface-0 font-mono text-text-3"
                style={{ fontSize: "10px" }}
              >
                +{images.length - 4}
              </span>
            )}
          </div>
        )}
      </div>
      {lightbox && (
        <ImageLightbox images={lightbox.images} index={lightbox.index} onClose={() => setLightbox(null)} />
      )}
    </>
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
    <div className="relative col-span-3 mt-1 flex flex-wrap gap-1" data-testid="live-frontmatter">
      {entries.map(([field, value]) => {
        const reason = byField.get(field);
        return (
          <span
            key={field}
            data-testid={reason ? "live-tag-warning" : "live-tag"}
            title={reason ? `Schema: ${reason}` : undefined}
            className={`inline-flex items-center gap-1 rounded border px-1.5 py-px font-mono ${
              reason
                ? "border-st-await/30 bg-st-await/14 text-st-await"
                : "border-line bg-surface-0 text-text-2"
            }`}
            style={{ fontSize: "10px" }}
          >
            {reason ? <TriangleAlert size={9} aria-label="Schema not respected" /> : (
              <span className="text-text-3">{field}</span>
            )}
            {reason ? `${field} ${display(value)}` : display(value)}
          </span>
        );
      })}
      {absent.map((w) => (
        <span
          key={`absent:${w.field}`}
          data-testid="live-tag-warning"
          title={`Schema: ${w.reason}`}
          className="inline-flex items-center gap-1 rounded border border-st-await/30 bg-st-await/14 px-1.5 py-px font-mono text-st-await"
          style={{ fontSize: "10px" }}
        >
          <TriangleAlert size={9} aria-label="Schema not respected" />
          {w.field === "(frontmatter)" ? "frontmatter" : `${w.field} missing`}
        </span>
      ))}
    </div>
  );
}

function display(value: unknown): string {
  return typeof value === "object" ? JSON.stringify(value) : String(value);
}
