/**
 * The field anatomy of the Settings surface: label, control, help, source note — plus the
 * amber border of a field the draft changed.
 */

import type { ReactNode } from "react";
import type { SettingField } from "../../model/settings";
import { sourceNote } from "../../model/sourceNotes";

/** Label / control / help / source note — today's field anatomy, plus the dirty border. */
export function FieldBlock({
  label,
  htmlFor,
  testId,
  dirty,
  help,
  source,
  sourceTestId,
  children,
}: {
  label: string;
  htmlFor?: string;
  testId?: string;
  dirty?: boolean;
  help?: ReactNode;
  source?: string;
  sourceTestId?: string;
  children: ReactNode;
}) {
  return (
    <div className="flex flex-col gap-1.5" data-testid={testId} data-dirty={dirty ? "true" : undefined}>
      <label htmlFor={htmlFor} className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
        {label}
      </label>
      {children}
      {help && (
        <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
          {help}
        </div>
      )}
      {source && (
        <div className="text-fg-3" style={{ fontSize: "10.5px" }} data-testid={sourceTestId}>
          {source}
        </div>
      )}
    </div>
  );
}

interface SettingRowProps {
  id: string;
  label: string;
  help: string;
  value: string;
  onChange: (v: string) => void;
  dirty: boolean;
  field: SettingField;
  envVar: string;
  /** Unit suffix appended to the env value in the disclosure (e.g. " s", " ms"). */
  unit: string;
  /** The env value is in milliseconds while the knob is in seconds (guard). */
  envIsMs?: boolean;
  /** Rendered right of the input (the live session preview). */
  inline?: ReactNode;
  children?: ReactNode;
}

export function SettingRow({
  id,
  label,
  help,
  value,
  onChange,
  dirty,
  field,
  envVar,
  unit,
  envIsMs,
  inline,
  children,
}: SettingRowProps) {
  return (
    <div className="flex flex-col gap-1.5" data-dirty={dirty ? "true" : undefined}>
      <label htmlFor={`setting-${id}`} className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
        {label}
      </label>
      <div className="flex items-center gap-3">
        <input
          id={`setting-${id}`}
          data-testid={`setting-${id}`}
          type="number"
          min={1}
          value={value}
          onChange={(e) => onChange(e.target.value)}
          className={`w-[220px] rounded-md border bg-bg-3 px-2.5 py-1.5 font-mono text-fg placeholder:text-fg-4 focus:outline-none ${
            dirty ? "border-st-await focus:border-st-await" : "border-line-strong focus:border-acc"
          }`}
          style={{ fontSize: "12px" }}
        />
        {inline}
      </div>
      <div className="text-fg-4" style={{ fontSize: "10.5px" }}>
        {help}
      </div>
      <div className="text-fg-3" style={{ fontSize: "10.5px" }} data-testid={`setting-source-${id}`}>
        {sourceNote(field, envVar, unit, envIsMs)}
      </div>
      {children}
    </div>
  );
}

export function CheckboxRow({
  id,
  checked,
  onChange,
  dirty,
  label,
  help,
  source,
}: {
  id: string;
  checked: boolean;
  onChange: (v: boolean) => void;
  dirty: boolean;
  label: string;
  help: ReactNode;
  source: string;
}) {
  return (
    <div className="flex flex-col gap-1.5" data-dirty={dirty ? "true" : undefined}>
      <label htmlFor={`setting-${id}`} className="flex cursor-pointer items-start gap-2">
        <input
          id={`setting-${id}`}
          data-testid={`setting-${id}`}
          type="checkbox"
          checked={checked}
          onChange={(e) => onChange(e.target.checked)}
          className={`mt-0.5 shrink-0 accent-acc ${dirty ? "outline outline-1 outline-st-await" : ""}`}
        />
        <span className="font-medium text-fg-2" style={{ fontSize: "11.5px" }}>
          {label}
        </span>
      </label>
      <div className="pl-5 text-fg-4" style={{ fontSize: "10.5px" }}>
        {help}
      </div>
      <div className="text-fg-3" style={{ fontSize: "10.5px" }} data-testid={`setting-source-${id}`}>
        {source}
      </div>
    </div>
  );
}
