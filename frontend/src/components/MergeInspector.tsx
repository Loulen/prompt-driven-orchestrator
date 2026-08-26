import { useEditStore } from "../stores/editStore";
import { SectionHead, Field } from "./InspectorPrimitives";
import ModelPicker from "./ModelPicker";
import EffortPicker from "./EffortPicker";
import HarnessSelect from "./HarnessSelect";
import { useHarnessCatalog } from "../hooks/useHarnessCatalog";
import { findHarnessOption, resolveEditorHarness } from "../lib/harness";

export default function MergeInspector() {
  const openTabs = useEditStore((s) => s.openTabs);
  const activeTabId = useEditStore((s) => s.activeTabId);
  const selection = useEditStore((s) => s.selection);
  const updateNode = useEditStore((s) => s.updateNode);

  // #586/#616: the harness catalogue, called before the early return so the hook
  // order is stable.
  const harnessCatalog = useHarnessCatalog();

  const tab = openTabs.find((t) => t.id === activeTabId);
  if (!tab || selection.kind !== "node" || !selection.id) return null;

  const node = tab.pipeline.nodes.find((n) => n.id === selection.id);
  if (!node || node.type !== "merge") return null;

  // #616 (correctif 9): a merge node launches an agent just like any work node, so
  // it gets the SAME harness picker and the SAME effort greying — no second,
  // drifting copy. The resolved harness (pin, else the `claude` floor) drives what
  // the model/effort mean and whether the effort picker greys.
  const resolvedHarness = resolveEditorHarness(node);
  const harnessOption = findHarnessOption(harnessCatalog, resolvedHarness);

  return (
    <aside className="flex h-full flex-col bg-bg-2 overflow-y-auto">
      <div
        className="flex h-[36px] items-center border-b border-line px-3 font-medium text-fg-2"
        style={{ fontSize: "11.5px" }}
      >
        Merge Inspector
      </div>

      <div className="flex flex-col gap-3 p-3" style={{ fontSize: "11.5px" }}>
        <SectionHead title="Identity" />
        <Field label="ID">
          <span className="font-mono text-fg-3">{node.id}</span>
        </Field>
        <Field label="Name">
          <input
            value={node.name ?? ""}
            onChange={(e) => updateNode(node.id, { name: e.target.value || null })}
            className="w-full rounded border border-line-strong bg-bg-3 px-2 py-1 text-fg outline-none focus:border-acc"
          />
        </Field>

        {/* Harness (#616 correctif 9): a merge node exposes the same harness choice
            as any node that launches an agent — same picker, same greying rule.
            `""` = "Default" (no pin, `claude` floor); a concrete name pins it. */}
        <Field label="Harness">
          <div data-testid="merge-harness" data-resolved={resolvedHarness}>
            <HarnessSelect
              data-testid="merge-harness-select"
              value={node.pin_harness ?? ""}
              onChange={(v) => updateNode(node.id, { pin_harness: v === "" ? null : v })}
              catalog={harnessCatalog}
              inheritLabel="Default (claude floor)"
              className="w-full cursor-pointer rounded border border-line-strong bg-bg-3 px-2 py-1 font-medium text-fg outline-none focus:border-acc"
              style={{ fontSize: "10px" }}
            />
          </div>
          <p className="mt-1 text-fg-4" style={{ fontSize: "9.5px" }}>
            Resolved: <span data-testid="merge-harness-resolved">{resolvedHarness}</span>
            {node.pin_harness ? " (pinned)" : " (floor — no pin)"}
          </p>
        </Field>

        {/* Model (#296/#324, #616): a merge node spawns an agent, so its model is
            settable here too. Dropdown of served ids + Custom… escape hatch. */}
        <Field label="Model">
          <ModelPicker
            value={node.model ?? null}
            onChange={(v) => updateNode(node.id, { model: v })}
            models={harnessOption?.models ?? []}
            testid="merge-model"
          />
        </Field>
        {/* Effort (#424, #616): a merge node IS a regular NodeDef routed through
            `spawn_node`, so it launches with the node's effort — and its picker
            follows the SAME served greying rule as any node (correctif 9). Not to be
            confused with the `__merge_resolver__` infra session, which has no NodeDef
            and always runs at the account default. */}
        <Field label="Effort">
          <EffortPicker
            value={node.effort ?? null}
            onChange={(v) => updateNode(node.id, { effort: v })}
            efforts={harnessOption?.efforts ?? []}
            testid="merge-effort"
            disabled={!(harnessOption?.hasEffort ?? true)}
          />
        </Field>

        <SectionHead title="Ports" />
        <Field label="Input">
          <span className="font-mono text-fg-3">branches (repeated)</span>
        </Field>
        <Field label="Output">
          <span className="font-mono text-fg-3">merged</span>
        </Field>

        <div
          className="mt-2 rounded border border-acc/30 bg-acc/5 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          Merge nodes wait for all upstream branches to complete, then merge
          their worktrees. If conflicts arise, a resolver session is spawned.
        </div>
      </div>
    </aside>
  );
}
