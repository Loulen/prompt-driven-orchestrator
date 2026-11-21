import { useEffect, useRef, useState } from "react";
import { BookOpen, Check, ExternalLink, Link2 } from "lucide-react";
import { fetchSettings } from "../../settings/api/settings";
import { fetchPageMounts } from "../api/pages";
import { copyToClipboard } from "../../shared/lib/clipboard";
import { toast } from "../../shared/ui/toast";
import { guidedPageUrl, pageUrl } from "../model/mountedPage";
import { useMountedPages } from "../store/mountedPages";
import { IconButton, Tooltip } from "../../shared/ui/kit";
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
          kind={<BookOpen size={13} strokeWidth={1.5} className="sh-tab__kind text-primary" aria-hidden />}
          label={<span className="font-mono">{tab.name}</span>}
          testId={`page-tab-${tab.name}`}
          labelTestId={`page-tab-label-${tab.name}`}
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
 * What the tab bar offers for the page shown: copy the link of PDO's view of it, under the
 * instance's Public URL (the machine's address by default), and open the page alone in a
 * browser tab.
 */
export function PageTabActions({ name }: { name: string }) {
  const [copied, setCopied] = useState(false);
  // Asked before the click, so the copy stays inside the user's gesture; a click that comes
  // first waits for it. `null`: the settings could not be read.
  const publicUrl = useRef<Promise<string | null> | null>(null);
  useEffect(() => {
    publicUrl.current = fetchSettings()
      .then((settings) => settings.public_url.effective)
      .catch(() => null);
  }, []);
  const copy = async () => {
    const base = await publicUrl.current;
    if (!base) {
      toast("The link could not be copied: the instance's Public URL could not be read.", "error");
      return;
    }
    if (await copyToClipboard(guidedPageUrl(base, name))) {
      setCopied(true);
      window.setTimeout(() => setCopied(false), 1500);
    } else {
      toast("The link could not be copied: the browser refused the clipboard.", "error");
    }
  };
  return (
    <>
      <IconButton
        icon={copied ? Check : Link2}
        label={copied ? "Copied" : "Copy link"}
        onClick={() => void copy()}
        data-testid="page-copy-link"
      />
      <Tooltip label="Open page alone">
        <a
          href={pageUrl("", name)}
          target="_blank"
          rel="noreferrer"
          aria-label="Open page alone"
          className="d-icon"
          data-testid="page-open-alone"
        >
          <ExternalLink size={14} strokeWidth={1.5} aria-hidden />
        </a>
      </Tooltip>
    </>
  );
}
