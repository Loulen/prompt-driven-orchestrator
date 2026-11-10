import { useMemo, useState } from "react";
import { ArrowUpRight, FileCode, FileImage, FileText, Files, Images, type LucideIcon } from "lucide-react";
import { Chip, IconButton } from "../../../shared/ui/kit";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import type { PortItemContext } from "../../../shared/extensions/contract";
import { artifactDownloadUrl, artifactUrl, fetchArtifact } from "../../api/artifacts";
import type { PortIO } from "../../api/nodes";
import ImageLightbox from "../../../shared/ui/ImageLightbox";
import { formatSize, isImageFile } from "../../model/nodeDetailFormat";
import CopyButton from "../artifacts/CopyButton";
import FrontmatterView from "../artifacts/FrontmatterView";
import { verdictFirst, verdictTone } from "../../model/frontmatter";
import { openFromAnywhere } from "./openFromAnywhere";
import FileChip from "./FileChip";
import PortDownloads from "./PortDownloads";
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
  // A `files` port: one file or more, of any format, each its own chip.
  const isFiles = portType === "files";
  // #41: an output row copies its main file (`output.*`, the first existing
  // one) of the iter the panel shows — `port.files` IS that iter's listing.
  // Images are out of scope, and a port with nothing on disk has nothing to copy.
  const copyTarget = kind === "output" && !isImage && !isFiles ? existing[0] : undefined;
  const { status: copyStatus, error: copyError, copy } = useCopyToClipboard();
  // An output is a card (the node's deliverable first); an input stays a compact row.
  const card = kind === "output";
  // An output the completion guard still waits for, by the daemon's own rule
  // (an image port holding only a non-image file is missing too): « missing »
  // at rest, next to its type. Approve's tooltip names the same outputs.
  const missing = card && port.missing;

  let displayPath = firstFile?.path ?? "";
  if (port.files.length > 1 && (port.repeated || isImage)) {
    displayPath = `${port.files.length} files`;
  }
  if (isFiles) {
    const names = existing.map((f) => f.path.split("/").pop() ?? f.path);
    displayPath =
      names.length === 0
        ? "no file yet"
        : `${names.length} ${names.length === 1 ? "file" : "files"} · ${names.join(", ")}`;
  }

  const totalSize = port.files.reduce((sum, f) => sum + (f.size ?? 0), 0);

  // An inherited file's frontmatter is a previous execution's verdict: no chip.
  const frontmatter =
    showFrontmatter && !isImage && !isFiles && firstFile?.frontmatter && !firstFile.inherited
      ? firstFile.frontmatter
      : null;
  // The verdict reads first: the daemon's map order is not the file's.
  const fields = frontmatter ? verdictFirst(frontmatter) : [];
  // The card's edge: the tone of its verdict-like value, if any.
  const tone = card ? fields.map(([, v]) => verdictTone(v)).find(Boolean) : undefined;

  const imageFiles = isImage
    ? port.files.filter((f) => f.exists && isImageFile(f.path))
    : [];
  // #779: a Start-sourced input carries the run's non-image attachments after
  // the prompt (`_input/<file>`). A single (non-repeated) port otherwise has
  // exactly one file, so anything past the first here IS an attachment.
  const attachmentFiles =
    !isImage && !isFiles && !port.repeated && port.files.length > 1
      ? port.files.slice(1).filter((f) => f.exists && !isImageFile(f.path))
      : [];
  const Icon = PORT_ICON[portType] ?? FileText;
  const tip = [port.port, portType, displayPath || (kind === "input" ? "not received" : "not written"), anyExists && totalSize > 0 ? formatSize(totalSize) : null]
    .filter(Boolean)
    .join(" · ");

  const lightboxEl = lightbox && (
    <ImageLightbox
      images={lightbox.images}
      index={lightbox.index}
      onClose={() => setLightbox(null)}
    />
  );

  const pendingCard = card && !anyExists;
  return (
    <div
      data-testid="port-row"
      data-port={port.port}
      data-kind={kind}
      data-pending={anyExists ? undefined : true}
      data-open={anyExists || undefined}
      data-st={tone}
      className={`port-row ri-port${card ? " ri-port--card" : ""}`}
      // A written port opens from anywhere on it; its controls keep their own clicks.
      onClick={anyExists ? openFromAnywhere(onOpen) : undefined}
    >
      <div className="ri-port__line">
        <Icon size={14} strokeWidth={1.5} className="ri-port__ic" aria-label={portType} role="img" />
        {anyExists ? (
          // The name opens the artifact too, from the keyboard.
          <button
            type="button"
            onClick={onOpen}
            data-testid="port-row-open"
            aria-label={`Open ${port.port}`}
            title={port.port}
            className="ri-port__name"
          >
            {port.port}
          </button>
        ) : (
          <span className="ri-port__name" title={port.port}>
            {port.port}
          </span>
        )}
        {port.repeated && <Chip mono>repeated</Chip>}
        {(isImage || isHtml || isFiles || pendingCard) && (
          <span className="d-chip d-chip--mono" data-testid="port-type-badge">
            {portType}
          </span>
        )}
        {inheritedOnly && (
          <span
            className="d-chip d-chip--mono"
            title="Present before this execution started: a previous execution's output"
            data-testid="inherited-badge"
          >
            inherited
          </span>
        )}
        {inheritedSome && (
          <span
            className="d-chip d-chip--mono"
            title={`${inheritedCount} of ${existing.length} files present before this execution started`}
            data-testid="inherited-badge"
          >
            {inheritedCount} inherited
          </span>
        )}
        <span className="contents" onClick={(e) => e.stopPropagation()}>
          <ExtensionDecorations<PortItemContext> location="port/item" extra={decorated} className="relative" />
        </span>
        {missing && (
          <span className="ri-port__missing" data-testid="port-missing">
            missing
          </span>
        )}
        <span className="ri-port__where" title={tip}>
          {pendingCard ? null : displayPath}
        </span>
        {anyExists && totalSize > 0 && <span className="ri-port__size">{formatSize(totalSize)}</span>}
        <span className="ri-port__tools">
          {copyTarget && (
            <CopyButton
              status={copyStatus}
              error={copyError}
              testId="port-copy"
              onClick={() => void copy(() => fetchArtifact(runId, copyTarget.path))}
            />
          )}
          <PortDownloads runId={runId} existing={existing} isImage={isImage} isFiles={isFiles} repeated={port.repeated} />
          {anyExists && <IconButton icon={ArrowUpRight} size="sm" label={`Open ${port.port}`} onClick={onOpen} />}
        </span>
      </div>

      {/* An output's whole frontmatter: a value copies, a cut one unfolds */}
      {fields.length > 0 && (
        <div className="ri-port__fm">
          <FrontmatterView values={fields} layout="rows" testId="frontmatter-card" />
        </div>
      )}

      {/* Image thumbnails */}
      {imageFiles.length > 0 && (
        <div className="ri-thumbs" data-testid="image-thumbnails">
          {imageFiles.slice(0, 4).map((f, i) => (
            <button
              key={f.path}
              type="button"
              className="ri-thumb"
              title={f.inherited ? "inherited: present before this execution started" : (f.path.split("/").pop() ?? "")}
              onClick={() => {
                // Open this thumbnail fullscreen. Snapshot the FULL imageFiles
                // list (not the four shown) so arrows reach images behind the
                // +N; a snapshot at click time is immune to poll churn.
                setLightbox({
                  images: imageFiles.map((im) => artifactUrl(runId, im.path)),
                  index: i,
                });
              }}
            >
              <img
                src={artifactUrl(runId, f.path)}
                alt={f.path.split("/").pop() ?? ""}
                data-inherited={f.inherited ? "true" : undefined}
                className={f.inherited ? "opacity-40 grayscale" : undefined}
                data-testid={`thumbnail-${i}`}
              />
            </button>
          ))}
          {imageFiles.length > 4 && <span className="self-center font-mono text-xs text-text-3">+{imageFiles.length - 4}</span>}
        </div>
      )}

      {/* A `files` port: one chip per file, each downloading it */}
      {isFiles && existing.length > 0 && (
        <div className="ri-port__sub" data-testid="port-files">
          {existing.map((f) => (
            <FileChip key={f.path} file={f} href={artifactDownloadUrl(runId, f.path)} download testId="port-file-chip" />
          ))}
        </div>
      )}

      {/* #779: input files — the chips of the New run dialog, read-only */}
      {attachmentFiles.length > 0 && (
        <div className="ri-port__sub" data-testid="input-files">
          <span className="d-caps">Input files</span>
          {attachmentFiles.map((f) => (
            <FileChip key={f.path} file={f} href={artifactUrl(runId, f.path)} testId="input-file-chip" />
          ))}
        </div>
      )}
      {lightboxEl}
    </div>
  );
}

const PORT_ICON: Record<string, LucideIcon> = {
  markdown: FileText,
  image: FileImage,
  image_list: Images,
  html: FileCode,
  files: Files,
};
