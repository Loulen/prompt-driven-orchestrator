import { useMemo, useState } from "react";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import type { PortItemContext } from "../../../shared/extensions/contract";
import { artifactUrl, fetchArtifact } from "../../api/artifacts";
import type { PortIO } from "../../api/nodes";
import ImageLightbox from "../../../shared/ui/ImageLightbox";
import { fileBadge, formatSize, isImageFile } from "../../model/nodeDetailFormat";
import CopyButton from "../artifacts/CopyButton";
import { useCopyToClipboard } from "../../store/useCopyToClipboard";

export default function PortRow({
  port,
  kind,
  runId,
  nodeId = "",
  showFrontmatter,
  onOpen,
}: {
  port: PortIO;
  kind: "input" | "output";
  runId: string;
  nodeId?: string;
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
  // ADR-0081 (#31): what the `port/item` decorations mark.
  const decorated = useMemo(
    () => ({
      entity: { kind: "port", id: `${nodeId}/${port.port}`, name: port.port, attributes: {} },
      port: { run: runId, node: nodeId, name: port.port, direction: kind, type: portType },
    }),
    [nodeId, port.port, runId, kind, portType],
  );
  // The ordered image list + clicked index currently shown fullscreen in the
  // lightbox, or null when it is closed (#312).
  const [lightbox, setLightbox] = useState<{ images: string[]; index: number } | null>(null);
  const isImage = portType === "image" || portType === "image_list";
  // #333: an html port shows a type badge for parity with image ports (its
  // artifact is otherwise a single markdown-like file row).
  const isHtml = portType === "html";
  // #41: an output row copies its main file (`output.*`, the first existing
  // one) of the iter the panel shows — `port.files` IS that iter's listing.
  // Images are out of scope, and a port with nothing on disk has nothing to copy.
  const copyTarget = kind === "output" && !isImage ? existing[0] : undefined;
  const { status: copyStatus, error: copyError, copy } = useCopyToClipboard();

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
              className="relative rounded border border-st-stopped/50 bg-bg-4 px-1 py-px font-mono text-st-stopped"
              style={{ fontSize: "9px" }}
              title="Present before this execution started — a previous execution's output, not this one's result"
              data-testid="inherited-badge"
            >
              inherited
            </span>
          )}
          {inheritedSome && (
            <span
              className="relative rounded border border-st-stopped/50 bg-bg-4 px-1 py-px font-mono text-st-stopped"
              style={{ fontSize: "9px" }}
              title={`${inheritedCount} of ${existing.length} files were present before this execution started — a previous execution's output, not this one's result`}
              data-testid="inherited-badge"
            >
              {inheritedCount} inherited
            </span>
          )}
          {/* #41: lifted above the row's open target, so their own hover/click
              still reach them (the inherited badges' explanation, an extension mark). */}
          <ExtensionDecorations<PortItemContext>
            location="port/item"
            extra={decorated}
            className="relative"
          />
        </div>
        <div className="mt-0.5 truncate font-mono text-fg-3" style={{ fontSize: "10.5px" }}>
          {displayPath}
        </div>
      </div>

      {/* Meta + copy + arrow icon */}
      <div className="flex items-center gap-2">
        {anyExists && totalSize > 0 && (
          <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
            {formatSize(totalSize)}
          </span>
        )}
        {copyTarget && (
          <CopyButton
            status={copyStatus}
            error={copyError}
            testId="port-copy"
            // Hidden at rest, shown on row hover / keyboard focus, and kept up
            // for as long as the feedback lasts. `relative` lifts it above the
            // row's open target, so its click never opens the modal.
            className={`relative -my-1 ${
              copyStatus === "idle"
                ? "opacity-0 group-hover:opacity-100 group-focus-within:opacity-100"
                : ""
            }`}
            onClick={(e) => {
              e.stopPropagation();
              void copy(() => fetchArtifact(runId, copyTarget.path));
            }}
          />
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
              className={`relative h-12 w-12 cursor-zoom-in rounded border border-line object-cover transition-opacity hover:opacity-80${
                f.inherited ? " opacity-40 grayscale" : ""
              }`}
              onClick={(e) => {
                // Open this thumbnail fullscreen instead of bubbling up to the
                // row (whose open target opens the artifact modal). Snapshot the
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
                  className="relative flex h-9 max-w-[220px] items-center gap-1.5 rounded border border-line bg-bg-0 px-1.5 transition-colors hover:border-fg-4"
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
    // #41: the row holds two SIBLING targets — a button can't nest another.
    // The open button is stretched over the whole row (under its content), so a
    // click anywhere opens the artifact as before; the copy button, thumbnails
    // and file chips are positioned above it and keep their own clicks.
    return (
      <>
        <div
          data-testid="port-row"
          data-port={port.port}
          data-kind={kind}
          className="port-row group relative grid w-full items-center gap-2 rounded-md border border-line bg-bg-3 px-2.5 py-2 transition-colors hover:bg-bg-4"
          style={gridStyle}
        >
          <button
            type="button"
            onClick={onOpen}
            data-testid="port-row-open"
            aria-label={`Open ${port.port}`}
            className="absolute inset-0 cursor-pointer rounded-md focus-visible:outline-none focus-visible:ring-1 focus-visible:ring-acc"
          />
          {children}
        </div>
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
