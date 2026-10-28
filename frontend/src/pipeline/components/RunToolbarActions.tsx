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
      before={readOnly ? null : <span className="cv-tb__sep" aria-hidden />}
      className="flex items-center gap-px"
      buttonClassName="d-icon"
      iconSize={14}
    />
  );
}
