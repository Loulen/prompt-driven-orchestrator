import type { CSSProperties } from "react";
import type { HarnessCatalog, HarnessOption } from "../lib/harness";

/**
 * The dynamic harness picker (#586, ADR-0045/0046).
 *
 * One native `<select>` split into two `<optgroup>` sections — **Built-in** and
 * **From descriptors** — listing harness NAMES only (no capability pills; the
 * "simple" direction). A harness whose binary is not installed (absent from the
 * daemon's `$PATH`) renders **greyed and non-selectable** with a discreet
 * "not installed" note, because spawning it would fail fast (ADR-0037). The first
 * option is always the inherit sentinel (value `""`), whose label the caller
 * supplies — each surface names what `""` resolves to differently.
 *
 * Shared by all four harness surfaces (node inspector, New Run, Projet, Settings
 * default) so the sectioning, greying, and "not installed" rendering live in one
 * place and can never drift between them.
 */
export default function HarnessSelect({
  value,
  onChange,
  catalog,
  inheritLabel,
  id,
  className,
  style,
  disabled,
  "data-testid": testId,
}: {
  /** The selected harness name, or `""` for the inherit sentinel. */
  value: string;
  onChange: (value: string) => void;
  catalog: HarnessCatalog;
  /** Label for the inherit option (value `""`), e.g. "Use instance default (claude)". */
  inheritLabel: string;
  id?: string;
  className?: string;
  style?: CSSProperties;
  disabled?: boolean;
  "data-testid"?: string;
}) {
  const renderOption = (o: HarnessOption) => (
    <option key={o.name} value={o.name} disabled={!o.installed}>
      {o.installed ? o.name : `${o.name} — not installed`}
    </option>
  );
  return (
    <select
      id={id}
      data-testid={testId}
      value={value}
      onChange={(e) => onChange(e.target.value)}
      disabled={disabled}
      className={className}
      style={style}
    >
      <option value="">{inheritLabel}</option>
      {catalog.builtin.length > 0 && (
        <optgroup label="Built-in">{catalog.builtin.map(renderOption)}</optgroup>
      )}
      {catalog.descriptors.length > 0 && (
        <optgroup label="From descriptors">
          {catalog.descriptors.map(renderOption)}
        </optgroup>
      )}
    </select>
  );
}
