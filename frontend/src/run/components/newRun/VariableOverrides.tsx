import { ChevronDown } from "lucide-react";
import type { NewRunForm } from "../../store/newRun/useNewRunForm";

/** ── CONFIG ── the variable overrides accordion. Renders nothing without variables. */
export default function VariableOverrides({ form }: { form: NewRunForm }) {
  const {
    overrides, varsOpen, setVarsOpen, variableEntries, overrideCount, handleOverrideChange,
  } = form;
  if (variableEntries.length === 0) return null;
  return (
    <div className="rounded-md border border-line">
      <button
        type="button"
        className="flex w-full items-center gap-2 px-3 py-2 text-left transition-colors hover:bg-bg-3/50"
        style={{ fontSize: "11.5px" }}
        onClick={() => setVarsOpen(!varsOpen)}
      >
        <ChevronDown
          size={12}
          className={`text-fg-3 transition-transform ${varsOpen ? "" : "-rotate-90"}`}
        />
        <span className="font-medium text-fg-2">
          Variable overrides
        </span>
        {overrideCount > 0 && (
          <span
            className="ml-auto font-mono text-acc"
            style={{ fontSize: "10.5px" }}
          >
            ({overrideCount} overridden)
          </span>
        )}
      </button>
      {varsOpen && (
        <div className="flex flex-col gap-2 border-t border-line px-3 py-2.5">
          {variableEntries.map(([name, decl]) => {
            const currentVal =
              overrides[name] ?? String(decl.default);
            const isOverridden = currentVal !== String(decl.default);
            return (
              <div
                key={name}
                className="grid items-center gap-2"
                style={{ gridTemplateColumns: "110px 1fr" }}
              >
                <span
                  className={`truncate font-mono ${isOverridden ? "text-fg-3" : "text-fg-4"}`}
                  style={{ fontSize: "11.5px" }}
                  title={`${name} (${decl.var_type})`}
                >
                  {name}
                </span>
                <input
                  className={`w-full rounded border bg-bg-3 px-2 py-1 font-mono text-fg transition-colors focus:border-acc focus:outline-none ${isOverridden ? "border-acc-border" : "border-line-strong"}`}
                  style={{ fontSize: "11.5px" }}
                  value={currentVal}
                  onChange={(e) =>
                    handleOverrideChange(name, e.target.value)
                  }
                />
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
