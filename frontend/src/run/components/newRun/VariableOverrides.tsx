import FormRow from "../../../shared/ui/FormRow";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** The Vars row: one line per variable of the pipeline (name, value, type). Nothing without variables. */
export default function VariableOverrides({ form }: { form: NewRunForm }) {
  const { overrides, variableEntries, handleOverrideChange } = form;
  if (variableEntries.length === 0) return null;
  return (
    <FormRow label="Vars" top title="Pipeline variables: a value other than the default overrides it">
      <div className="ln-vars" data-testid="variable-overrides">
        {variableEntries.map(([name, decl]) => {
          const value = overrides[name] ?? String(decl.default);
          const overridden = value !== String(decl.default);
          return (
            <label key={name} className="ln-var">
              <span className="ln-mono ln-var__k" title={name}>
                {name}
              </span>
              <input
                className="d-input d-input--mono"
                aria-label={name}
                spellCheck={false}
                value={value}
                data-overridden={overridden || undefined}
                onChange={(e) => handleOverrideChange(name, e.target.value)}
              />
              <span className="ln-faint ln-var__t">{decl.var_type}</span>
            </label>
          );
        })}
      </div>
    </FormRow>
  );
}
