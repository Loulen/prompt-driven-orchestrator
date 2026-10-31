/**
 * The core's rows of the command field: its commands (New run, Stats,
 * Settings, the Theme, the Dark mode and Compact nodes…), its Runs, Pipelines and Triggers, and the gestures
 * on the selected item (a Run's Pause, Resume, Retry all; a node's Mark
 * complete, Retry and Stop) that the caller may make (CORE-42: a refused gesture is absent). And
 * the extensions' Actions, as rows.
 */

import {
  CircleCheck,
  Copy,
  FoldVertical,
  History,
  MousePointerClick,
  Palette,
  Pause,
  Play,
  Plus,
  RotateCcw,
  Square,
  SquareChevronRight,
  SunMoon,
  Workflow,
  Zap,
} from "lucide-react";
import { iconOf } from "../../shared/extensions/useActions";
import { runCommand } from "../../shared/extensions/commands";
import type { PaletteAction } from "../../shared/extensions/palette";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import { isRefusal, toastRefusal } from "../../shared/authorization/refusal";
import { copyToClipboard } from "../../shared/lib/clipboard";
import { toast } from "../../shared/ui/toast";
import { SettingsIcon, StatsIcon } from "../../shared/ui/brand/AccentIcons";
import { pauseRun, resumeRun, retryAll } from "../../run/api/commands";
import { retryNode, retryNodePreview, stopNode } from "../../run/api/nodes";
import { markNodeDone } from "../../run/api/completion";
import { mayOnNode, mayOnRun, type GestureRights } from "../../run/model/runGestures";
import { isMarkCompletable } from "../../run/model/nodeStatus";
import type { Icon } from "../../shared/ui/kit";
import type { Appearance, Theme } from "../../lib/theme";
import type { BuildInput, Lead, PaletteItem } from "./paletteModel";
import type { PaletteHost, PaletteSelection } from "./paletteHost";
import type { PaletteConfirm } from "./paletteStore";

export const GROUP_ICONS: BuildInput["icons"] = {
  selection: MousePointerClick,
  recent: History,
  commands: SquareChevronRight,
  runs: Play,
  pipelines: Workflow,
  triggers: Zap,
};

export interface CoreContext {
  host: PaletteHost;
  pipelines: PipelineListEntry[];
  openPipeline: (id: string) => void;
  appearance: Appearance;
  setTheme: (theme: Theme) => void;
  toggleMode: () => void;
  compactNodes: boolean;
  toggleCompactNodes: () => void;
}

const icon = (glyph: Icon): Lead => ({ kind: "icon", icon: glyph });

/** The core's commands, Runs, Pipelines and Triggers. */
export function coreItems({ host, pipelines, openPipeline, appearance, setTheme, toggleMode, compactNodes, toggleCompactNodes }: CoreContext): PaletteItem[] {
  const items: PaletteItem[] = [];
  const command = (id: string, label: string, glyph: Icon, run: () => void, extra: Partial<PaletteItem> = {}) =>
    items.push({ id: `cmd:${id}`, group: "commands", label, lead: icon(glyph), run, command: true, ...extra });
  if (host.newRun) command("new-run", "New run", Plus, host.newRun);
  if (host.newTrigger) command("new-trigger", "New trigger", Zap, host.newTrigger);
  if (host.openStats) command("stats", "Stats", StatsIcon, host.openStats);
  if (host.openSettings) command("settings", "Settings", SettingsIcon, host.openSettings);
  // ADR-0093: one row per Theme, and the Dark mode switch; each checked while it holds.
  command("theme-acogni", "Theme: Acogni", Palette, () => setTheme("acogni"), { checked: appearance.theme === "acogni" });
  command("theme-classic", "Theme: Classic", Palette, () => setTheme("classic"), { checked: appearance.theme === "classic" });
  command("dark-mode", "Dark mode", SunMoon, toggleMode, { checked: appearance.mode === "dark" });
  command("compact-nodes", "Compact nodes", FoldVertical, toggleCompactNodes, { checked: compactNodes });
  for (const run of host.runs) {
    items.push({
      id: `run:${run.run_id}`,
      group: "runs",
      label: run.name || run.run_id,
      meta: [run.pipeline_name, run.run_id].join(" "),
      lead: { kind: "run", status: run.stalled ? "stalled" : run.status },
      recent: { kind: "run", id: run.run_id },
      run: () => host.openRun(run.run_id),
    });
  }
  for (const pipeline of pipelines) {
    items.push({
      id: `pipeline:${pipeline.id}`,
      group: "pipelines",
      label: pipeline.name,
      lead: icon(Workflow),
      recent: { kind: "pipeline", id: pipeline.id },
      run: () => openPipeline(pipeline.id),
    });
  }
  for (const trigger of host.triggers) {
    items.push({
      id: `trigger:${trigger.id}`,
      group: "triggers",
      label: trigger.name,
      meta: trigger.pipeline_name,
      lead: icon(Zap),
      recent: { kind: "trigger", id: trigger.id },
      run: () => host.openTrigger(trigger.id),
    });
  }
  return items;
}

const LIVE = new Set(["running", "awaiting_user"]);
const NODE_RETRYABLE = new Set(["failed", "stale", "stopped", "interrupted", "completed"]);
const NODE_STOPPABLE = new Set(["running", "stale", "awaiting_user"]);

