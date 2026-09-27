import { useState } from "react";
import { artifactUrl } from "../../api/artifacts";
import type { PortIO } from "../../api/nodes";
import ImageLightbox from "../../../components/ImageLightbox";
import { fileBadge, formatSize, isImageFile } from "../../model/nodeDetailFormat";

export default function PortRow({
  port,
  kind,
  runId,
  showFrontmatter,
  onOpen,
}: {
  port: PortIO;
  kind: "input" | "output";
  runId: string;
  showFrontmatter?: boolean;
  onOpen: () => void;
}) {
  const firstFile = port.files[0];
  const anyExists = port.files.some((f) => f.exists);
  // #796: every file on disk predates the execution shown — the port carries a
  // previous session's result (same-iter re-spawn, or a child run whose branch
  // brought a previous run's artifacts in). Shown, but never as THIS result.
  const existing = port.files.filter((f) => f.exists);
  const inheritedCount = existing.filter((f) => f.inherited === true).length;
  const inheritedOnly = anyExists && inheritedCount === existing.length;
  // A multi-file port (image list) where only SOME files predate the execution:
  // the collapsed « N files » row would hide which ones — count them.
  const inheritedSome = inheritedCount > 0 && !inheritedOnly;
  const portType = port.port_type ?? "markdown";
  // The ordered image list + clicked index currently shown fullscreen in the
  // lightbox, or null when it is closed (#312).
  const [lightbox, setLightbox] = useState<{ images: string[]; index: number } | null>(null);
  const isImage = portType === "image" || portType === "image_list";
  // #333: an html port shows a type badge for parity with image ports (its
  // artifact is otherwise a single markdown-like file row).
  const isHtml = portType === "html";

  let dotClass = "bg-fg-5";
  if (inheritedOnly) {
    dotClass = "bg-fg-5";
  } else if (anyExists && port.repeated && port.files.length > 1) {
    dotClass = "bg-st-running";
  } else if (anyExists) {
    dotClass = "bg-st-done";
  }

  let displayPath = firstFile?.path ?? "";
  if (port.files.length > 1 && (port.repeated || isImage)) {
    displayPath = `${port.files.length} files`;
  }

  const totalSize = port.files.reduce((sum, f) => sum + (f.size ?? 0), 0);

  // An inherited file's frontmatter is a previous execution's verdict: no chip.
  const frontmatter =
    showFrontmatter && !isImage && firstFile?.frontmatter && !firstFile.inherited
      ? firstFile.frontmatter
      : null;

  const imageFiles = isImage
    ? port.files.filter((f) => f.exists && isImageFile(f.path))
    : [];
  // #779: a Start-sourced input carries the run's non-image attachments after
  // the prompt (`_input/<file>`). A single (non-repeated) port otherwise has
  // exactly one file, so anything past the first here IS an attachment.
  const attachmentFiles =
    !isImage && !port.repeated && port.files.length > 1
      ? port.files.slice(1).filter((f) => f.exists && !isImageFile(f.path))
      : [];

  const gridStyle = {
    gridTemplateColumns: "8px 1fr auto",
    fontSize: "11.5px",
  };

  const children = (
    <>
      {/* Status dot */}
      <div className={`h-2 w-2 rounded-full ${dotClass}`} />

      {/* Name + path */}
      <div className="min-w-0 text-left">
        <div className="flex items-center gap-1.5">
          <span className="font-mono text-fg" style={{ fontSize: "11.5px" }}>
            {port.port}
          </span>
          {port.repeated && (
            <span
              className="rounded border border-line-strong bg-bg-4 px-1 py-px font-mono text-fg-4"
              style={{ fontSize: "9px" }}
            >
              repeated
            </span>
          )}
          {(isImage || isHtml) && (
            <span
              className="rounded border border-line-strong bg-bg-4 px-1 py-px font-mono text-fg-4"
              style={{ fontSize: "9px" }}
              data-testid="port-type-badge"
            >
              {portType}
            </span>
          )}
          {inheritedOnly && (
            <span
              className="rounded border border-st-stopped/50 bg-bg-4 px-1 py-px font-mono text-st-stopped"
              style={{ fontSize: "9px" }}
              title="Present before this execution started — a previous execution's output, not this one's result"
              data-testid="inherited-badge"
            >
              inherited
            </span>
          )}
          {inheritedSome && (
            <span
              className="rounded border border-st-stopped/50 bg-bg-4 px-1 py-px font-mono text-st-stopped"
              style={{ fontSize: "9px" }}
              title={`${inheritedCount} of ${existing.length} files were present before this execution started — a previous execution's output, not this one's result`}
              data-testid="inherited-badge"
            >
              {inheritedCount} inherited
            </span>
          )}
        </div>
        <div
          className="mt-0.5 truncate font-mono text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          {displayPath}
        </div>
      </div>

      {/* Meta + arrow icon */}
      <div className="flex items-center gap-2">
        {anyExists && totalSize > 0 && (
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {formatSize(totalSize)}
          </span>
        )}
        {anyExists && (
          <span
            className="font-mono text-fg-3"
            style={{ fontSize: "10.5px" }}
            aria-hidden="true"
          >
            ↗
          </span>
        )}
      </div>

      {/* Image thumbnails */}
      {imageFiles.length > 0 && (
        <div
          className="col-span-3 mt-1 flex gap-1 overflow-x-auto"
          data-testid="image-thumbnails"
        >
          {imageFiles.slice(0, 4).map((f, i) => (
            <img
              key={f.path}
              src={artifactUrl(runId, f.path)}
              alt={f.path.split("/").pop() ?? ""}
              title={f.inherited ? "inherited — present before this execution started" : undefined}
              data-inherited={f.inherited ? "true" : undefined}
              className={`h-12 w-12 cursor-zoom-in rounded border border-line object-cover transition-opacity hover:opacity-80${
                f.inherited ? " opacity-40 grayscale" : ""
              }`}
              onClick={(e) => {
                // Open this thumbnail fullscreen instead of bubbling up to the
                // row button (which opens the artifact modal). Snapshot the
                // FULL imageFiles list (not the .slice(0,4) shown as
                // thumbnails) so arrows reach images behind the +N chip; `i`
                // is a valid index into the full array since the slice starts
                // at 0. A snapshot at click time is immune to poll churn
                // (NodeDetailPanel re-polls IO ~every 1s).
                e.stopPropagation();
                setLightbox({
                  images: imageFiles.map((im) => artifactUrl(runId, im.path)),
                  index: i,
                });
              }}
              data-testid={`thumbnail-${i}`}
            />
          ))}
          {imageFiles.length > 4 && (
            <span
              className="flex h-12 w-12 items-center justify-center rounded border border-line bg-bg-0 font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              +{imageFiles.length - 4}
            </span>
          )}
        </div>
      )}

      {/* #779: input files — the chips of the New run dialog, read-only */}
      {attachmentFiles.length > 0 && (
        <div className="col-span-3 mt-1 flex flex-col gap-1" data-testid="input-files">
          <div className="text-fg-4" style={{ fontSize: "10px", fontWeight: 500 }}>
            Input files
          </div>
          <div className="flex flex-wrap gap-1.5">
            {attachmentFiles.map((f) => {
              const name = f.path.split("/").pop() ?? f.path;
              return (
                <a
                  key={f.path}
                  href={artifactUrl(runId, f.path)}
                  target="_blank"
                  rel="noreferrer"
                  onClick={(e) => e.stopPropagation()}
                  className="flex h-9 max-w-[220px] items-center gap-1.5 rounded border border-line bg-bg-0 px-1.5 transition-colors hover:border-fg-4"
                  title={name}
                  data-testid="input-file-chip"
                >
                  <span
                    className="grid h-6 w-6 flex-shrink-0 place-items-center rounded border border-line bg-bg-3 font-mono font-medium text-fg-3"
                    style={{ fontSize: "7.5px" }}
                    aria-hidden="true"
                  >
                    {fileBadge(name)}
                  </span>
                  <span className="flex min-w-0 flex-col leading-tight">
                    <span className="truncate text-fg-2" style={{ fontSize: "10.5px" }}>
                      {name}
                    </span>
                    {f.size != null && (
                      <span className="font-mono text-fg-4" style={{ fontSize: "9px" }}>
                        {formatSize(f.size)}
                      </span>
                    )}
                  </span>
                </a>
              );
            })}
          </div>
        </div>
      )}

      {/* Frontmatter card (spans full width below) */}
      {frontmatter && Object.keys(frontmatter).length > 0 && (
        <div
          className="col-span-3 mt-1 grid rounded border border-line bg-bg-0 p-1.5 font-mono"
          data-testid="frontmatter-card"
          style={{
            fontSize: "10px",
            gridTemplateColumns: "auto 1fr",
            gap: "2px 8px",
          }}
        >
          {Object.entries(frontmatter).map(([k, v]) => (
            <FrontmatterKV key={k} field={k} value={v} />
          ))}
        </div>
      )}
    </>
  );

  const lightboxEl = lightbox && (
    <ImageLightbox
      images={lightbox.images}
      index={lightbox.index}
      onClose={() => setLightbox(null)}
    />
  );

  if (anyExists) {
    return (
      <>
        <button
          type="button"
          onClick={onOpen}
          data-testid="port-row"
          data-port={port.port}
          data-kind={kind}
          className="port-row grid w-full cursor-pointer items-center gap-2 rounded-md border border-line bg-bg-3 px-2.5 py-2 transition-colors hover:bg-bg-4"
          style={gridStyle}
        >
          {children}
        </button>
        {lightboxEl}
      </>
    );
  }

  return (
    <div
      data-testid="port-row"
      data-port={port.port}
      data-kind={kind}
      className="port-row grid items-center gap-2 rounded-md border border-line bg-bg-3 px-2.5 py-2 opacity-60"
      style={gridStyle}
    >
      {children}
      {lightboxEl}
    </div>
  );
}

function FrontmatterKV({ field, value }: { field: string; value: unknown }) {
  const display =
    typeof value === "object" ? JSON.stringify(value) : String(value);
  return (
    <>
      <span className="text-fg-3">{field}</span>
      <span className="text-fg">{display}</span>
    </>
  );
}
