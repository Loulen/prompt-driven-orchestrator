import { useCallback, useEffect, useRef, useState } from "react";
import { cancelClone, fetchClone, fetchCloneTarget, startClone } from "../../../workspace/api/repos";
import { isRemoteUrl } from "../../../workspace/model/clone";
import type { CloneJob } from "../../../shared/generated/CloneJob";
import type { CloneTarget } from "../../../shared/generated/CloneTarget";
import type { Project } from "../../../workspace/model/project";
import type { NewRunState } from "./useNewRunState";

/** How often a running clone is read again. */
const POLL_MS = 500;

/** What New Run knows of the URL typed as the target repository. */
interface UrlState {
  /** The URL these values belong to: typing another one starts afresh. */
  url: string;
  target: CloneTarget | null;
  /** The folder the user typed after « change »; `null` → the target's. */
  path: string | null;
  job: CloneJob | null;
  error: string | null;
}

const fresh = (url: string): UrlState => ({ url, target: null, path: null, job: null, error: null });

/**
 * The target repository given by its URL (CORE-97): where it will be cloned,
 * the clone itself (« Clone and launch »), its progress and its Cancel. Once
 * cloned, the field holds the clone's path, the form validates it as any
 * repository, and the launch asked for goes on (`takeLaunchAfterClone`).
 * Typing elsewhere, or closing the form, drops that launch.
 */
export function useRepoClone(open: boolean, s: NewRunState, launchProject: Project | null) {
  const { targetRepo, handleRepoChange, loadPipelines } = s;
  const typed = targetRepo.trim();
  const isUrl = open && isRemoteUrl(typed);
  const [state, setState] = useState<UrlState>(() => fresh(""));
  // A launch asked with the clone of a URL (`launchFor`), then waiting on the
  // clone's path (`launchOn`) until the form validated it. Refs: taking one
  // must not render.
  const launchFor = useRef<string | null>(null);
  const launchOn = useRef<string | null>(null);
  const typedNow = useRef(typed);
  useEffect(() => {
    typedNow.current = typed;
    if (!open) {
      launchFor.current = null;
      launchOn.current = null;
    }
    if (launchFor.current !== null && launchFor.current !== typed) launchFor.current = null;
    if (launchOn.current !== null && launchOn.current !== typed) launchOn.current = null;
  }, [open, typed]);
  const current = state.url === typed ? state : fresh(typed);
  const update = useCallback(
    (url: string, change: Partial<UrlState>) =>
      setState((prev) => ({ ...(prev.url === url ? prev : fresh(url)), ...change })),
    [setState],
  );

  // Where the clone goes, asked again as the URL settles.
  useEffect(() => {
    if (!isUrl) return;
    let live = true;
    const timer = setTimeout(() => {
      fetchCloneTarget(typed)
        .then((target) => live && update(typed, { target }))
        .catch((e) => live && update(typed, { error: e instanceof Error ? e.message : String(e) }));
    }, 300);
    return () => {
      live = false;
      clearTimeout(timer);
    };
  }, [isUrl, typed, update]);

  // A clone done: the Projets are read again (it joined the launch Projet),
  // then the field holds the clone, which the form validates as any path.
  const finish = useCallback(
    async (url: string, job: CloneJob) => {
      update(url, { job });
      const launch = launchFor.current === url;
      launchFor.current = null;
      if (job.state === "done") {
        // The field moved on meanwhile: the clone is there, nothing else changes.
        if (typedNow.current !== url) return;
        await loadPipelines();
        if (typedNow.current !== url) return;
        launchOn.current = launch ? job.path : null;
        handleRepoChange(job.path);
      } else {
        if (job.state === "failed") update(url, { error: job.error ?? "The clone failed." });
      }
    },
    [update, loadPipelines, handleRepoChange],
  );

  // A running clone is read again until it ends.
  const jobId = current.job?.state === "running" ? current.job.id : null;
  const finishRef = useRef(finish);
  useEffect(() => {
    finishRef.current = finish;
  });
  useEffect(() => {
    if (!jobId) return;
    const url = typed;
    const timer = setInterval(() => {
      fetchClone(jobId)
        .then((job) => {
          if (job.state === "running") update(url, { job });
          else void finishRef.current(url, job);
        })
        .catch(() => {});
    }, POLL_MS);
    return () => clearInterval(timer);
  }, [jobId, typed, update]);

  /** Clone the URL; with `launch`, the Run is launched once the clone is there. */
  const clone = useCallback(
    async (launch: boolean) => {
      if (!isUrl) return;
      const url = typed;
      update(url, { error: null });
      launchFor.current = launch ? url : null;
      try {
        const job = await startClone({
          url,
          ...(current.path?.trim() ? { path: current.path.trim() } : {}),
          // The clone joins the Projet the Run is launched in: a Run of a
          // Projet works in one of its repositories (ADR-0087).
          ...(launchProject ? { project_id: launchProject.id } : {}),
        });
        if (job.state === "running") update(url, { job });
        else await finish(url, job);
      } catch (e) {
        launchFor.current = null;
        update(url, { error: e instanceof Error ? e.message : String(e) });
      }
    },
    [isUrl, typed, current.path, launchProject, update, finish],
  );

  const cancel = useCallback(() => {
    if (!current.job) return;
    launchFor.current = null;
    void cancelClone(current.job.id).catch(() => {});
  }, [current.job]);

  return {
    /** The target repository field holds a URL. */
    remoteUrl: isUrl,
    cloneTarget: isUrl ? current.target : null,
    /** The folder typed after « change », else `null`. */
    clonePath: current.path,
    setClonePath: (path: string | null) => update(typed, { path }),
    cloneJob: isUrl ? current.job : null,
    cloneError: isUrl ? current.error : null,
    cloning: isUrl && current.job?.state === "running",
    clone,
    cancelClone: cancel,
    /** Whether a launch waits on the clone at `path`; asking clears it. */
    takeLaunchAfterClone: (path: string) => {
      const asked = launchOn.current !== null && launchOn.current === path;
      if (asked) launchOn.current = null;
      return asked;
    },
  };
}

export type RepoClone = ReturnType<typeof useRepoClone>;