/** What a contextual row shows of the selected item. */
export function selectionEntity(selection: PaletteSelection): PaletteItem["entity"] {
  switch (selection.kind) {
    case "run":
      return { label: selection.run.name || selection.run.id, lead: { kind: "run", status: selection.run.status } };
    case "node":
      return { label: selection.node.name || selection.node.id, lead: { kind: "node", status: selection.node.status } };
    case "trigger":
      return { label: selection.trigger.name, lead: icon(Zap) };
    case "pipeline":
      return { label: selection.pipeline.name, lead: icon(Workflow) };
  }
}

/**
 * The core's gestures on the selected item the caller may make: on a node,
 * Mark complete (exactly where the Inspector offers it), Retry and Stop; on
 * its Run (or a selected Run), Pause, Resume, Retry all and Copy run id.
 */
export function selectionItems(
  selection: PaletteSelection | null,
  rights: GestureRights,
  confirm: (request: PaletteConfirm) => void,
  openRun: (runId: string) => void,
): PaletteItem[] {
  if (!selection || (selection.kind !== "run" && selection.kind !== "node")) return [];
  const entity = selectionEntity(selection);
  const items: PaletteItem[] = [];
  const add = (id: string, label: string, glyph: Icon, run: () => void, extra: Partial<PaletteItem> = {}) =>
    items.push({ id: `sel:${id}`, group: "selection", label, lead: icon(glyph), entity, command: true, run, ...extra });
  const runId = selection.run.id;
  const runStatus = selection.run.status;

  if (selection.kind === "node") {
    const { node } = selection;
    if (isMarkCompletable(node.status, runStatus) && mayOnNode(rights, runId, node.id, "mark_node_done")) {
      add("node-done", "Mark complete", CircleCheck, () => void markComplete(runId, node.id, node.iter));
    }
    if (NODE_RETRYABLE.has(node.status) && runStatus !== "paused" && runStatus !== "halted" && runStatus !== "archived" && mayOnNode(rights, runId, node.id, "retry")) {
      add("node-retry", "Retry", RotateCcw, () => void retryWithPreview(runId, node.id, confirm));
    }
    if (NODE_STOPPABLE.has(node.status) && mayOnNode(rights, runId, node.id, "stop")) {
      add("node-stop", "Stop", Square, () => void stopNode(runId, node.id).catch(report), { danger: true });
    }
  }
  if (LIVE.has(runStatus) && mayOnRun(rights, runId, "pause_run")) {
    add("run-pause", "Pause", Pause, () => void pauseRun(runId).catch(report));
  }
  if (runStatus === "paused" && mayOnRun(rights, runId, "resume_run")) {
    add("run-resume", "Resume", Play, () => void resumeRun(runId).catch(report));
  }
  if ((runStatus === "failed" || runStatus === "halted") && mayOnRun(rights, runId, "retry_all")) {
    add("run-retry-all", "Retry all", RotateCcw, () =>
      confirm({
        title: "Retry all nodes?",
        consequence: "Archives this run",
        target: runId,
        actionLabel: "Retry all",
        run: () =>
          void retryAll(runId).then(
            ({ run_id }) => openRun(run_id),
            report,
          ),
      }),
    );
  }
  add("run-copy-id", "Copy run id", Copy, () =>
    void copyToClipboard(runId).then((ok) =>
      ok ? toast("Run id copied") : toast("The clipboard is not available", "error"),
    ),
  );
  return items;
}

/** A node's Retry: asked first when it resets downstream nodes with artifacts, as the panel's. */
async function retryWithPreview(runId: string, nodeId: string, confirm: (request: PaletteConfirm) => void) {
  const retry = () => void retryNode(runId, nodeId).catch(report);
  try {
    const preview = await retryNodePreview(runId, nodeId);
    if (preview.affected_count === 0) return retry();
    const n = preview.affected_count;
    confirm({
      title: "Retry this node?",
      consequence: `Resets ${n} downstream ${n === 1 ? "node" : "nodes"}`,
      target: nodeId,
      actionLabel: "Retry",
      run: retry,
    });
  } catch (e) {
    report(e);
  }
}

/** A node's Mark complete: its refusal (missing outputs…) said in a toast. */
async function markComplete(runId: string, nodeId: string, iter: number) {
  try {
    const outcome = await markNodeDone(runId, nodeId, iter);
    if (outcome.kind === "refused") toast(outcome.message, "error");
  } catch (e) {
    report(e);
  }
}

/** A gesture that failed: its refusal, or its error, in a toast. */
function report(e: unknown) {
  if (isRefusal(e)) toastRefusal(e);
  else toast(e instanceof Error ? e.message : String(e), "error");
}

/** An extension's Action as a row: global in its extension's group, contextual in the selection's. */
export function actionItem(action: PaletteAction, selection: PaletteSelection | null): PaletteItem {
  const { element, context } = action;
  const { extension, contribution } = element;
  const contextual = action.entity !== null;
  return {
    id: `act:${element.key}${contextual ? `:${action.entity!.id}` : ""}`,
    group: contextual ? "selection" : `ext:${extension.id}`,
    label: contribution.label,
    lead: { kind: "extension", url: iconOf(element) ?? null, label: contribution.label },
    entity: contextual && selection ? selectionEntity(selection) : undefined,
    extension: { id: extension.id, name: extension.name },
    counter: (!contextual && contribution.counter) || undefined,
    command: true,
    run: () => void runCommand(element, context),
  };
}
