import { useMemo } from "react";
import type { RunState } from "../../shared/generated/RunState";
import { runContext } from "../../shared/extensions/context";
import { ExtensionActionButtons } from "../../shared/extensions/ExtensionActions";

/**
 * The extensions' `run/toolbar` actions (#30) in the toolbar of a Run's canvas,
 * after a separator. Their context's Run is the Run on screen, whatever the
 * list's selection.
 */
export default function RunToolbarActions({ run, readOnly }: { run: RunState; readOnly: boolean }) {
  const extra = useMemo(() => ({ run: runContext(run) }), [run]);
  return (
    <ExtensionActionButtons
      location="run/toolbar"
      extra={extra}
      // An archived canvas has no editing group: nothing to separate from.
      before={readOnly ? null : <span className="mx-0.5 h-4 w-px bg-line" />}
      className="flex items-center gap-0.5"
      buttonClassName="grid h-7 w-7 cursor-pointer place-items-center rounded text-fg-3 transition-colors hover:bg-bg-4 hover:text-fg active:bg-acc active:text-bg-0"
      iconSize={14}
    />
  );
}
