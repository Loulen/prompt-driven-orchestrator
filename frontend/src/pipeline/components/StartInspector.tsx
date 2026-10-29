import { useEffect, useState } from "react";
import { ExternalLink, Play } from "lucide-react";
import { Chip, IconButton, Section } from "../../shared/ui/kit";
import "./inspect.css";
import type { StartNodeInfo } from "../../shared/generated/StartNodeInfo";
import { fetchArtifact, artifactUrl } from "../../run/api/artifacts";
import type { FileInfo } from "../../run/api/nodes";
import MarkdownArtifactModal from "../../run/components/MarkdownArtifactModal";
import ImageLightbox from "../../shared/ui/ImageLightbox";

interface Props {
  startNode: StartNodeInfo;
  runId: string;
  nodeId: string;
}

/** #779: the uppercase extension badge of an input-file chip. */
function startFileBadge(name: string): string {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return "FILE";
  return name.slice(dot + 1).toUpperCase().slice(0, 4);
}

export default function StartInspector({ startNode, runId, nodeId }: Props) {
  const [inputText, setInputText] = useState<string | null>(null);
  const [modalOpen, setModalOpen] = useState(false);
  // The ordered image list + clicked index currently shown fullscreen, or null
  // when the lightbox is closed (issue #145, arrow-nav #312).
  const [lightbox, setLightbox] = useState<{ images: string[]; index: number } | null>(null);

  // Images uploaded with the run are stored in `_input/` alongside the prompt.
  const inputImages = startNode.input_images ?? [];
  // #779: the non-image attachments, same folder.
  const inputFiles = startNode.input_files ?? [];

  useEffect(() => {
    let cancelled = false;

    fetchArtifact(runId, startNode.input_path)
      .then((text) => {
        if (!cancelled) setInputText(text);
      })
      .catch(() => {
        if (!cancelled) setInputText(null);
      });

    return () => {
      cancelled = true;
    };
  }, [runId, startNode.input_path]);

  const modalFiles: FileInfo[] = [
    {
      path: startNode.input_path,
      exists: inputText != null,
      size: inputText?.length ?? null,
      frontmatter: null,
    },
  ];

  return (
    <aside className="start-inspector ei" data-testid="start-inspector">
      <div className="ei-bar">
        <span className="ei-bar__title">
          <Play size={14} strokeWidth={1.5} aria-hidden />
          <span>Run start</span>
          <span className="ei-bar__sub">{nodeId}</span>
        </span>
        <span className="ei-bar__actions">
          <span className="runtime-badge" title="Filled in by the runtime">
            <Chip>runtime</Chip>
          </span>
          <IconButton
            icon={ExternalLink}
            label="View as markdown"
            className="view-markdown-link"
            onClick={() => setModalOpen(true)}
          />
        </span>
      </div>

      <Section title="Input" flush>
        <pre data-testid="start-input-text" className="start-input-text ei-quote">
          {inputText ?? <span className="ei-dim">Loading input…</span>}
        </pre>
      </Section>

      {inputImages.length > 0 && (
        <div data-testid="start-inspector-images">
          <Section title="Images" count={inputImages.length} flush>
            <div className="ei-thumbs">
              {inputImages.map((name, i) => {
                const src = artifactUrl(runId, `_input/${name}`);
                return (
                  <button
                    key={name}
                    type="button"
                    onClick={() =>
                      setLightbox({
                        images: inputImages.map((n) => artifactUrl(runId, `_input/${n}`)),
                        index: i,
                      })
                    }
                    title={name}
                    className="ei-thumb"
                  >
                    <img src={src} alt={name} data-testid="start-input-thumbnail" />
                  </button>
                );
              })}
            </div>
          </Section>
        </div>
      )}

      {inputFiles.length > 0 && (
        <div data-testid="start-inspector-files">
          <Section title="Files" count={inputFiles.length} flush>
            {inputFiles.map((name) => (
              <a
                key={name}
                href={artifactUrl(runId, `_input/${name}`)}
                target="_blank"
                rel="noreferrer"
                title={name}
                className="ei-row"
                data-testid="start-input-file"
              >
                <span className="ei-ext" aria-hidden="true">
                  {startFileBadge(name)}
                </span>
                <span className="ei-row__name">{name}</span>
              </a>
            ))}
          </Section>
        </div>
      )}

      {modalOpen && (
        <MarkdownArtifactModal
          runId={runId}
          portName="_input"
          source={{ kind: "static", files: modalFiles }}
          onClose={() => setModalOpen(false)}
        />
      )}

      {lightbox && (
        <ImageLightbox
          images={lightbox.images}
          index={lightbox.index}
          onClose={() => setLightbox(null)}
        />
      )}
    </aside>
  );
}
