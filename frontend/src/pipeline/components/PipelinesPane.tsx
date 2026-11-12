/**
 * The Pipelines tab of the left panel: the instance's pipelines, open,
 * rename, duplicate, delete one or many, create one, import a workflow.
 */

import { useCallback, useMemo, useState } from "react";
import { Copy, FileUp, Funnel, Plus, Trash2, Workflow } from "lucide-react";
import type { PipelineListEntry } from "../../shared/generated/PipelineListEntry";
import { duplicatePipeline } from "../api/pipelines";
import { useEditStore } from "../store/editStore";
import { useSelectionStore } from "../../stores/selectionStore";
import { handleSelectionKeydown } from "../../lib/selectionKeys";
import { type BulkItem, type BulkOutcome } from "../../lib/bulk";
import BulkActionBar from "./BulkActionBar";
import BulkActionModal from "./BulkActionModal";
import ConfirmDeleteModal from "../../shared/ui/ConfirmDeleteModal";
import LibraryRow from "./LibraryRow";
import ValidationIndicator from "./ValidationIndicator";
import { validationOf } from "../model/pipelineValidation";
import type { Project } from "../../workspace/model/project";
import { ExtensionActionButtons } from "../../shared/extensions/ExtensionActions";
import type { PipelineEntity } from "../../shared/extensions/contract";

const NO_EXTRA = {};

import ImportWorkflowModal from "./ImportWorkflowModal";
import NewPipelineModal from "./NewPipelineModal";
import PipelineItemDecorations from "./PipelineItemDecorations";
import ExtensionListFilters from "../../shared/extensions/ExtensionListFilters";
import LeftPaneBar from "../../shared/ui/LeftPaneBar";
import { IconButton } from "../../shared/ui/kit";
import { usePipelineRefusals } from "../store/usePipelineRefusals";
import ListEmpty from "../../shared/ui/leftList/ListEmpty";
import {
  isAttributeFilterActive,
  matchesAttributes,
  type Attributes,
} from "../../shared/extensions/attributes";

