import { TextInput } from "../../../shared/ui/kit";
import FormRow from "../../../shared/ui/FormRow";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** Run mode: the name (#184) and, at the row end, auto-naming (#338). */
export default function RunNameSection({ form }: { form: NewRunForm }) {
  const { runName, setRunName, autoName } = form;
  return (
    <div className="ln-sec">
      <FormRow label="Name" htmlFor="run-name">
        <TextInput
          id="run-name"
          className="ln-grow"
          placeholder={autoName ? "Auto-named" : "Untitled run"}
          value={autoName ? "" : runName}
          onChange={(e) => setRunName(e.target.value)}
          disabled={autoName}
          data-autofocus=""
          data-testid="run-name-input"
        />
        <AutoNameBox form={form} />
      </FormRow>
    </div>
  );
}

/** Auto-naming of the Run, or of each Run a Trigger fires (#338). */
export function AutoNameBox({ form }: { form: NewRunForm }) {
  const { autoName, setAutoName, mode } = form;
  return (
    <label className="d-check ln-autoname" title={mode === "trigger" ? "Auto-name each fired run" : "Auto-name"}>
      <input
        type="checkbox"
        checked={autoName}
        onChange={(e) => setAutoName(e.target.checked)}
        data-testid="auto-name-checkbox"
      />
      Auto
    </label>
  );
}
