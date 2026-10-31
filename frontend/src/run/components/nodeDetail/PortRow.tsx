import { useMemo, useState } from "react";
import { ArrowUpRight, FileCode, FileImage, FileText, Files, Images, type LucideIcon } from "lucide-react";
import { Chip, IconButton } from "../../../shared/ui/kit";
import type { StatusTone } from "../../../shared/ui/kit/tones";
import ExtensionDecorations from "../../../shared/extensions/ExtensionDecorations";
import type { PortItemContext } from "../../../shared/extensions/contract";
import { artifactDownloadUrl, artifactUrl, fetchArtifact } from "../../api/artifacts";
import type { PortIO } from "../../api/nodes";
import ImageLightbox from "../../../shared/ui/ImageLightbox";
import { formatSize, isImageFile } from "../../model/nodeDetailFormat";
import CopyButton from "../artifacts/CopyButton";
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

  return (
    <div
      data-testid="port-row"
      data-port={port.port}
      data-kind={kind}
      data-pending={anyExists ? undefined : true}
      className="port-row ri-port"
    >
      {/* A written port opens from anywhere on its line, as before; its tools
          keep their own clicks. */}
      <div className="ri-port__line" data-open={anyExists || undefined} onClick={anyExists ? onOpen : undefined}>
        <Icon size={14} strokeWidth={1.5} className="ri-port__ic" aria-label={portType} role="img" />
        {anyExists ? (
          // The name opens the artifact; the tools at the end keep their own clicks.
          <button
            type="button"
            onClick={(e) => {
              e.stopPropagation();
              onOpen();
            }}
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
        {(isImage || isHtml || isFiles) && (
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
        {frontmatter && Object.keys(frontmatter).length > 0 && <FrontmatterChips values={frontmatter} />}
        <span className="contents" onClick={(e) => e.stopPropagation()}>
          <ExtensionDecorations<PortItemContext> location="port/item" extra={decorated} className="relative" />
        </span>
        <span className="ri-port__where" title={tip}>
          {displayPath}
        </span>
        {anyExists && totalSize > 0 && <span className="ri-port__size">{formatSize(totalSize)}</span>}
        <span className="ri-port__tools" onClick={(e) => e.stopPropagation()}>
          {copyTarget && (
            <CopyButton
              status={copyStatus}
              error={copyError}
              testId="port-copy"
              onClick={(e) => {
                e.stopPropagation();
                void copy(() => fetchArtifact(runId, copyTarget.path));
              }}
            />
          )}
          <PortDownloads runId={runId} existing={existing} isImage={isImage} isFiles={isFiles} repeated={port.repeated} />
          {anyExists && <IconButton icon={ArrowUpRight} size="sm" label={`Open ${port.port}`} onClick={onOpen} />}
        </span>
      </div>

      {/* Image thumbnails */}
      {imageFiles.length > 0 && (
        <div className="ri-thumbs" data-testid="image-thumbnails">
          {imageFiles.slice(0, 4).map((f, i) => (
            <button
              key={f.path}
              type="button"
              className="ri-thumb"
              title={f.inherited ? "inherited: present before this execution started" : (f.path.split("/").pop() ?? "")}
              onClick={(e) => {
                // Open this thumbnail fullscreen. Snapshot the FULL imageFiles
                // list (not the four shown) so arrows reach images behind the
                // +N; a snapshot at click time is immune to poll churn.
                e.stopPropagation();
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

/** The verdicts a frontmatter value may carry, and their tone. */
const VERDICT_TONE: Record<string, StatusTone> = {
  pass: "done",
  passed: "done",
  approved: "done",
  ok: "done",
  done: "done",
  true: "done",
  fail: "failed",
  failed: "failed",
  rejected: "failed",
  needs_work: "awaiting",
  false: "failed",
};

const toneOf = (v: unknown) => VERDICT_TONE[display(v).toLowerCase()];

/** A frontmatter on one line: its verdict-like value (else its first key) as
 *  a tinted chip, a count of the other keys, every key in the tooltip. */
export function FrontmatterChips({ values }: { values: Record<string, unknown> }) {
  const entries = Object.entries(values);
  const [k, v] = entries.find(([, value]) => toneOf(value)) ?? entries[0];
  const others = entries.length - 1;
  return (
    <span className="ri-fm" data-testid="frontmatter-card" title={entries.map(([key, value]) => `${key}: ${display(value)}`).join("\n")}>
      <span className="ri-fm__chip" data-st={toneOf(v)}>
        <span className="ri-fm__k">{k}</span>
        <span>{display(v)}</span>
      </span>
      {others > 0 && <span className="ri-fm__chip ri-fm__more">+{others}</span>}
    </span>
  );
}

function display(value: unknown): string {
  return typeof value === "object" ? JSON.stringify(value) : String(value);
}
