import { useEffect, useRef } from "react";
import { BookOpen } from "lucide-react";
import type { PageMount } from "../../shared/generated/PageMount";
import { pageUrl } from "../model/mountedPage";
import { showInFrame } from "../model/pageFrame";
import { useMountedPages } from "../store/mountedPages";

/**
 * A mounted page in its tab of the center panel: the page itself in a frame, served by
 * the daemon under `/pages/<name>/`. When the reader folds the guide, its rail stays on
 * the right edge (« Guide · N ») and unfolds it.
 */
export default function MountedPageView({ page }: { page: PageMount }) {
  const frameRef = useRef<HTMLIFrameElement>(null);
  const request = useMountedPages((s) => s.frameRequest);
  const guideOpen = useMountedPages((s) => s.guideOpen);
  const setGuideOpen = useMountedPages((s) => s.setGuideOpen);
  const entries = page.guide?.length ?? 0;

  useEffect(() => {
    const frame = frameRef.current;
    if (!frame || !request || request.name !== page.name) return;
    showInFrame(frame, page.name, request.link);
  }, [request, page.name]);

  return (
    <div className="flex min-h-0 flex-1" data-testid="mounted-page-view">
      <div className="flex min-h-0 min-w-0 flex-1 p-2">
        <iframe
          ref={frameRef}
          key={page.name}
          src={pageUrl("", page.name)}
          title={page.name}
          data-testid="mounted-page-frame"
          className="h-full w-full rounded-md border border-line bg-surface-0"
        />
      </div>
      {entries > 0 && !guideOpen && (
        <button
          type="button"
          onClick={() => setGuideOpen(true)}
          data-testid="guide-rail"
          title="Show the reading guide"
          className="flex w-[34px] shrink-0 cursor-pointer flex-col items-center gap-2 border-l border-line bg-surface-1 pt-3 text-text-3 transition-colors hover:text-text"
        >
          <BookOpen size={14} className="text-primary" />
          <span className="font-mono [writing-mode:vertical-rl]" style={{ fontSize: "10px" }}>
            Guide · {entries}
          </span>
        </button>
      )}
    </div>
  );
}