export default function PipelinesPane({ projects = [] }: { projects?: Project[] } = {}) {
  // #774 — inline rename of a Pipelines-tab row. Same pattern as the run rows
  // above: the pencil flips the row's name into an input; Enter/blur commits,
  // Escape cancels.
  const [renamingPipelineId, setRenamingPipelineId] = useState<string | null>(null);
  const [pipelineRenameValue, setPipelineRenameValue] = useState("");

  const allPipelines = useEditStore((s) => s.pipelines);
  // CORE-248: the caller may not read the pipelines: the list says why, with no New.
  const refused = useEditStore((s) => s.pipelinesRefused);
  // ADR-0081 (#31) — the `pipelines/filter` of the extensions: the list keeps
  // the pipelines carrying every attribute asked for.
  const [attributeFilter, setAttributeFilter] = useState<Attributes>({});
  const pipelines = useMemo(
    () =>
      isAttributeFilterActive(attributeFilter)
        ? allPipelines.filter((p) => matchesAttributes(p.attributes, attributeFilter))
        : allPipelines,
    [allPipelines, attributeFilter],
  );
  const loadPipelines = useEditStore((s) => s.loadPipelines);
  const openPipeline = useEditStore((s) => s.openPipeline);
  const removePipeline = useEditStore((s) => s.removePipeline);
  const renamePipeline = useEditStore((s) => s.renamePipeline);
  const activeTabId = useEditStore((s) => s.activeTabId);

  const librarySelIds = useSelectionStore((s) => s.library);
  const toggleSel = useSelectionStore((s) => s.toggle);
  const selectRange = useSelectionStore((s) => s.selectRange);
  const selectVisible = useSelectionStore((s) => s.selectVisible);
  const deselect = useSelectionStore((s) => s.deselect);
  const clearSel = useSelectionStore((s) => s.clear);
  const librarySel = useMemo(() => new Set(librarySelIds), [librarySelIds]);
  // CORE-205: the gestures the caller may not make, greyed with why.
  const { createRefused, renameRefused, deleteRefused } = usePipelineRefusals(pipelines);
  const [libBulkKind, setLibBulkKind] = useState<"delete" | "duplicate" | null>(null);

  const [showNewModal, setShowNewModal] = useState(false);
  const [showImportModal, setShowImportModal] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<PipelineListEntry | null>(null);
  // Busy guard so a double-click on a library row's Copy icon fires once (#224).
  const [duplicatingId, setDuplicatingId] = useState<string | null>(null);

  // #774 — inline rename of a pipeline row. The store action drives everything
  // (daemon rename, list refresh, open-tab rekey); a refusal (409 collision,
  // active runs) is silently swallowed like every other row action — the list
  // simply stays as it was.
  function startPipelineRename(p: PipelineListEntry) {
    setRenamingPipelineId(p.id);
    setPipelineRenameValue(p.name);
  }

  async function commitPipelineRename() {
    const id = renamingPipelineId;
    const name = pipelineRenameValue.trim();
    setRenamingPipelineId(null);
    setPipelineRenameValue("");
    if (!id || !name) return;
    try {
      await renamePipeline(id, name);
    } catch {
      // 409 (name/file collision, active runs) and network errors: the row
      // stays under its current name.
    }
  }

  function cancelPipelineRename() {
    setRenamingPipelineId(null);
    setPipelineRenameValue("");
  }

  async function handleConfirmDelete() {
    if (!deleteTarget) return;
    try {
      await removePipeline(deleteTarget.id);
      await loadPipelines();
    } catch {
      // ignore (e.g. 409 active runs)
    } finally {
      setDeleteTarget(null);
    }
  }

  // #224/#371 — duplicate a library template. One shared seam for both Copy
  // affordances (the block-1 scope:"library" row and the block-2 library-only
  // row) so they can never again drift apart. Busy-guarded (a double-click on
  // the Copy icon fires once), then refreshes BOTH pipeline lists:
  //   - loadPipelines() re-fetches the authoritative /pipelines, where the
  //     daemon tags the copy scope:"library" — so it lands in the proper
  //     block-1 button path at once: a clickable, correctly-badged row.
  //   - onLibraryPipelinesChanged() re-fetches /library/pipelines.
  // Refreshing only the latter (the pre-#371 behaviour) left the copy in the
  // degraded block-2 <div>: wrong "user" badge, no button role, dead click,
  // until a full page reload. The New/Import handlers already loadPipelines()
  // the same way; only Duplicate had drifted.
  async function handleDuplicate(id: string) {
    if (duplicatingId === id) return;
    setDuplicatingId(id);
    try {
      const copy = await duplicatePipeline(id);
      await loadPipelines();
      await openPipeline(copy.id);
    } catch {
      /* ignore */
    } finally {
      setDuplicatingId(null);
    }
  }

  // #577 — Library multi-select. The two Library lists (openable /pipelines rows
  // and passive library-only rows) delete through different seams, so each row
  // registers its own `del`/`dup` keyed by the SAME id its React key uses. Bulk
  // then just dispatches per selected id — no re-deriving which list it came from.
  interface LibTarget {
    selId: string;
    name: string;
    del: () => Promise<void>;
    dup?: () => Promise<void>;
    /** CORE-205: why the caller may not delete it, else absent. */
    delRefused?: string;
  }
  const libTargets: LibTarget[] = pipelines.map((p) => ({
      selId: `${p.scope}-${p.id}`,
      name: p.name,
      del: () => removePipeline(p.id, p.scope),
      dup: () => duplicatePipeline(p.id).then(() => {}),
      delRefused: deleteRefused(p.id),
    }));
  const libTargetById = new Map(libTargets.map((t) => [t.selId, t]));
  const libVisibleIds = libTargets.map((t) => t.selId);
  const selectedLibTargets = libTargets.filter((t) => librarySel.has(t.selId));
  // CORE-205: only the pipelines the caller may delete; none, Delete is greyed with why.
  const libDeleteItems: BulkItem[] = selectedLibTargets
    .filter((t) => !t.delRefused)
    .map((t) => ({ id: t.selId, label: t.name }));
  const libDeleteRefused =
    selectedLibTargets.length > 0 && libDeleteItems.length === 0 ? selectedLibTargets[0].delRefused : undefined;
  const libDupItems: BulkItem[] = selectedLibTargets
    .filter((t) => t.dup)
    .map((t) => ({ id: t.selId, label: t.name }));
  const handleLibrarySettled = useCallback(
    (o: BulkOutcome) => {
      deselect("library", o.succeeded.map((r) => r.id));
      void loadPipelines();
    },
    [deselect, loadPipelines],
  );

  return (
    <>
        <div className="lp-pane" role="tabpanel">
      <LeftPaneBar title="Pipelines">
        {/* #30 — `pipelines/header`: the extensions' icon buttons, before the
            core's. */}
        <ExtensionActionButtons
          location="pipelines/header"
          extra={NO_EXTRA}
          className="flex items-center gap-px"
          buttonClassName="d-icon"
          iconSize={14}
        />
        {refused === null && (
          <>
            <IconButton
              icon={FileUp}
              label="Import a workflow"
              disabledReason={createRefused}
              onClick={() => setShowImportModal(true)}
              data-testid="import-workflow-button"
            />
            <IconButton
              icon={Plus}
              label="New pipeline"
              disabledReason={createRefused}
              onClick={() => setShowNewModal(true)}
              data-testid="new-pipeline-button"
            />
          </>
        )}
      </LeftPaneBar>

      <ExtensionListFilters
        location="pipelines/filter"
        filter={attributeFilter}
        onChange={setAttributeFilter}
      />

        <div
          className="lp-scroll"
          tabIndex={-1}
          onKeyDown={(e) =>
            handleSelectionKeydown(e, {
              tab: "library",
              visibleIds: libVisibleIds,
              hasSelection: librarySelIds.length > 0,
              selectVisible,
              clear: clearSel,
              onBulkDelete: () => {
                if (libDeleteItems.length > 0) setLibBulkKind("delete");
              },
            })
          }
        >
          {pipelines.length === 0 &&
            (refused !== null ? (
              <ListEmpty icon={Workflow} title={refused} testId="pipelines-refused" />
            ) : allPipelines.length > 0 ? (
              <ListEmpty
                icon={Funnel}
                title="No match"
                actionLabel="Clear filters"
                onAction={() => setAttributeFilter({})}
                testId="pipeline-filter-empty"
              />
            ) : (
              <ListEmpty
                icon={Workflow}
                title="No pipelines"
                actionLabel="New pipeline"
                actionIcon={Plus}
                onAction={() => setShowNewModal(true)}
                actionRefused={createRefused}
                testId="pipelines-empty"
              />
            ))}
          <div className="lp-rows">
          {pipelines.map((p) => (
            <LibraryRow
              key={p.id}
              // #823: the tour's last step points at the row of the pipeline the
              // user just built — keep it or delete it, both from here.
              testId={`library-row-${p.id}`}
              name={p.name}
              checked={librarySel.has(`${p.scope}-${p.id}`)}
              selecting={librarySelIds.length > 0}
              invalid={p.invalid}
              onToggleSelect={(e) => {
                const selId = `${p.scope}-${p.id}`;
                if (e.shiftKey) selectRange("library", selId, libVisibleIds);
                else toggleSel("library", selId);
              }}
              selected={p.id === activeTabId}
              // #273: scope:"library" rows now appear here in block 1 (the
              // /pipelines scope-merge from #216 means they no longer fall
              // through to the library-only block below). Surface the same Copy
              // affordance #224 shipped, gated on identity (scope), not the
              // name-absence filter that block 2 uses. `p.id` is the HOME
              // library file-stem — duplicateLibraryPipeline resolves it.
              showDuplicate
              modified={p.modified}
              draft={p.draft}
              version={p.version?.number}
              decorations={<PipelineItemDecorations pipeline={p} />}
              // Where the launch forms offer it, at a glance.
              indicator={
                <ValidationIndicator
                  validation={validationOf(p)}
                  projects={projects}
                  testId={`validation-indicator-${p.id}`}
                />
              }
              onOpen={() => openPipeline(p.id)}
              onDuplicate={() => handleDuplicate(p.id)}
              onRenameStart={() => startPipelineRename(p)}
              renaming={renamingPipelineId === p.id}
              renameValue={pipelineRenameValue}
              onRenameValueChange={setPipelineRenameValue}
              onRenameCommit={commitPipelineRename}
              onRenameCancel={cancelPipelineRename}
              // Confirm-gated, because this row is a working pipeline file and
              // the delete may cascade to its Library twin (#227).
              onDelete={() => setDeleteTarget(p)}
              deleteTitle="Delete pipeline"
              menuEntity={pipelineEntity(p)}
              refused={{ rename: renameRefused(p.id), duplicate: createRefused, delete: deleteRefused(p.id) }}
            />
          ))}
          </div>
        </div>

        {/* #577 — the floating bulk bar of the Pipelines tab. */}
        {librarySelIds.length > 0 && (
          <BulkActionBar
            count={librarySelIds.length}
            actions={[
              {
                key: "delete",
                label: "Delete",
                icon: <Trash2 size={14} strokeWidth={1.5} aria-hidden />,
                destructive: true,
                disabledReason: libDeleteRefused,
                onClick: () => setLibBulkKind("delete"),
              },
              {
                key: "duplicate",
                label: "Duplicate",
                icon: <Copy size={14} strokeWidth={1.5} aria-hidden />,
                disabled: libDupItems.length === 0,
                disabledReason: createRefused,
                onClick: () => setLibBulkKind("duplicate"),
              },
            ]}
            onClear={() => clearSel("library")}
          />
        )}
        {libBulkKind === "delete" && (
          <BulkActionModal
            destructive
            runningLabel="Deleting"
            title={`Delete ${libDeleteItems.length} pipeline${libDeleteItems.length === 1 ? "" : "s"}?`}
            description="This permanently removes the selected pipelines' files (YAML + prompts) from disk. This can't be undone."
            confirmLabel="Delete"
            items={libDeleteItems}
            run={(selId) => libTargetById.get(selId)!.del()}
            onClose={() => setLibBulkKind(null)}
            onSettled={handleLibrarySettled}
          />
        )}
        {libBulkKind === "duplicate" && (
          <BulkActionModal
            skipConfirm
            runningLabel="Duplicating"
            title=""
            description=""
            confirmLabel="Duplicate"
            items={libDupItems}
            run={(selId) => libTargetById.get(selId)!.dup!()}
            onClose={() => setLibBulkKind(null)}
            onSettled={handleLibrarySettled}
          />
        )}
        </div>

      <ConfirmDeleteModal
        // Remount per target so the checkbox resets to OFF each open (#227).
        key={deleteTarget?.id ?? "none"}
        open={deleteTarget !== null}
        onClose={() => setDeleteTarget(null)}
        onConfirm={handleConfirmDelete}
        name={deleteTarget?.name ?? ""}
      />

      {showNewModal && (
        <NewPipelineModal onClose={() => setShowNewModal(false)} />
      )}

      {showImportModal && (
        <ImportWorkflowModal onClose={() => setShowImportModal(false)} />
      )}
    </>
  );
}

/** A row's Pipeline, as its menu's context hands it over (#30). */
function pipelineEntity(p: PipelineListEntry): PipelineEntity {
  return { kind: "pipeline", id: p.id, name: p.name, attributes: p.attributes ?? {} };
}
