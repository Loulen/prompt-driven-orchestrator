import { useEffect, useState } from "react";
import Markdown from "react-markdown";
import { Download, FileImage, FileText, Files, Radio } from "lucide-react";
import { EmptyState, Tooltip } from "../../../shared/ui/kit";
import { REMARK_PLUGINS } from "../../../shared/markdown/plugins";
import MarkdownLink from "../../../shared/markdown/MarkdownLink";
import { formatSize } from "../../../run/model/nodeDetailFormat";
import { liveUpdatedAgo } from "../../../run/model/liveArtifacts";
import { stripFrontmatter } from "../../../run/model/frontmatter";
import { fetchHostedLiveArtifactText, hostedLiveArtifactUrl } from "../../api/hostedSessions";
import { isLive, livePreview, newestModification, type HostedLiveArtifact } from "../../model/hostedSession";
import { useHostedSession } from "../../store/useHostedSession";
import "../../../run/components/inspector.css";
import "./hostedSession.css";

const ICON = { markdown: FileText, image: FileImage, text: FileText, none: Files } as const;

/** Above this size an Artefact live is not previewed, only downloaded: it is
 *  read again at each change. */
const PREVIEW_LIMIT = 256 * 1024;

/** The last written first, then by path. */
function byNewest(live: HostedLiveArtifact[]): HostedLiveArtifact[] {
  return [...live].sort((a, b) => b.modified_at.localeCompare(a.modified_at) || a.path.localeCompare(b.path));
}

/**
 * The Artefacts live of a Session hébergée (CORE-125): the files its agent
 * writes in its `PDO_LIVE_DIR`, live, the last written first. A Markdown
 * file shows rendered, open; an image or a text file opens on its name; each
 * downloads.
 */
export default function LiveArtifacts({ session: id }: { session: string }) {
  const { live, session } = useHostedSession(id);
  const running = session ? isLive(session) : false;
  const newest = newestModification(live);
  return (
    <section className="hs-block" data-testid="hosted-live-artifacts">
      <div className="hs-block__head">
        <span className="hs-block__title">
          Live artifacts <span className="hs-block__hint">{live.length}</span>
        </span>
        {newest && (
          <span className="hs-block__hint" title={newest} data-testid="hosted-live-updated">
            updated {liveUpdatedAgo(newest)}
          </span>
        )}
      </div>
      {live.length === 0 ? (
        <EmptyState icon={Radio} layout="row">
          No live artifact yet
        </EmptyState>
      ) : (
        <div className="hs-live__list">
          {byNewest(live).map((artifact) => (
            <LiveRow key={artifact.path} session={id} artifact={artifact} running={running} />
          ))}
        </div>
      )}
    </section>
  );
}

function LiveRow({ session, artifact, running }: { session: string; artifact: HostedLiveArtifact; running: boolean }) {
  const preview = artifact.size > PREVIEW_LIMIT ? "none" : livePreview(artifact.path);
  const [open, setOpen] = useState(preview === "markdown");
  const name = artifact.path.split("/").pop() ?? artifact.path;
  const Icon = ICON[preview];
  return (
    <div className="hs-live__row" data-testid="hosted-live-row" data-path={artifact.path} data-open={open || undefined}>
      <div className="hs-live__line">
        <span className="ri-dot" data-live={running || undefined} aria-label={running ? "live" : "kept"} role="img" />
        <Icon size={14} strokeWidth={1.5} className="hs-live__ic" aria-hidden />
        <button
          type="button"
          className="hs-live__name"
          aria-expanded={preview === "none" ? undefined : open}
          disabled={preview === "none"}
          onClick={() => setOpen((o) => !o)}
          title={
            preview !== "none"
              ? `${open ? "Hide" : "Show"} ${artifact.path}`
              : artifact.size > PREVIEW_LIMIT
                ? `${artifact.path} · too large to preview: download it`
                : `${artifact.path} · no preview`
          }
          data-testid="hosted-live-toggle"
        >
          {name}
        </button>
        <span className="hs-live__size">{formatSize(artifact.size)}</span>
        <Tooltip label="Download">
          <a
            href={hostedLiveArtifactUrl(session, artifact.path, true)}
            download={name}
            className="d-icon d-icon--sm"
            aria-label={`Download ${artifact.path}`}
            data-testid="hosted-live-download"
          >
            <Download size={12} strokeWidth={1.5} aria-hidden />
          </a>
        </Tooltip>
        <span className="hs-live__where" title={artifact.modified_at}>
          live/{artifact.path} · {liveUpdatedAgo(artifact.modified_at)}
        </span>
      </div>
      {open && preview !== "none" && (
        <div className="hs-live__preview" data-testid="hosted-live-preview">
          {preview === "image" ? (
            <img
              src={`${hostedLiveArtifactUrl(session, artifact.path)}?v=${encodeURIComponent(artifact.modified_at)}`}
              alt={name}
            />
          ) : (
            <LiveText session={session} artifact={artifact} markdown={preview === "markdown"} />
          )}
        </div>
      )}
    </div>
  );
}

/** The content of a text Artefact live, read again each time it changes. */
function LiveText({ session, artifact, markdown }: { session: string; artifact: HostedLiveArtifact; markdown: boolean }) {
  const [text, setText] = useState<{ at: string; body: string | null; failed?: boolean } | null>(null);
  useEffect(() => {
    let alive = true;
    fetchHostedLiveArtifactText(session, artifact.path).then(
      (body) => alive && setText({ at: artifact.modified_at, body }),
      () => alive && setText({ at: artifact.modified_at, body: null, failed: true }),
    );
    return () => {
      alive = false;
    };
  }, [session, artifact.path, artifact.modified_at]);
  if (!text) return <span className="hs-block__hint">Loading…</span>;
  if (text.failed || text.body === null) return <span className="hs-block__hint">Could not read it</span>;
  if (!markdown) return <pre className="hs-live__text">{text.body}</pre>;
  return (
    <div className="artifact-markdown" data-testid="hosted-live-markdown">
      <Markdown remarkPlugins={REMARK_PLUGINS} components={{ a: MarkdownLink }}>
        {stripFrontmatter(text.body)}
      </Markdown>
    </div>
  );
}
