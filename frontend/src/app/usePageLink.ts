import { useEffect } from "react";
import { dismissTransientOverlays } from "../lib/overlays";
import { fetchPageMounts } from "../platform/api/pages";
import { parsePageLink } from "../platform/model/mountedPage";
import { useActivePage, useMountedPages } from "../platform/store/mountedPages";
import type { PageMount } from "../shared/generated/PageMount";
import { usePages } from "../shared/extensions/deepLink";
import { toast } from "../shared/ui/toast";

/**
 * The mounted page in the app shell: the page tab shown in the center (another
 * pipeline or Run tab takes the center back), whether its guide shows in the right
 * pane, and its deep link followed.
 */
export function useMountedPageShell(editActiveTabId: string | null): {
  activePage: PageMount | null;
  guideShown: boolean;
} {
  const activePage = useActivePage();
  const guideOpen = useMountedPages((s) => s.guideOpen);
  const showPage = useMountedPages((s) => s.showPage);
  useEffect(() => showPage(null), [editActiveTabId, showPage]);
  usePageLink();
  return { activePage, guideShown: guideOpen && (activePage?.guide?.length ?? 0) > 0 };
}

/**
 * Follow the deep links toward a mounted page (`#/pages/<name>`, what `pdo page mount`
 * prints for a page with a guide): the page opens in its tab of the center panel, its
 * guide in the right pane, over any open extension page. A name no mount has says so in
 * an error toast. Either way the link leaves the address bar, like a Run's.
 */
export function usePageLink(): void {
  useEffect(() => {
    let latest = 0;
    const follow = async () => {
      const name = parsePageLink(window.location.hash);
      if (!name) return;
      const attempt = ++latest;
      let failure: string | null = null;
      let mount = null;
      try {
        mount = (await fetchPageMounts()).find((m) => m.name === name) ?? null;
        if (!mount) failure = `Page “${name}” is not mounted: this link can't be opened.`;
      } catch (cause) {
        failure = `Page “${name}” can't be opened: ${cause instanceof Error ? cause.message : String(cause)}`;
      }
      if (attempt !== latest || parsePageLink(window.location.hash) !== name) return;
      if (mount) {
        usePages.setState({ open: null, requested: null });
        dismissTransientOverlays();
        useMountedPages.getState().openPage(mount);
      } else if (failure) {
        toast(failure, "error");
      }
      const { pathname, search } = window.location;
      window.history.replaceState(window.history.state, "", `${pathname}${search}`);
    };
    void follow();
    window.addEventListener("hashchange", follow);
    window.addEventListener("popstate", follow);
    return () => {
      latest = -1;
      window.removeEventListener("hashchange", follow);
      window.removeEventListener("popstate", follow);
    };
  }, []);
}
