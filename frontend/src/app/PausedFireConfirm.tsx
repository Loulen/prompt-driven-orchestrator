import { ConfirmDialog } from "../shared/ui/kit";
import type { useLaunchActions } from "./useLaunchActions";

/** #348: « Run now » while the Triggers are paused is deliberate: confirmed first. */
export default function PausedFireConfirm({ launch }: { launch: ReturnType<typeof useLaunchActions> }) {
  if (!launch.pausedFire) return null;
  return (
    <ConfirmDialog
      title={`Fire ${launch.pausedFire.name}?`}
      consequence="Triggers are paused"
      actionLabel="Run now"
      onConfirm={launch.confirmPausedFire}
      onClose={launch.cancelPausedFire}
    />
  );
}
