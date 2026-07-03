import { useCallback, useEffect, useMemo, useState } from "react";
import { X, ChevronLeft, ChevronRight } from "lucide-react";
import Markdown from "react-markdown";
import remarkGfm from "remark-gfm";
import { fetchArtifact, fetchNodeIO, artifactUrl } from "../api";
import type { FileInfo } from "../api";
import type { IterationInfo, PortType } from "../types";
import type { Element } from "hast";
import ImageLightbox from "./ImageLightbox";
import MermaidDiagram from "./MermaidDiagram";

export type ArtifactSource =
  | { kind: "static"; files: FileInfo[] }
  | {
      kind: "iter-nav";
      nodeId: string;
      portKind: "input" | "output";
      iterations: IterationInfo[];
      initialIter: number;
    };

interface Props {
  runId: string;
  portName: string;
  portType?: PortType;
  source: ArtifactSource;
  onClose: () => void;
}

export default function MarkdownArtifactModal({
  runId,
  portName,
  portType = "markdown",
  source,
  onClose,
}: Props) {
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

  useEffect(() => {
    if (source.kind !== "iter-nav") return;
    let cancelled = false;
    fetchNodeIO(runId, source.nodeId, iter)
      .then((io) => {
        if (cancelled) return;
        const ports = source.portKind === "input" ? io.inputs : io.outputs;
        const port = ports.find((p) => p.port === portName);
        setFiles(port?.files ?? []);
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
  }, [source, runId, iter, portName]);

  const file = files[fileIndex];
  const total = files.length;
  const isRepeated = total > 1;

  const iterIndex =
    source.kind === "iter-nav" ? iterNumbers.indexOf(iter) : -1;
  const hasIterNav = source.kind === "iter-nav" && iterNumbers.length > 1;

  const isImage = portType === "image" || portType === "image_list";
  const [content, setContent] = useState<string | null>(null);
  const [loading, setLoading] = useState(!isImage);
  // The ordered image list + clicked index currently shown fullscreen in the
  // lightbox, or null when it is closed (#312).
  const [lightbox, setLightbox] = useState<{ images: string[]; index: number } | null>(null);

  useEffect(() => {
    if (isImage) return;

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
  }, [runId, file?.path, file?.exists, isImage]);

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

  const frontmatter = file?.frontmatter;
  const bodyContent = content ? stripFrontmatter(content) : null;

  return (
    <div
      className="fixed inset-0 z-50 grid place-items-center"
      style={{ background: "rgba(5,7,10,0.66)", backdropFilter: "blur(4px)" }}
      onClick={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className="flex max-h-[80vh] w-[560px] flex-col overflow-hidden rounded-lg border border-line-strong bg-bg-2"
        style={{ boxShadow: "0 30px 80px rgba(0,0,0,0.6)" }}
      >
        {/* Header */}
        <div className="flex items-center justify-between border-b border-line px-4 py-3">
          <div className="flex items-center gap-2">
            <span className="font-medium text-fg" style={{ fontSize: "13px" }}>
              {portName}
            </span>
            {file?.path && (
              <span
                className="font-mono text-fg-4"
                style={{ fontSize: "10px" }}
              >
                {file.path}
              </span>
            )}
          </div>
          <div className="flex items-center gap-2">
            {hasIterNav && (
              <div className="flex items-center gap-1" data-testid="iter-nav">
                <button
                  data-testid="iter-prev"
                  onClick={goPrevIter}
                  disabled={iterIndex <= 0}
                  className="rounded p-0.5 text-fg-3 hover:bg-bg-3 hover:text-fg disabled:text-fg-5"
                  aria-label="Previous iteration"
                >
                  <ChevronLeft size={14} />
                </button>
                <span
                  className="font-mono text-fg-3"
                  style={{ fontSize: "11px" }}
                >
                  iter {iter} of {iterNumbers[iterNumbers.length - 1]}
                </span>
                <button
                  data-testid="iter-next"
                  onClick={goNextIter}
                  disabled={iterIndex >= iterNumbers.length - 1}
                  className="rounded p-0.5 text-fg-3 hover:bg-bg-3 hover:text-fg disabled:text-fg-5"
                  aria-label="Next iteration"
                >
                  <ChevronRight size={14} />
                </button>
              </div>
            )}
            {isRepeated && (
              <div className="flex items-center gap-1">
                <button
                  onClick={() => setFileIndex(Math.max(0, fileIndex - 1))}
                  disabled={fileIndex === 0}
                  className="rounded p-0.5 text-fg-3 hover:bg-bg-3 hover:text-fg disabled:text-fg-5"
                  aria-label="Previous file"
                >
                  <ChevronLeft size={14} />
                </button>
                <span
                  className="font-mono text-fg-3"
                  style={{ fontSize: "11px" }}
                >
                  file {fileIndex + 1} of {total}
                </span>
                <button
                  onClick={() =>
                    setFileIndex(Math.min(total - 1, fileIndex + 1))
                  }
                  disabled={fileIndex === total - 1}
                  className="rounded p-0.5 text-fg-3 hover:bg-bg-3 hover:text-fg disabled:text-fg-5"
                  aria-label="Next file"
                >
                  <ChevronRight size={14} />
                </button>
              </div>
            )}
            <button
              onClick={onClose}
              className="rounded p-1 text-fg-3 hover:bg-bg-3 hover:text-fg"
            >
              <X size={14} />
            </button>
          </div>
        </div>

        {/* Body */}
        <div className="flex-1 overflow-auto p-4">
          {isImage ? (
            <ImageBody
              runId={runId}
              files={files}
              filesLoading={filesLoading}
              fileIndex={fileIndex}
              portType={portType}
              onZoom={(images, index) => setLightbox({ images, index })}
            />
          ) : (
            <>
              {/* Frontmatter card */}
              {frontmatter && Object.keys(frontmatter).length > 0 && (
                <div
                  className="mb-3 grid rounded border border-line bg-bg-0 p-2 font-mono"
                  style={{
                    fontSize: "10.5px",
                    gridTemplateColumns: "auto 1fr",
                    gap: "4px 10px",
                  }}
                >
                  {Object.entries(frontmatter).map(([k, v]) => (
                    <FrontmatterRow key={k} field={k} value={v} />
                  ))}
                </div>
              )}

              {/* Markdown body */}
              {filesLoading || loading ? (
                <span className="text-fg-4" style={{ fontSize: "11px" }}>
                  Loading...
                </span>
              ) : bodyContent ? (
                <div className="artifact-markdown prose-sm">
                  <Markdown
                    remarkPlugins={[remarkGfm]}
                    components={{
                      img: ({ src, alt }) => {
                        const url = typeof src === "string" ? src : undefined;
                        return (
                          <img
                            src={url}
                            alt={alt ?? ""}
                            className="cursor-zoom-in rounded transition-opacity hover:opacity-90"
                            onClick={() => {
                              // A markdown-embedded <img> is rendered in
                              // isolation by react-markdown — there is no
                              // collected list to page through, so it is an
                              // honest single-image set.
                              if (url) setLightbox({ images: [url], index: 0 });
                            }}
                          />
                        );
                      },
                      // A ```mermaid fenced block parses to
                      // <pre><code class="language-mermaid">src</code></pre>.
                      // Detect it on the child <code> and unwrap to a rendered
                      // SVG diagram; every other block falls through to a plain
                      // <pre>. We override `pre` (not `code`) to avoid emitting a
                      // <div> inside a <pre> (invalid nesting). Regular ```ts /
                      // ```bash fences are untouched. (#240)
                      pre: ({ node, children, ...rest }) => {
                        const child = node?.children?.[0];
                        const isMermaid =
                          child?.type === "element" &&
                          child.tagName === "code" &&
                          (
                            (child.properties?.className as string[]) ?? []
                          ).includes("language-mermaid");
                        if (isMermaid) {
                          const codeEl = child as Element;
                          const text = codeEl.children?.[0];
                          const source =
                            text && text.type === "text"
                              ? text.value.replace(/\n$/, "")
                              : "";
                          return <MermaidDiagram source={source} />;
                        }
                        return <pre {...rest}>{children}</pre>;
                      },
                    }}
                  >
                    {bodyContent}
                  </Markdown>
                </div>
              ) : (
                <span className="text-fg-4" style={{ fontSize: "11px" }}>
                  {file?.exists ? "Could not load content." : "File does not exist yet."}
                </span>
              )}
            </>
          )}
        </div>
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

function FrontmatterRow({ field, value }: { field: string; value: unknown }) {
  const display =
    typeof value === "object" ? JSON.stringify(value) : String(value);
  return (
    <>
      <span className="text-fg-3">{field}</span>
      <span className="text-fg">{display}</span>
    </>
  );
}

function ImageBody({
  runId,
  files,
  filesLoading,
  fileIndex,
  portType,
  onZoom,
}: {
  runId: string;
  files: FileInfo[];
  filesLoading: boolean;
  fileIndex: number;
  portType: PortType;
  onZoom: (images: string[], index: number) => void;
}) {
  const existingFiles = files.filter((f) => f.exists);

  if (filesLoading) {
    return (
      <span className="text-fg-4" style={{ fontSize: "11px" }}>
        Loading...
      </span>
    );
  }

  if (existingFiles.length === 0) {
    return (
      <span className="text-fg-4" style={{ fontSize: "11px" }}>
        No image files yet.
      </span>
    );
  }

  if (portType === "image_list") {
    return (
      <div className="flex flex-col gap-3" data-testid="image-gallery">
        {existingFiles.map((f, i) => (
          <div key={f.path} className="flex flex-col gap-1">
            <img
              src={artifactUrl(runId, f.path)}
              alt={f.path.split("/").pop() ?? ""}
              className="max-h-[60vh] w-full cursor-zoom-in rounded border border-line object-contain transition-opacity hover:opacity-90"
              onClick={() =>
                onZoom(
                  existingFiles.map((ef) => artifactUrl(runId, ef.path)),
                  i,
                )
              }
              data-testid={`gallery-image-${i}`}
            />
            <span
              className="font-mono text-fg-4"
              style={{ fontSize: "10px" }}
            >
              {f.path.split("/").pop()}
            </span>
          </div>
        ))}
      </div>
    );
  }

  const currentIdx = existingFiles[fileIndex] ? fileIndex : 0;
  const current = existingFiles[currentIdx];
  if (!current) return null;

  return (
    <div className="flex flex-col items-center gap-2" data-testid="image-viewer">
      <img
        src={artifactUrl(runId, current.path)}
        alt={current.path.split("/").pop() ?? ""}
        className="max-h-[60vh] max-w-full cursor-zoom-in rounded border border-line object-contain transition-opacity hover:opacity-90"
        onClick={() =>
          onZoom(
            existingFiles.map((f) => artifactUrl(runId, f.path)),
            currentIdx,
          )
        }
        data-testid="image-viewer-img"
      />
      <span className="font-mono text-fg-4" style={{ fontSize: "10px" }}>
        {current.path.split("/").pop()}
      </span>
    </div>
  );
}

function stripFrontmatter(content: string): string {
  const trimmed = content.trimStart();
  if (!trimmed.startsWith("---")) return content;
  const after = trimmed.slice(3);
  const end = after.indexOf("\n---");
  if (end === -1) return content;
  return after.slice(end + 4).trimStart();
}
