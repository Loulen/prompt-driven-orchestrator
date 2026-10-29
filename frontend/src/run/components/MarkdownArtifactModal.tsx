import { useCallback, useEffect, useMemo, useState } from "react";
import type { ReactNode } from "react";
import { artifactDownloadUrl, fetchArtifact } from "../api/artifacts";
import { fetchNodeIO, type FileInfo } from "../api/nodes";
import { registerTransientOverlay } from "../../lib/overlays";
import type { IterationInfo } from "../../shared/generated/IterationInfo";
import type { PortType } from "../../shared/generated/PortType";
import ImageLightbox from "../../shared/ui/ImageLightbox";
import { stripFrontmatter } from "../model/frontmatter";
import { filesPreviewKind } from "../model/nodeDetailFormat";
import ArtifactBody from "./artifacts/ArtifactBody";
import ArtifactHeader from "./artifacts/ArtifactHeader";
import CopyButton from "./artifacts/CopyButton";
import DownloadButton from "./artifacts/DownloadButton";
import { useCopyToClipboard } from "../store/useCopyToClipboard";
import { useMarkdownComponents } from "./artifacts/markdownComponents";
import { formatBytes } from "../model/transcript";

export type ArtifactSource =
  | { kind: "static"; files: FileInfo[] }
  | {
      kind: "iter-nav";
      nodeId: string;
      portKind: "input" | "output" | "live";
      iterations: IterationInfo[];
      initialIter: number;
    }
  // #698: an in-memory markdown document (the « What's new » changelog). Nothing is
  // fetched: `content` IS the body; `loading` shows the viewer's own loading text
  // while the caller is still fetching. `runId` is unused for this kind (pass `""`).
  | { kind: "inline"; content: string | null; loading?: boolean };

interface Props {
  runId: string;
  portName: string;
  portType?: PortType;
  source: ArtifactSource;
  onClose: () => void;
  /** #698: replaces the `portName` + path header block. The close X stays. */
  header?: ReactNode;
  /** #698: rendered at the top of the body, above the markdown. */
  banner?: ReactNode;
  /** #698: a fixed bar under the scrolling body, in place of the file's meta line. */
  footer?: ReactNode;
  /** #698: `l` 900 px for an artifact, `m` 680 px for « What's new ». */
  size?: "m" | "l";
  /** #825: an artifact modal is a *targetable* surface now — the *First run*
   *  tour ends on "open `out`", and « the file opened » is what says it did. */
  testId?: string;
}

