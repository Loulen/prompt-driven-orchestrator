import type { OperationQuestion } from "../../shared/authorization/api";
import { useListRefusals } from "../../shared/authorization/useRefusals";

const CREATE: OperationQuestion[] = [{ resource: "pipeline", verb: "create" }];
const pipelineId = (p: { id: string }) => p.id;
const perPipeline = (p: { id: string }): OperationQuestion[] => [
  { resource: "pipeline", verb: "update", target: p.id },
  { resource: "pipeline", verb: "delete", target: p.id },
];

/**
 * The gestures of the Pipelines list the caller may not make (CORE-205), asked
 * once for the list: `pipeline` × `create` for New, Import and Duplicate,
 * × `update` for Rename, × `delete`, each on its pipeline. A refused gesture
 * is greyed, its tooltip saying the refusal's sentence; `undefined` when
 * allowed (or not answered yet).
 */
export function usePipelineRefusals(pipelines: readonly { id: string }[]) {
  const refusals = useListRefusals(pipelines, pipelineId, perPipeline, CREATE);
  return {
    createRefused: refusals.global[0],
    renameRefused: (id: string) => refusals.of(id)[0],
    deleteRefused: (id: string) => refusals.of(id)[1],
  };
}
