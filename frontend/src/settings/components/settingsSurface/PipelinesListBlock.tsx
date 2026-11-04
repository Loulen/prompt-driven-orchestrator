import { useState } from "react";
import { PIPELINE_ROW_ELEMENTS, PIPELINE_ROW_LOCATIONS } from "../../../pipeline/model/pipelineRowElements";
import { useEditStore } from "../../../pipeline/store/editStore";
import type { RowElement } from "../../../run/model/runRowElements";
import { useHideableContributions } from "../../../shared/extensions/hideableContributions";
import type { Project } from "../../../workspace/model/project";
import AppearanceBlock from "./AppearanceBlock";
import { ElementSwitch, Group } from "./ElementSwitch";
import PipelinesListPreview from "./PipelinesListPreview";

/**
 * The Pipelines list block of Settings › Appearance › Appearance: a switch per element
 * of a row, then one per extension contribution (« Extension · label ») under
 * *Extensions*, and beside them the list's own rows as a preview, on the Pipelines
 * of the list.
 */
export default function PipelinesListBlock({ projects }: { projects: Project[] }) {
  const pipelines = useEditStore((s) => s.pipelines);
  const contributions = useHideableContributions("pipelines.list", PIPELINE_ROW_LOCATIONS);
  const [pointed, setPointed] = useState<string | null>(null);
  const toggle = (element: RowElement) => (
    <ElementSwitch
      key={element.id}
      element={element}
      surfaceName="Pipelines list"
      pointed={pointed === element.id}
      onPoint={setPointed}
    />
  );
  return (
    <AppearanceBlock
      id="pipelines-list"
      title="Pipelines list"
      alwaysShown="Name, select control"
      preview={<PipelinesListPreview pipelines={pipelines} projects={projects} pointed={pointed} />}
    >
      {PIPELINE_ROW_ELEMENTS.map(toggle)}
      {contributions.length > 0 && <Group label="Extensions" />}
      {contributions.map(toggle)}
    </AppearanceBlock>
  );
}
