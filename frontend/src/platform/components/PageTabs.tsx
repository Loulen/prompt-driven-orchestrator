import { useEffect, useState } from "react";
import { BookOpen, ExternalLink, Link2 } from "lucide-react";
import { fetchSettings } from "../../settings/api/settings";
import { fetchPageMounts } from "../api/pages";
import { copyToClipboard } from "../../shared/lib/clipboard";
import { toast } from "../../shared/ui/toast";
import { guidedPageUrl, pageUrl } from "../model/mountedPage";
import { useMountedPages } from "../store/mountedPages";
import { Button } from "../../shared/ui/kit";
import { DocTab } from "../../shared/ui/DocTab";
import type { TabDrag } from "../../shared/ui/useTabDrag";

/**
 * The tabs of the mounted pages, after the Run and pipeline tabs of the tab bar. Showing
 * a tab reads its mount again: a page unmounted since (its guide gone with it) closes
 * and says so, a guide written again shows its new entries.
 */
export function PageTabButtons({ drag }: { drag?: TabDrag } = {}) {
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
      {tabs.map((tab, index) => (
        <DocTab
          key={`page:${tab.name}`}
          active={tab.name === active}
          title={`/pages/${tab.name}/`}
          closeLabel={`Close ${tab.name}`}
          kind={<BookOpen size={13} strokeWidth={1.5} className="sh-tab__kind" aria-hidden />}
          label={tab.name}
          testId={`page-tab-${tab.name}`}
          closeTestId={`page-tab-close-${tab.name}`}
          drag={drag?.props("page", tab.name, index)}
          onActivate={() => void show(tab.name)}
          onClose={() => closePage(tab.name)}
        />
      ))}
    </>
  );
}

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
      <Button size="sm" icon={Link2} onClick={() => void copy()} data-testid="page-copy-link">
        {copied ? "Copied" : "Copy link"}
      </Button>
      <a
        href={pageUrl("", name)}
        target="_blank"
        rel="noreferrer"
        className="d-btn d-btn--sm"
        data-testid="page-open-alone"
      >
        <ExternalLink size={12} strokeWidth={1.5} aria-hidden />
        Open page alone
      </a>
    </>
  );
}
