import { useEffect, useState } from "react";
import { fetchPipeline, fetchPipelines } from "../../../pipeline/api/pipelines";
import { fetchRun, fetchRuns } from "../../../run/api/runs";
import { fetchSessions } from "../../../session/api/sessions";
import { fetchSettings } from "../../../settings/api/settings";
import type { PipelineListEntry } from "../../../shared/generated/PipelineListEntry";
import type { RunListEntry } from "../../../shared/generated/RunListEntry";
import { CORE_NAME } from "../../../shared/ui/brand/brand";
import { fetchBugReportDestinations } from "../../api/bugReport";
import { fetchUpdateStatus } from "../../api/update";
import type { BugReportDestination, InstanceFacts } from "../../model/bugReport";
import { destinationsOf } from "../../model/bugReportDestinations";
import type { PipelineFacts, RunFacts } from "../../model/bugReportDetails";
import type { UpdateStatus } from "../../model/update";
import { installedVersionLabel, isDistribution } from "../../model/updateStatus";

/** The version a report states: the core's `PDO 2.81.0`, or a distribution's with its core. */
export function versionLabel(update: UpdateStatus | null, daemonVersion: string | undefined): string | null {
  if (update) return isDistribution(update) ? installedVersionLabel(update) : `${CORE_NAME} ${update.installed_version}`;
  return daemonVersion ? `${CORE_NAME} ${daemonVersion}` : null;
}

/**
 * The facts every report states, read once the window opens: the version
 * (`GET /update`, else `GET /sessions`), the daemon's system and its
 * installed harnesses with their versions (`GET /settings`). A read that
 * fails leaves its fact unknown.
 */
export function useInstanceFacts(): InstanceFacts | null {
  const [facts, setFacts] = useState<InstanceFacts | null>(null);
  useEffect(() => {
    let live = true;
    void Promise.all([
      fetchUpdateStatus().catch(() => null),
      fetchSessions().catch(() => null),
      fetchSettings().catch(() => null),
    ]).then(([update, sessions, settings]) => {
      if (!live) return;
      const harnesses = (settings?.harness_descriptors?.harnesses ?? [])
        .filter((h) => h.installed)
        .map((h) => ({ name: h.name, version: h.version ?? null }));
      setFacts({ version: versionLabel(update, sessions?.version), os: settings?.host_os ?? null, harnesses });
    });
    return () => {
      live = false;
    };
  }, []);
  return facts;
}

/** The Runs and the Pipelines the menus offer; empty until read, `pipelinesRead` once answered (or failed). */
export function useChoices(): { runs: RunListEntry[]; pipelines: PipelineListEntry[]; pipelinesRead: boolean } {
  const [runs, setRuns] = useState<RunListEntry[]>([]);
  const [pipelines, setPipelines] = useState<PipelineListEntry[] | null>(null);
  useEffect(() => {
    let live = true;
    fetchRuns().then((r) => live && setRuns(r), () => {});
    fetchPipelines().then(
      (p) => live && setPipelines(p),
      () => live && setPipelines([]),
    );
    return () => {
      live = false;
    };
  }, []);
  return { runs, pipelines: pipelines ?? [], pipelinesRead: pipelines !== null };
}

/** A detail being read: `undefined` while it loads, `null` when nothing is asked or the read failed. */
export type Loaded<T> = T | null | undefined;

/** One read per key: the latest answer for the latest key, `null` for no key. */
function useRead<T>(key: string | null, read: (key: string) => Promise<T>): Loaded<T> {
  const [state, setState] = useState<{ key: string | null; value: Loaded<T> }>({ key: null, value: null });
  useEffect(() => {
    if (!key) return;
    let live = true;
    read(key).then(
      (value) => live && setState({ key, value }),
      () => live && setState({ key, value: null }),
    );
    return () => {
      live = false;
    };
  }, [key, read]);
  if (!key) return null;
  return state.key === key ? state.value : undefined;
}

const readRun = (runId: string): Promise<RunFacts> => fetchRun(runId);

const readPipeline = (id: string): Promise<PipelineFacts> =>
  fetchPipeline(id).then((detail) => ({
    name: detail.pipeline.name,
    nodes: detail.pipeline.nodes.map((n) => ({
      id: n.id,
      name: n.name,
      type: n.type,
      pin_harness: n.pin_harness,
      agent_choice: n.agent_choice,
      isolated_worktree: n.isolated_worktree,
    })),
    edges: detail.pipeline.edges,
    prompts: detail.prompts,
  }));

/** The joined Run's details, read when one is chosen. */
export const useRunFacts = (runId: string | null) => useRead(runId, readRun);

/** The joined Pipeline's structure and prompts, read when one is chosen. */
export const usePipelineFacts = (pipelineId: string | null) => useRead(pipelineId, readPipeline);

/**
 * « Send to »: the core's destination, then those the extensions offer the
 * caller, and its last choice, read once the window opens. Until read, or
 * when the read fails, the core's alone.
 */
export function useDestinations(): { destinations: BugReportDestination[]; last: string | null } {
  const [answer, setAnswer] = useState<{ destinations: BugReportDestination[]; last: string | null }>(() => ({
    destinations: destinationsOf(null),
    last: null,
  }));
  useEffect(() => {
    let live = true;
    fetchBugReportDestinations().then(
      (read) => live && setAnswer({ destinations: destinationsOf(read), last: read.last }),
      () => {},
    );
    return () => {
      live = false;
    };
  }, []);
  return answer;
}
