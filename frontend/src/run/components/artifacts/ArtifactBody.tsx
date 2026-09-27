import type { ReactNode } from "react";
import Markdown from "react-markdown";
import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import type { FileInfo } from "../../api/nodes";
import type { PortType } from "../../../shared/generated/PortType";
import FrontmatterRow from "./FrontmatterRow";
import ImageBody from "./ImageBody";

// #369 (residual flicker): react-markdown remounts a custom component whenever the
// `components` entry at its position changes IDENTITY. Hoisting `remarkPlugins` to
// module scope (and memoising `components` per viewer instance) keeps those
// identities stable, so a poll-driven re-render no longer remounts the diagram.
const REMARK_PLUGINS = [remarkGfm];

/** The artifact viewer's body: the images, the html in its sandbox, or the
 *  frontmatter card and the rendered markdown. */
export default function ArtifactBody({
  runId,
  portName,
  portType,
  isImage,
  isHtml,
  isInline,
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
    <div className="flex-1 overflow-auto p-4">
      {isImage ? (
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
         *  - Uses raw `content` (fetched text), not `bodyContent` (which
         *    strips a leading `---` frontmatter fence html doesn't have).
         */
        filesLoading || loading ? (
          <span className="text-fg-4" style={{ fontSize: "11px" }}>
            Loading...
          </span>
        ) : file?.exists ? (
          <iframe
            data-testid="html-artifact-frame"
            sandbox=""
            srcDoc={content ?? ""}
            title={portName}
            className="h-[70vh] w-full rounded border border-line bg-white"
          />
        ) : (
          <span className="text-fg-4" style={{ fontSize: "11px" }}>
            File does not exist yet.
          </span>
        )
      ) : (
        <>
          {banner}
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
                remarkPlugins={REMARK_PLUGINS}
                components={markdownComponents}
              >
                {bodyContent}
              </Markdown>
            </div>
          ) : isInline ? null : (
            <span className="text-fg-4" style={{ fontSize: "11px" }}>
              {file?.exists ? "Could not load content." : "File does not exist yet."}
            </span>
          )}
        </>
      )}
    </div>
  );
}
