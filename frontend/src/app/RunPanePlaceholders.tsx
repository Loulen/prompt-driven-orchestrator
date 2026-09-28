// What the Run pane of the inspector shows before a node of the Run is live.

export function RunTabPlaceholder({ nodeId }: { nodeId: string | null }) {
  return (
    <aside
      className="flex h-full flex-col items-center justify-center bg-bg-2 text-fg-4"
      style={{ fontSize: "12px" }}
      data-testid="pending-placeholder"
    >
      <div className="px-6 text-center">
        <div className="font-medium text-fg-3">
          <em>en attente d&apos;activation</em>
        </div>
        {nodeId && (
          <div className="mt-1 font-mono" style={{ fontSize: "10px" }}>
            {nodeId}
          </div>
        )}
        <div className="mt-2 text-fg-4" style={{ fontSize: "11px" }}>
          This node is waiting for upstream dependencies to complete.
        </div>
      </div>
    </aside>
  );
}

export function NoRunPlaceholder() {
  return (
    <aside
      className="flex h-full flex-col items-center justify-center bg-bg-2 text-fg-4"
      style={{ fontSize: "12px" }}
    >
      <div className="px-6 text-center">
        <div className="font-medium text-fg-3">No active run</div>
        <div className="mt-1">
          Launch a run to see execution state in this tab.
        </div>
      </div>
    </aside>
  );
}
