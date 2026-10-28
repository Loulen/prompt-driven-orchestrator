import { useState } from "react";
import { BookOpen, ExternalLink, Hash, PanelRightClose } from "lucide-react";
import type { GuideEntry } from "../../shared/generated/GuideEntry";
import type { PageMount } from "../../shared/generated/PageMount";
import { guideLinkLabel, guideTarget, isPdoTarget, type GuideTarget } from "../model/mountedPage";
import { useMountedPages } from "../store/mountedPages";

export interface GuideNavigation {
  /** The name a Run shows in the list, when the app knows it. */
  runName: (runId: string) => string | null;
  /** Open a place of PDO the guide leads to (in its own tab). */
  openInPdo: (target: GuideTarget) => void;
}

/**
 * The reading guide of the page shown in the center, in the right pane: who wrote it,
 * then its numbered entries (title, text, link). A link into PDO (↗) opens its target in
 * another tab; a link into the page (#) moves the page's frame. The header folds the
 * guide into its rail, remembered on this browser.
 */
export default function GuidePanel({ page, navigation }: { page: PageMount; navigation: GuideNavigation }) {
  const setGuideOpen = useMountedPages((s) => s.setGuideOpen);
  const goInPage = useMountedPages((s) => s.goInPage);
  const [current, setCurrent] = useState<number | null>(null);
  const entries = page.guide ?? [];

  const follow = (entry: GuideEntry, index: number) => {
    setCurrent(index);
    const target = guideTarget(entry.link);
    if (isPdoTarget(target)) navigation.openInPdo(target);
    else goInPage(page.name, entry.link);
  };

  return (
    <aside className="flex h-full flex-col bg-surface-1" data-testid="guide-panel">
      <div className="flex h-8 shrink-0 items-center gap-2 border-b border-line px-3">
        <BookOpen size={13} className="text-primary" />
        <span className="font-medium text-text" style={{ fontSize: "12px" }}>
          Reading guide
        </span>
        <span
          className="rounded-full bg-surface-2 px-1.5 font-mono text-text-3"
          style={{ fontSize: "10px" }}
          data-testid="guide-count"
        >
          {entries.length}
        </span>
        <button
          type="button"
          onClick={() => setGuideOpen(false)}
          title="Fold the guide"
          aria-label="Fold the guide"
          data-testid="guide-fold"
          className="ml-auto cursor-pointer rounded p-1 text-text-3 transition-colors hover:bg-hover hover:text-text"
        >
          <PanelRightClose size={13} />
        </button>
      </div>
      <div className="px-3 pt-2.5 pb-1 text-text-3" style={{ fontSize: "10.5px" }}>
        {page.run_id ? (
          <>
            Written by the agent that mounted the page
            {page.node_id ? <> · node {page.node_id}</> : null} of «{" "}
            {navigation.runName(page.run_id) ?? page.run_id} ».
          </>
        ) : (
          <>Written by whoever mounted the page.</>
        )}
      </div>
      <ol className="min-h-0 flex-1 overflow-y-auto px-1.5 pb-3">
        {entries.map((entry, index) => (
          <GuideRow
            key={index}
            entry={entry}
            index={index}
            current={current === index}
            navigation={navigation}
            onFollow={() => follow(entry, index)}
          />
        ))}
      </ol>
    </aside>
  );
}

function GuideRow({
  entry,
  index,
  current,
  navigation,
  onFollow,
}: {
  entry: GuideEntry;
  index: number;
  current: boolean;
  navigation: GuideNavigation;
  onFollow: () => void;
}) {
  const target = guideTarget(entry.link);
  const pdo = isPdoTarget(target);
  const label = guideLinkLabel(target, navigation.runName);
  return (
    <li
      className={`mt-1 flex gap-2.5 rounded-md px-2 py-2 ${current ? "bg-selected" : ""}`}
      data-testid={`guide-entry-${index}`}
      data-current={current || undefined}
    >
      <span
        className={`mt-px flex h-[17px] w-[17px] shrink-0 items-center justify-center rounded-full font-mono ${
          current ? "bg-primary text-primary-ink" : "bg-surface-2 text-text-3"
        }`}
        style={{ fontSize: "9.5px" }}
      >
        {index + 1}
      </span>
      <div className="flex min-w-0 flex-col gap-1">
        <span className="font-medium text-text" style={{ fontSize: "11.5px" }}>
          {entry.title}
        </span>
        {entry.text && (
          <span className="whitespace-pre-line text-text-3" style={{ fontSize: "11px" }}>
            {entry.text}
          </span>
        )}
        <button
          type="button"
          onClick={onFollow}
          data-testid={`guide-link-${index}`}
          data-kind={pdo ? "pdo" : "page"}
          title={pdo ? `Open in PDO: ${entry.link}` : `Show in the page: ${entry.link}`}
          className={`flex w-fit max-w-full cursor-pointer items-center gap-1.5 truncate font-mono hover:underline ${
            pdo ? "text-st-running" : "text-text-2"
          }`}
          style={{ fontSize: "10.5px" }}
        >
          {pdo ? <ExternalLink size={10} className="shrink-0" /> : <Hash size={10} className="shrink-0 text-text-3" />}
          <span className="truncate">{pdo ? `${label.kind} · ${label.where}` : label.where}</span>
        </button>
      </div>
    </li>
  );
}
