import { useCallback, useMemo } from "react";
import { useEditStore } from "../../../pipeline/store/editStore";
import * as newRunForm from "../../model/newRunForm";
import type { OpenIntent } from "../../model/openIntent";
import { useNewRunState } from "./useNewRunState";
import { useOpenIntentSeeding } from "./useOpenIntentSeeding";
import { useAttachments } from "./useAttachments";
import { useNewRunSubmit } from "./useNewRunSubmit";
import { usePipelineOffer } from "./usePipelineOffer";
import { useLaunchProject } from "./useLaunchProject";
import type { SectionFormHost } from "../../../shared/extensions/sectionForm";

interface NewRunFormArgs {
  open: boolean;
  openIntent: OpenIntent;
  onClose: () => void;
  onCreated: (runId: string) => void;
  onTriggerSaved?: () => void;
  /** ADR-0081 (#31): the Save and Cancel of the `run/new` extension sections. */
  sections?: SectionFormHost;
}

/**
 * Everything `NewRunModal` holds besides its JSX: state, per-open effects, derived values
 * and handlers. The sub-hooks run in the order the component body declared them, so the
 * hook order, the effect timing and the memo deps are those of the pre-split component.
 */
export function useNewRunForm({ open, openIntent, onClose, onCreated, onTriggerSaved, sections }: NewRunFormArgs) {
  const state = useNewRunState(open);
  useOpenIntentSeeding(open, openIntent, state);
  // ADR-0087: the Projet the Run is launched in, first field of Where.
  const launch = useLaunchProject(open, openIntent, state);
  const {
    selectedPipeline, setSelectedPipelineId, setPipelineChosen, overrides, setOverrides,
    setVarsOpen,
  } = state;

  // The Pipeline menu's sections, and the selection held to them.
  const { repoProject, sections: pipelineSections } = usePipelineOffer(
    open,
    state,
    launch.launchProject,
  );

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
      setPipelineChosen(true);
      setOverrides({});
      setVarsOpen(false);
    },
    [setSelectedPipelineId, setPipelineChosen, setOverrides, setVarsOpen],
  );

  const flushPendingSaves = useEditStore((s) => s.flushPendingSaves);

  const handleOverrideChange = useCallback((key: string, value: string) => {
    setOverrides((prev) => ({ ...prev, [key]: value }));
  }, [setOverrides]);

  // Closing without launching is the sections' Cancel.
  const cancel = useCallback(() => {
    sections?.cancel();
    onClose();
  }, [sections, onClose]);
  const attachmentState = useAttachments(state, cancel);
  const submit = useNewRunSubmit(state, {
    primaryInProject: launch.primaryInProject,
    launchProject: launch.launchProject,
    flushPendingSaves,
    attachmentsOverLimit: attachmentState.attachmentsOverLimit,
    onCreated,
    onClose,
    onTriggerSaved,
    saveSections: sections?.save,
    provideSections: sections?.provided,
  });

  return {
    ...state,
    ...launch,
    repoProject,
    pipelineSections,
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
