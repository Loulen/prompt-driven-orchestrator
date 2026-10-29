import type { ReactNode } from "react";
import Markdown from "react-markdown";
import { REMARK_PLUGINS } from "../../../shared/markdown/plugins";
import type { Components } from "react-markdown";
import type { FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";
import { artifactDownloadUrl, artifactUrl } from "../../api/artifacts";
import FrontmatterRow from "./FrontmatterRow";
import ImageBody from "./ImageBody";

/** The artifact viewer's body: the images, the html in its sandbox, one file
 *  of a `files` port, or the frontmatter card and the rendered markdown. */
export default function ArtifactBody({
  runId,
  portName,
  portType,
  isImage,
  isHtml,
  isInline,
  filePreview = null,
  files,
  filesLoading,
  fileIndex,
  file,
  loading,
  content,
  bodyContent,
  banner,
  markdownComponents,
  onZoom,
}: {
  runId: string;
  portName: string;
  portType: PortType;
  isImage: boolean;
  isHtml: boolean;
  isInline: boolean;
  /** A `files` port's file: how it previews (null for any other port). */
  filePreview?: "image" | "text" | "none" | null;
  files: FileInfo[];
  filesLoading: boolean;
  fileIndex: number;
  file: FileInfo | undefined;
  loading: boolean;
  content: string | null;
  bodyContent: string | null;
  banner?: ReactNode;
  markdownComponents: Components;
  onZoom: (images: string[], index: number) => void;
}) {
  const frontmatter = file?.frontmatter;
  return (
    <div className="art__body">
      {filePreview && filePreview !== "text" ? (
        filesLoading ? (
          <span className="art__state">
            Loading...
          </span>
        ) : !file?.exists ? (
          <span className="art__state">
            No file yet.
          </span>
        ) : filePreview === "image" ? (
          <img
            src={artifactUrl(runId, file.path)}
            alt={file.path.split("/").pop() ?? ""}
            className="art__read max-w-full cursor-zoom-in border border-line"
            onClick={() => onZoom([artifactUrl(runId, file.path)], 0)}
            data-testid="files-image"
          />
        ) : (
          <div className="flex flex-col items-start gap-2" data-testid="files-no-preview">
            <span className="art__state">
              No preview for this format.
            </span>
            <a href={artifactDownloadUrl(runId, file.path)} download className="d-btn">
              Download {file.path.split("/").pop()}
            </a>
          </div>
        )
      ) : filePreview === "text" && !/\.(md|markdown)$/i.test(file?.path ?? "") ? (
        filesLoading || loading ? (
          <span className="art__state">
            Loading...
          </span>
        ) : (
          <pre className="art__text" data-testid="files-text">
            {content ?? (file?.exists ? "Could not load content." : "No file yet.")}
          </pre>
        )
      ) : isImage ? (
        <ImageBody
          runId={runId}
          files={files}
          filesLoading={filesLoading}
          fileIndex={fileIndex}
          portType={portType}
          onZoom={onZoom}
        />
      ) : isHtml ? (
        /*
         * #333 / ADR-0028: render the html artifact in a maximally isolated
         * iframe. SECURITY INVARIANTS — DO NOT RELAX:
         *  - `sandbox=""` is a literal empty allow-list: no scripts, opaque
         *    origin, no form submission, no top-level navigation. NEVER add
         *    `allow-scripts` / `allow-forms` / `allow-same-origin` — that
         *    would let agent-authored (prompt-injectable) HTML execute.
         *  - The HTML is passed via the `srcDoc` PROP, never
         *    `dangerouslySetInnerHTML` and never concatenated into the
         *    parent DOM.
         *  - The page is white whatever the theme, as a browser shows a
         *    document that sets no background.
         *  - Uses raw `content` (fetched text), not `bodyContent` (which
         *    strips a leading `---` frontmatter fence html doesn't have).
         */
        filesLoading || loading ? (
          <span className="art__state">
            Loading...
          </span>
        ) : file?.exists ? (
          <iframe
            data-testid="html-artifact-frame"
            sandbox=""
            srcDoc={content ?? ""}
            title={portName}
            className="h-[calc(70vh/var(--app-zoom,1))] w-full border border-line bg-[white]"
          />
        ) : (
          <span className="art__state">
            File does not exist yet.
          </span>
        )
      ) : (
        <>
          {banner}
          {/* Frontmatter card */}
          {frontmatter && Object.keys(frontmatter).length > 0 && (
            <dl className="art__fm" data-testid="artifact-frontmatter">
              {Object.entries(frontmatter).map(([k, v]) => (
                <FrontmatterRow key={k} field={k} value={v} />
              ))}
            </dl>
          )}

          {/* Markdown body */}
          {filesLoading || loading ? (
            <span className="art__state">
              Loading...
            </span>
          ) : bodyContent ? (
            <div className="artifact-markdown art__read">
              <Markdown
                remarkPlugins={REMARK_PLUGINS}
                components={markdownComponents}
              >
                {bodyContent}
              </Markdown>
            </div>
          ) : isInline ? null : (
            <span className="art__state">
              {file?.exists ? "Could not load content." : "File does not exist yet."}
            </span>
          )}
        </>
      )}
    </div>
  );
}
