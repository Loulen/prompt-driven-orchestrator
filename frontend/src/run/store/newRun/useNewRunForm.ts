import { useCallback, useMemo } from "react";
import { useEditStore } from "../../../pipeline/store/editStore";
import * as newRunForm from "../../model/newRunForm";
import type { OpenIntent } from "../../model/openIntent";
import { useNewRunState } from "./useNewRunState";
import { useOpenIntentSeeding } from "./useOpenIntentSeeding";
import { useAttachments } from "./useAttachments";
import { useNewRunSubmit } from "./useNewRunSubmit";

interface NewRunFormArgs {
  open: boolean;
  openIntent: OpenIntent;
  onClose: () => void;
  onCreated: (runId: string) => void;
  onTriggerSaved?: () => void;
}

/**
 * Everything `NewRunModal` holds besides its JSX: state, per-open effects, derived values
 * and handlers. The sub-hooks run in the order the component body declared them, so the
 * hook order, the effect timing and the memo deps are those of the pre-split component.
 */
export function useNewRunForm({ open, openIntent, onClose, onCreated, onTriggerSaved }: NewRunFormArgs) {
  const state = useNewRunState(open);
  useOpenIntentSeeding(open, openIntent, state);
  const {
    pipelines, selectedPipeline, selectedPipelineId, setSelectedPipelineId, repoValid,
    overrides, setOverrides, setVarsOpen,
  } = state;

  // Auto-select first repo pipeline when available
  const shouldAutoSelect = open && repoValid && pipelines.length > 0 && !selectedPipelineId;
  if (shouldAutoSelect) {
    const first = pipelines[0];
    if (first) setSelectedPipelineId(first.id);
  }

  const variableEntries = useMemo(() => {
    if (!selectedPipeline) return [];
    return Object.entries(selectedPipeline.variables).sort(([a], [b]) =>
      a.localeCompare(b),
    );
  }, [selectedPipeline]);

  const overrideCount = useMemo(
    () => newRunForm.overrideCount(overrides, selectedPipeline),
    [overrides, selectedPipeline],
  );

  const handlePipelineChange = useCallback(
    (value: string) => {
      setSelectedPipelineId(value);
      setOverrides({});
      setVarsOpen(false);
    },
    [setSelectedPipelineId, setOverrides, setVarsOpen],
  );

  const flushPendingSaves = useEditStore((s) => s.flushPendingSaves);

  const handleOverrideChange = useCallback((key: string, value: string) => {
    setOverrides((prev) => ({ ...prev, [key]: value }));
  }, [setOverrides]);

  const attachmentState = useAttachments(state, onClose);
  const submit = useNewRunSubmit(state, {
    flushPendingSaves,
    attachmentsOverLimit: attachmentState.attachmentsOverLimit,
    onCreated,
    onClose,
    onTriggerSaved,
  });

  return {
    ...state,
    variableEntries,
    overrideCount,
    handlePipelineChange,
    flushPendingSaves,
    handleOverrideChange,
    ...attachmentState,
    ...submit,
  };
}

export type NewRunForm = ReturnType<typeof useNewRunForm>;
