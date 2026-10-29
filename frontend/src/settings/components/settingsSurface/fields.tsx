/**
 * The rows of the Settings surface: a setting's name on the left (its help and the source
 * of its value in the name's tooltip), its control on the right, the scope icon at the
 * end, and the amber edge of a row the draft changed. A row hides itself when the search
 * line does not find it.
 */

import { CircleX, Monitor, Zap } from "lucide-react";
import { useId, type ReactNode } from "react";
import { Switch, TextInput } from "../../../shared/ui/kit";
import { cx } from "../../../shared/ui/kit/util";
import type { SettingField } from "../../model/settings";
import { sourceNote } from "../../model/sourceNotes";
import { rangeError } from "./range";
import { useRowVisible } from "./search";

/** Where a value is kept when it is not the instance form: this browser, or its own resource. */
export type Scope = "device" | "live";

const SCOPE: Record<Scope, { icon: typeof Monitor; label: string }> = {
  device: { icon: Monitor, label: "This device, saved at once" },
  live: { icon: Zap, label: "Saved at once" },
};

/** The scope of a row or a section, as an icon with its name in the tooltip. */
export function ScopeIcon({ scope, testId }: { scope?: Scope; testId?: string }) {
  if (!scope) return <span className="set-scope" aria-hidden />;
  const { icon: Icon, label } = SCOPE[scope];
  return (
    <span className="set-scope" title={label} data-testid={testId} data-scope={scope}>
      <Icon size={12} strokeWidth={1.5} aria-label={label} />
    </span>
  );
}

export interface SettingRowProps {
  label: string;
  /** The name's tooltip: what the setting does. */
  help?: string;
  /** The name's tooltip too: which tier the value comes from. */
  source?: string;
  /** Found by the search line too. */
  keywords?: string;
  scope?: Scope;
  /** `wide`: a control that needs the width (a URL, a list); `top`: a control taller than a line. */
  kind?: "value" | "wide";
  top?: boolean;
  dirty?: boolean;
  /** The control's id: the name is its label. */
  htmlFor?: string;
  testId?: string;
  /** One short line under the control: a warning, a refusal. */
  note?: ReactNode;
  noteTone?: "warn" | "failed";
  noteTestId?: string;
  children: ReactNode;
}

/** A setting: name left, control right, 30 px. */
export function SettingRow({
  label,
  help,
  source,
  keywords,
  scope,
  kind = "value",
  top,
  dirty,
  htmlFor,
  testId,
  note,
  noteTone,
  noteTestId,
  children,
}: SettingRowProps) {
  const id = useId();
  const visible = useRowVisible(`${label} ${help ?? ""} ${keywords ?? ""}`, !!dirty);
  if (!visible) return null;
  const tip = [help, source].filter(Boolean).join("\n") || label;
  const Name = htmlFor ? "label" : "span";
  return (
    // A group named by the setting and described by its tooltip's text, read by
    // assistive technologies that never hover.
    <div
      className={cx("set-row", kind === "wide" && "set-row--wide", top && "set-row--top")}
      role="group"
      aria-labelledby={`${id}-name`}
      aria-describedby={tip !== label ? `${id}-tip` : undefined}
      data-testid={testId}
      data-dirty={dirty ? "true" : undefined}
    >
      <Name id={`${id}-name`} className="set-row__name" htmlFor={htmlFor} title={tip}>
        {label}
      </Name>
      {tip !== label && (
        <span id={`${id}-tip`} className="d-sr">
          {tip}
        </span>
      )}
      <div className="set-row__ctl">{children}</div>
      <ScopeIcon scope={scope} />
      {note && (
        <div
          className={cx("set-row__note", noteTone && `set-row__note--${noteTone}`)}
          data-testid={noteTestId}
          title={typeof note === "string" ? note : undefined}
        >
          <span className="set-ellipsis">{note}</span>
        </div>
      )}
    </div>
  );
}

/** An on/off setting: the kit's switch at the row end. */
export function SwitchRow({
  checked,
  onChange,
  disabled,
  ...row
}: Omit<SettingRowProps, "children"> & { checked: boolean; onChange: (on: boolean) => void; disabled?: boolean }) {
  return (
    <SettingRow {...row}>
      <Switch label={row.label} checked={checked} onChange={onChange} disabled={disabled} />
    </SettingRow>
  );
}

/** A short error at the row end, before the control (2 to 5 words). */
export function RowError({ children, title, testId }: { children: ReactNode; title?: string; testId?: string }) {
  return (
    <span className="d-err" role="alert" title={title} data-testid={testId}>
      <CircleX size={12} strokeWidth={1.5} aria-hidden />
      {children}
    </span>
  );
}

/** A numeric knob of the instance form: right-aligned field, unit after, the range when it is out. */
export function NumberRow({
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
  min = 1,
  max,
  before,
  note,
  noteTone,
  noteTestId,
}: {
  id: string;
  label: string;
  help: string;
  value: string;
  onChange: (v: string) => void;
  dirty: boolean;
  field: SettingField;
  envVar: string;
  /** Unit after the field, and in the source note (" s", " MB", " ms"). */
  unit: string;
  /** The env value is in milliseconds while the knob is in seconds (guard). */
  envIsMs?: boolean;
  min?: number;
  max?: number;
  /** Before the field (the live sessions). */
  before?: ReactNode;
  note?: ReactNode;
  noteTone?: "warn" | "failed";
  noteTestId?: string;
}) {
  const error = dirty ? rangeError(value, min, max) : null;
  const shownUnit = (unit === " ms" ? " s" : unit).trim();
  return (
    <SettingRow
      label={label}
      help={help}
      source={sourceNote(field, envVar, unit, envIsMs)}
      keywords={envVar}
      dirty={dirty}
      htmlFor={`setting-${id}`}
      testId={`setting-row-${id}`}
      note={note}
      noteTone={noteTone}
      noteTestId={noteTestId}
    >
      {before}
      {error && <RowError title={`A whole number, ${error}`}>{error}</RowError>}
      <TextInput
        id={`setting-${id}`}
        data-testid={`setting-${id}`}
        type="number"
        min={min}
        max={max}
        value={value}
        invalid={!!error}
        onChange={(e) => onChange(e.target.value)}
        className="set-num"
      />
      <span className="set-unit">{shownUnit}</span>
    </SettingRow>
  );
}
