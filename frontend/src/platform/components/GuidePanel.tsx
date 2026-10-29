import { useState } from "react";
import { BookOpen, Bot, ExternalLink, Hash, PanelRightClose } from "lucide-react";
import { IconButton } from "../../shared/ui/kit";
import type { GuideEntry } from "../../shared/generated/GuideEntry";
import type { PageMount } from "../../shared/generated/PageMount";
import { guideLinkLabel, guideTarget, isPdoTarget, type GuideTarget } from "../model/mountedPage";
import { useMountedPages } from "../store/mountedPages";

export interface GuideNavigation {
  /** The name a Run shows in the list, when the app knows it. */
  runName: (runId: string) => string | null;
  /** Open a place of the app the guide leads to (in its own tab). */
  openInPdo: (target: GuideTarget) => void;
}

/**
 * The reading guide of the page shown in the center, in the right pane: who wrote it,
 * then its numbered entries (title, text, link). A link into the app (↗) opens its target in
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
    <aside className="flex h-full flex-col bg-surface-1" aria-label="Reading guide" data-testid="guide-panel">
      <div className="d-bar gap-2">
        <BookOpen size={14} strokeWidth={1.5} className="shrink-0 text-primary" aria-hidden />
        <span className="font-medium text-text">Reading guide</span>
        <span className="d-chip tnum" data-testid="guide-count">
          {entries.length}
        </span>
        <span className="d-grow" />
        <IconButton
          icon={PanelRightClose}
          label="Fold the guide"
          onClick={() => setGuideOpen(false)}
          data-testid="guide-fold"
        />
      </div>
      <div className="px-3 pt-2 text-xs text-text-3">
        {page.run_id ? (
          <span
            className="flex min-w-0 items-center gap-1 whitespace-nowrap"
            title={`Mounted by the agent of ${page.node_id ? `node ${page.node_id} of ` : ""}« ${
              navigation.runName(page.run_id) ?? page.run_id
            } »`}
          >
            <Bot size={12} strokeWidth={1.5} className="shrink-0" aria-hidden />
            {page.node_id && <span className="shrink-0 font-mono text-text-2">{page.node_id}</span>}
            <span className="truncate">{navigation.runName(page.run_id) ?? page.run_id}</span>
          </span>
        ) : (
          <span>Mounted by hand</span>
        )}
      </div>
      <ol className="min-h-0 flex-1 overflow-y-auto px-1 pt-1 pb-3">
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
      className={`flex gap-2 border-l-2 px-1.5 py-1.5 ${current ? "border-selected-edge bg-selected" : "border-transparent"}`}
      data-testid={`guide-entry-${index}`}
      data-current={current || undefined}
    >
      <span
        className={`grid h-4 min-w-4 shrink-0 place-items-center rounded-full font-mono text-2xs tnum ${
          current ? "bg-primary text-primary-ink" : "bg-hover text-text-2"
        }`}
      >
        {index + 1}
      </span>
      <div className="flex min-w-0 flex-1 flex-col gap-0.5">
        <span className="font-medium break-words text-text">{entry.title}</span>
        {entry.text && <span className="whitespace-pre-line break-words text-sm text-text-2">{entry.text}</span>}
        <button
          type="button"
          onClick={onFollow}
          data-testid={`guide-link-${index}`}
          data-kind={pdo ? "pdo" : "page"}
          title={pdo ? `Open: ${entry.link}` : `Show in the page: ${entry.link}`}
          className={`flex h-5 w-fit max-w-full cursor-pointer items-center gap-1 text-xs ${
            pdo ? "text-st-running" : "text-text-2"
          } [&:hover>span]:underline [&:hover>span]:decoration-primary`}
        >
          {pdo ? (
            <ExternalLink size={11} strokeWidth={1.5} className="shrink-0" aria-hidden />
          ) : (
            <Hash size={11} strokeWidth={1.5} className="shrink-0" aria-hidden />
          )}
          <span className="truncate font-mono">{pdo ? `${label.kind} · ${label.where}` : label.where}</span>
        </button>
      </div>
    </li>
  );
}
