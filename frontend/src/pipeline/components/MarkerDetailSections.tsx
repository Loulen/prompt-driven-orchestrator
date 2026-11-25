/**
 * The `marker/detail` sections of the extensions (ADR-0081, amendment
 * CORE-322) in the inspector of a Run's Start or End, after the core's
 * sections, headed like them. Nothing without a contribution.
 */

import ExtensionSections from "../../shared/extensions/ExtensionSections";
import type { MarkerContext } from "../../shared/extensions/contract";
import { useMarkerExtra, type MarkerOfRun } from "../../shared/extensions/markerContext";
import { inspectorSection } from "./inspectorSection";

export default function MarkerDetailSections(marker: MarkerOfRun) {
  const extra = useMarkerExtra(marker);
  return <ExtensionSections<MarkerContext> location="marker/detail" extra={extra} wrap={inspectorSection} />;
}
