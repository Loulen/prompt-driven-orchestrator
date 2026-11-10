import { toast } from "../../shared/ui/kit";

/** Restores the version `number` into the draft, and says so: one change Undo takes back. */
export async function restoreIntoDraft(
  restoreVersion: (id: string, n: number) => Promise<void>,
  pipelineId: string,
  number: number,
) {
  try {
    await restoreVersion(pipelineId, number);
    toast(`v${number} restored into the draft`, "success");
  } catch (e) {
    toast(e instanceof Error ? e.message : String(e), "error");
  }
}
