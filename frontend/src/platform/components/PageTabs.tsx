import { useEffect, useState } from "react";
import { BookOpen, ExternalLink, Link2, X } from "lucide-react";
import { fetchSettings } from "../../settings/api/settings";
import { fetchPageMounts } from "../api/pages";
import { copyToClipboard } from "../../shared/lib/clipboard";
import { toast } from "../../shared/ui/toast";
import { guidedPageUrl, pageUrl } from "../model/mountedPage";
import { useMountedPages } from "../store/mountedPages";

/**
 * The tabs of the mounted pages, after the Run and pipeline tabs of the tab bar. Showing
 * a tab reads its mount again: a page unmounted since (its guide gone with it) closes
 * and says so, a guide written again shows its new entries.
 */
export function PageTabButtons() {
  const tabs = useMountedPages((s) => s.tabs);
  const active = useMountedPages((s) => s.active);
  const showPage = useMountedPages((s) => s.showPage);
  const closePage = useMountedPages((s) => s.closePage);
  const openPage = useMountedPages((s) => s.openPage);
  const show = async (name: string) => {
    showPage(name);
    let mounts;
    try {
      mounts = await fetchPageMounts();
    } catch {
      return; // Unreachable daemon: the tab shows what it last knew.
    }
    const mount = mounts.find((m) => m.name === name);
    if (useMountedPages.getState().active !== name) return;
    if (mount) {
      openPage(mount);
    } else {
      closePage(name);
      toast(`Page “${name}” is no longer mounted.`, "error");
    }
  };
  return (
    <>
      {tabs.map((tab) => {
        const isActive = tab.name === active;
        return (
          <button
            key={`page:${tab.name}`}
            type="button"
            onClick={() => void show(tab.name)}
            data-testid={`page-tab-${tab.name}`}
            data-active={isActive || undefined}
            title={`/pages/${tab.name}/`}
            className={`group flex shrink-0 cursor-pointer items-center gap-1.5 rounded-t-md border border-b-0 px-2.5 py-1 transition-colors ${
              isActive ? "border-line bg-bg-1 text-fg" : "border-transparent bg-bg-2 text-fg-3 hover:text-fg-2"
            }`}
            style={{ fontSize: "11px", maxWidth: 180 }}
          >
            <BookOpen size={11} className="shrink-0 text-acc" />
            <span className="truncate">{tab.name}</span>
            <span
              role="button"
              aria-label={`Close ${tab.name}`}
              data-testid={`page-tab-close-${tab.name}`}
              onClick={(e) => {
                e.stopPropagation();
                closePage(tab.name);
              }}
              className="ml-auto hidden shrink-0 cursor-pointer rounded p-0.5 text-fg-4 hover:bg-bg-3 hover:text-fg group-hover:inline-flex"
            >
              <X size={10} />
            </span>
          </button>
        );
      })}
    </>
  );
}

const ACTION =
  "flex cursor-pointer items-center gap-1 rounded-md border border-line-strong bg-bg-3 px-2 py-0.5 text-fg-2 transition-colors hover:bg-bg-4 hover:text-fg";

/**
 * What the tab bar offers for the page shown: copy the link of PDO's view of it (under
 * the instance's Public URL when one is set, else this browser's address), and open the
 * page alone in a browser tab.
 */
export function PageTabActions({ name }: { name: string }) {
  const [copied, setCopied] = useState(false);
  // Read before the click, so the copy stays inside the user's gesture.
  const [publicUrl, setPublicUrl] = useState<string | null>(null);
  useEffect(() => {
    let live = true;
    fetchSettings()
      .then((settings) => live && setPublicUrl(settings.public_url?.effective ?? null))
      .catch(() => {
        // Unreadable settings: this browser's address still works here.
      });
    return () => {
      live = false;
    };
  }, []);
  const copy = async () => {
    if (await copyToClipboard(guidedPageUrl(publicUrl ?? window.location.origin, name))) {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } else {
      toast("The link could not be copied: the browser refused the clipboard.", "error");
    }
  };
  return (
    <>
      <button type="button" onClick={() => void copy()} className={ACTION} style={{ fontSize: "11px" }} data-testid="page-copy-link">
        <Link2 size={11} />
        {copied ? "Copied" : "Copy link"}
      </button>
      <a
        href={pageUrl("", name)}
        target="_blank"
        rel="noreferrer"
        className={ACTION}
        style={{ fontSize: "11px" }}
        data-testid="page-open-alone"
      >
        <ExternalLink size={11} />
        Open page alone
      </a>
    </>
  );
}