export default function MarkdownArtifactModal({
  runId,
  portName,
  portType = "markdown",
  source,
  onClose,
  header,
  banner,
  footer,
  size = "l",
  testId = "artifact-modal",
}: Props) {
  const isInline = source.kind === "inline";
  const inlineContent = source.kind === "inline" ? source.content : null;
  const inlineLoading = source.kind === "inline" ? !!source.loading : false;
  const iterNumbers = useMemo(
    () =>
      source.kind === "iter-nav"
        ? source.iterations.map((it) => it.iter).sort((a, b) => a - b)
        : [],
    [source],
  );
  const [iter, setIter] = useState(
    source.kind === "iter-nav" ? source.initialIter : 0,
  );
  const [files, setFiles] = useState<FileInfo[]>(
    source.kind === "static" ? source.files : [],
  );
  const [fileIndex, setFileIndex] = useState(0);
  const [filesLoading, setFilesLoading] = useState(source.kind === "iter-nav");

  const changeIter = useCallback((newIter: number) => {
    setFilesLoading(true);
    setIter(newIter);
  }, []);

  // #369: key the fetch effect on the primitive fields it actually reads, NOT
  // on the `source` object reference. The parent (NodeDetailPanel) polls node
  // I/O and re-renders on every tick; if it hands us a fresh-but-equivalent
  // `source` object, depending on its identity would re-run this effect each
  // tick — re-fetching and calling `setFiles(new array)` + `setFileIndex(0)`,
  // which momentarily blanks the body and remounts the mermaid diagram (the
  // reported flicker). These primitives only change when the iter-nav target
  // genuinely does, so the effect (and file pagination) stays put under polling.
  const isIterNav = source.kind === "iter-nav";
  const navNodeId = source.kind === "iter-nav" ? source.nodeId : null;
  const navPortKind = source.kind === "iter-nav" ? source.portKind : null;

  useEffect(() => {
    if (!isIterNav || navNodeId == null || navPortKind == null) return;
    let cancelled = false;
    fetchNodeIO(runId, navNodeId, iter)
      .then((io) => {
        if (cancelled) return;
        // ADR-0089: a live artifact is read from the iteration's `live` list.
        const files =
          navPortKind === "live"
            ? io.live?.find((l) => l.name === portName)?.files
            : (navPortKind === "input" ? io.inputs : io.outputs).find((p) => p.port === portName)
                ?.files;
        setFiles(files ?? []);
        setFileIndex(0);
        setFilesLoading(false);
      })
      .catch(() => {
        if (cancelled) return;
        setFiles([]);
        setFileIndex(0);
        setFilesLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [isIterNav, navNodeId, navPortKind, runId, iter, portName]);

  const file = files[fileIndex];
  const total = files.length;
  const isRepeated = total > 1;

  const iterIndex =
    source.kind === "iter-nav" ? iterNumbers.indexOf(iter) : -1;
  const hasIterNav = source.kind === "iter-nav" && iterNumbers.length > 1;

  const isImage = portType === "image" || portType === "image_list";
  // #333: an html port renders its fetched text inside a sandboxed iframe. Like
  // markdown (and unlike image), its content flows through `fetchArtifact`, so
  // the fetch effect below (which only short-circuits on `isImage`) still runs.
  const isHtml = portType === "html";
  // A `files` port shows each file by its format: an image as an image, a text
  // file as text, anything else as its download.
  const isFiles = portType === "files";
  const filePreview = isFiles ? filesPreviewKind(file?.path) : null;
  const skipFetch = isImage || filePreview === "image" || filePreview === "none";
  const [fetchedContent, setContent] = useState<string | null>(null);
  // #41: which file `fetchedContent` belongs to. Paging to another file keeps
  // the previous text on screen until the new fetch lands; the copy button must
  // never write that previous file, so it is only armed when the paths agree.
  const [fetchedPath, setFetchedPath] = useState<string | null>(null);
  const [fetchedLoading, setLoading] = useState(!skipFetch && !isInline);
  // #698: an inline source never fetches — its content/loading are read straight from
  // the prop (the caller fetches, the viewer renders), no state to keep in sync.
  const content = isInline ? inlineContent : fetchedContent;
  // Paging keeps the previous file's text until the next one lands: a `files`
  // port's text preview waits for its own file instead of showing another's.
  const staleText = filePreview === "text" && fetchedPath !== file?.path && !!file?.exists;
  const loading = isInline ? inlineLoading : !skipFetch && (fetchedLoading || staleText);
  // The ordered image list + clicked index currently shown fullscreen in the
  // lightbox, or null when it is closed (#312).
  const [lightbox, setLightbox] = useState<{ images: string[]; index: number } | null>(null);

  useEffect(() => {
    if (skipFetch || isInline) return;

    let cancelled = false;

    (async () => {
      if (!file?.exists) {
        if (!cancelled) {
          setContent(null);
          setLoading(false);
        }
        return;
      }
      try {
        const text = await fetchArtifact(runId, file.path);
        if (!cancelled) {
          setContent(text);
          setFetchedPath(file.path);
          setLoading(false);
        }
      } catch {
        if (!cancelled) {
          setContent(null);
          setLoading(false);
        }
      }
    })();

    return () => {
      cancelled = true;
    };
  }, [runId, file?.path, file?.exists, skipFetch, isInline]);

  const goPrevIter = useCallback(() => {
    if (!hasIterNav || iterIndex <= 0) return;
    changeIter(iterNumbers[iterIndex - 1]);
  }, [hasIterNav, iterIndex, iterNumbers, changeIter]);

  const goNextIter = useCallback(() => {
    if (!hasIterNav || iterIndex < 0 || iterIndex >= iterNumbers.length - 1)
      return;
    changeIter(iterNumbers[iterIndex + 1]);
  }, [hasIterNav, iterIndex, iterNumbers, changeIter]);

  const handleKeyDown = useCallback(
    (e: KeyboardEvent) => {
      // While the lightbox is open it owns Escape (close lightbox, not the
      // modal) and the arrow keys (page images, not files/iters) — this guard
      // is the ONLY thing preventing double-navigation: both listeners are on
      // `window`, where stopPropagation between them is a no-op.
      if (lightbox !== null) return;
      if (e.key === "Escape") onClose();
      if (e.key === "ArrowLeft") {
        if (isRepeated && fileIndex > 0) setFileIndex(fileIndex - 1);
        else goPrevIter();
      }
      if (e.key === "ArrowRight") {
        if (isRepeated && fileIndex < total - 1) setFileIndex(fileIndex + 1);
        else goNextIter();
      }
    },
    [lightbox, onClose, isRepeated, fileIndex, total, goPrevIter, goNextIter],
  );

  useEffect(() => {
    window.addEventListener("keydown", handleKeyDown);
    return () => window.removeEventListener("keydown", handleKeyDown);
  }, [handleKeyDown]);

  // This modal's backdrop covers the window, so it blocks the app for as long as
  // it is up — including for whoever needs the app next. Declaring it dismissable
  // is how a guided tour starting on top of it gets a clear stage instead of a
  // frozen screen (`lib/overlays.ts`, #825). Nothing here knows what a tour is.
  useEffect(() => registerTransientOverlay(onClose), [onClose]);

  const zoom = useCallback(
    (images: string[], index: number) => setLightbox({ images, index }),
    [],
  );
  const markdownComponents = useMarkdownComponents(zoom);

  // #41: copy the displayed file's RAW text (frontmatter included — the strip
  // below is for rendering only; html copies its source). No copy for images
  // (out of scope) nor for an inline document (the changelog).
  const showCopy = !isImage && !isInline && !skipFetch;
  const copyReady =
    !filesLoading &&
    !loading &&
    !!file?.exists &&
    fetchedContent != null &&
    fetchedPath === file.path;
  const { status: copyStatus, error: copyError, copy } = useCopyToClipboard();

  // An inline document is rendered as given (a release note may open on a `---` rule).
  const bodyContent = content ? (isInline ? content : stripFrontmatter(content)) : null;

  // The meta line of the file shown: which node and iteration it comes from,
  // its kind and its size.
  const meta = [
    source.kind === "iter-nav" ? source.nodeId : null,
    source.kind === "iter-nav" ? `iter ${iter}` : null,
    portType,
    file?.size != null ? formatBytes(file.size) : null,
  ]
    .filter(Boolean)
    .join(" · ");

  return (
    <div
      className="art-scrim z-50"
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={`art ${size === "m" ? "art--m" : ""}`}
        data-testid={testId}
        data-port={portName}
        // #825: `role="dialog"` is what makes this modal an Escape *owner* while
        // it is up. Its own handler already closes it on Escape, on `window`,
        // where `stopPropagation` would be a no-op — without the role, one press
        // would close the artifact AND throw the running tour away with it.
        role="dialog"
        aria-modal="true"
        aria-label={portName}
      >
        <ArtifactHeader
          header={header}
          portName={portName}
          portType={portType}
          path={file?.path}
          hasIterNav={hasIterNav}
          iter={iter}
          iterIndex={iterIndex}
          iterNumbers={iterNumbers}
          goPrevIter={goPrevIter}
          goNextIter={goNextIter}
          isRepeated={isRepeated}
          fileIndex={fileIndex}
          total={total}
          setFileIndex={setFileIndex}
          downloadButton={
            !isInline && file?.exists ? (
              <DownloadButton
                href={artifactDownloadUrl(runId, file.path)}
                testId="artifact-download"
                size="md"
              />
            ) : undefined
          }
          copyButton={
            showCopy ? (
              <CopyButton
                status={copyStatus}
                error={copyError}
                testId="artifact-copy"
                size="md"
                disabled={!copyReady}
                onClick={() => {
                  if (copyReady && fetchedContent != null) void copy(fetchedContent);
                }}
              />
            ) : undefined
          }
          onClose={onClose}
        />

        <ArtifactBody
          runId={runId}
          portName={portName}
          portType={portType}
          isImage={isImage}
          isHtml={isHtml}
          isInline={isInline}
          filePreview={filePreview}
          files={files}
          filesLoading={filesLoading}
          fileIndex={fileIndex}
          file={file}
          loading={loading}
          content={content}
          bodyContent={bodyContent}
          banner={banner}
          markdownComponents={markdownComponents}
          onZoom={zoom}
        />

        {footer ? (
          <footer className="art__foot art__foot--bar">{footer}</footer>
        ) : (
          meta && (
            <footer className="art__foot" data-testid="artifact-meta">
              <span className="art__meta" title={file?.path}>
                {meta}
              </span>
            </footer>
          )
        )}
      </div>

      {lightbox && (
        <ImageLightbox
          images={lightbox.images}
          index={lightbox.index}
          onClose={() => setLightbox(null)}
        />
      )}
    </div>
  );
}
