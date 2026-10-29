/**
 * The source notes of the Settings surface (D6): which tier a knob's value comes from, in
 * words — and, when a stored value wins, that a set env var is shadowed rather than ignored.
 */

import type {
  BoolSettingField,
  EnumSettingFieldWithReason,
  SettingField,
  StringSettingField,
} from "./settings";

/** Human-readable disclosure of which tier a knob's value comes from (D6). */
export function sourceNote(
  field: SettingField,
  envVar: string,
  unit: string,
  envIsMs?: boolean,
): string {
  const envDisplay = field.env != null ? `${envVar}=${field.env}${unit}` : null;
  if (field.source === "stored") {
    // Stored wins — but if an env var is also set, disclose that it is shadowed.
    return envDisplay
      ? `Source: stored value (wins). Env ${envDisplay} is set but overridden.`
      : `Source: stored value (overrides env and default).`;
  }
  if (field.source === "env") {
    const note = envDisplay ? `Source: env ${envDisplay}.` : `Source: env ${envVar}.`;
    return envIsMs ? `${note} (Saving stores it in seconds.)` : note;
  }
  return `Source: built-in default (${field.default}${unit === " ms" ? " s" : unit}).`;
}

/** Which tier the instance default_model comes from (#347). Unlike the numeric
 *  knobs there is no built-in default, so the "default" tier is the account
 *  default (no `--model`). Discloses a shadowed env var too. */
export function modelSourceNote(field: StringSettingField): string {
  const envDisplay = field.env ? `PDO_DEFAULT_MODEL=${field.env}` : null;
  if (field.source === "stored") {
    return envDisplay
      ? `Source: stored value (wins). Env ${envDisplay} is set but overridden.`
      : `Source: stored value (overrides env and account default).`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_DEFAULT_MODEL"}.`;
  }
  return `Source: your Claude account default (no --model).`;
}

/** Which tier the instance default harness comes from (#550). No built-in value: unset,
 *  a node falls to the `claude` floor. Discloses a shadowed env var too. */
export function harnessSourceNote(field: StringSettingField): string {
  const envDisplay = field.env ? `PDO_DEFAULT_HARNESS=${field.env}` : null;
  if (field.source === "stored") {
    return envDisplay
      ? `Source: stored value (wins). Env ${envDisplay} is set but overridden.`
      : `Source: stored value (overrides env and the claude floor).`;
  }
  if (field.source === "env") return `Source: env ${envDisplay ?? "PDO_DEFAULT_HARNESS"}.`;
  return "Source: unset (the claude floor).";
}

/** Which tier turn-end auto-completion comes from (#469). Like the enum knobs there IS
 *  a built-in default (`off`), and both directions of a save are a stored decision — so
 *  "stored (off)" is a meaningful state and must read differently from the default. */
export function autocompleteSourceNote(field: BoolSettingField): string {
  const onOff = (v: boolean) => (v ? "on" : "off");
  const envDisplay =
    field.env != null ? `PDO_AUTOCOMPLETE_TURN_END=${onOff(field.env)}` : null;
  if (field.source === "stored") {
    const base = `Source: stored value (${onOff(field.effective)}).`;
    return envDisplay
      ? `${base} Env ${envDisplay} is set but overridden.`
      : `${base} Overrides env and default.`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_AUTOCOMPLETE_TURN_END"}.`;
  }
  return `Source: built-in default (${onOff(field.default)}).`;
}

/** Which tier the instance default_auto_name comes from (#338). Same shape as
 *  {@link autocompleteSourceNote} — a real built-in default (`on`), and both directions
 *  of a save are a stored decision, so "stored (off)" reads differently from the default. */
export function defaultAutoNameSourceNote(field: BoolSettingField): string {
  const onOff = (v: boolean) => (v ? "on" : "off");
  const envDisplay = field.env != null ? `PDO_DEFAULT_AUTO_NAME=${onOff(field.env)}` : null;
  if (field.source === "stored") {
    const base = `Source: stored value (${onOff(field.effective)}).`;
    return envDisplay
      ? `${base} Env ${envDisplay} is set but overridden.`
      : `${base} Overrides env and default.`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_DEFAULT_AUTO_NAME"}.`;
  }
  return `Source: built-in default (${onOff(field.default)}).`;
}

/** Which tier `review_agent_can_resolve` comes from (#751). A real built-in default
 *  (`off`), and both directions of a save are a stored decision — same shape as
 *  {@link defaultAutoNameSourceNote}. */
export function reviewAgentCanResolveSourceNote(field: BoolSettingField): string {
  const onOff = (v: boolean) => (v ? "on" : "off");
  const envDisplay = field.env != null ? `PDO_REVIEW_AGENT_CAN_RESOLVE=${onOff(field.env)}` : null;
  if (field.source === "stored") {
    const base = `Source: stored value (${onOff(field.effective)}).`;
    return envDisplay
      ? `${base} Env ${envDisplay} is set but overridden.`
      : `${base} Overrides env and default.`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_REVIEW_AGENT_CAN_RESOLVE"}.`;
  }
  return `Source: built-in default (${onOff(field.default)}). Set PDO_REVIEW_AGENT_CAN_RESOLVE=true in the daemon's environment to change it; a stored value wins over both.`;
}

/** Which tier the manager auto-start flag comes from (manager on demand). A real
 *  built-in default (`off`), and both directions of a save are a stored decision —
 *  same shape as {@link defaultAutoNameSourceNote}. */
export function managerEnabledSourceNote(field: BoolSettingField): string {
  const onOff = (v: boolean) => (v ? "on" : "off");
  const envDisplay = field.env != null ? `PDO_MANAGER_ENABLED=${onOff(field.env)}` : null;
  if (field.source === "stored") {
    const base = `Source: stored value (${onOff(field.effective)}).`;
    return envDisplay
      ? `${base} Env ${envDisplay} is set but overridden.`
      : `${base} Overrides env and default.`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_MANAGER_ENABLED"}.`;
  }
  return `Source: built-in default (${onOff(field.default)}) — runs start managerless.`;
}

/** Which tier the manager profile pin comes from (manager on demand). No env tier
 *  and no stored default value: unset IS « Follow the Run ». */
export function managerProfileSourceNote(field: StringSettingField): string {
  if (field.source === "stored" && field.stored) {
    return `Source: stored value (pins the manager to « ${field.stored} »).`;
  }
  return "Source: unset — the manager follows the Run.";
}

/** Which tier the instance default_sandbox comes from (#410). Unlike `default_model` there
 *  IS a built-in default (`off`). Discloses a shadowed env var too. The dangling-profile
 *  `reason` is rendered separately — it is a health signal, not a precedence note. */
export function defaultSandboxSourceNote(field: EnumSettingFieldWithReason): string {
  const envDisplay = field.env ? `PDO_DEFAULT_SANDBOX=${field.env}` : null;
  if (field.source === "stored") {
    return envDisplay
      ? `Source: stored value (wins). Env ${envDisplay} is set but overridden.`
      : `Source: stored value (overrides env and default).`;
  }
  if (field.source === "env") {
    return `Source: env ${envDisplay ?? "PDO_DEFAULT_SANDBOX"}.`;
  }
  return `Source: built-in default (${field.default ?? "off"}).`;
}

/** Which tier the OTLP export switch comes from (ADR-0090). Off by default; both
 *  directions of a save are a stored decision. */
export function telemetryOtlpSourceNote(field: BoolSettingField): string {
  const onOff = (v: boolean) => (v ? "on" : "off");
  const envDisplay = field.env != null ? `PDO_TELEMETRY_OTLP=${onOff(field.env)}` : null;
  if (field.source === "stored") {
    const base = `Source: stored setting (${onOff(field.effective)}).`;
    return envDisplay ? `${base} Env ${envDisplay} is set but overridden.` : base;
  }
  if (field.source === "env") return `Source: env ${envDisplay ?? "PDO_TELEMETRY_OTLP"}.`;
  return "Source: built-in default (off). Nothing leaves this instance.";
}

/** Which tier a telemetry switch that is not the export's own comes from (ADR-0090): the
 *  `/metrics` switch, « include the Actor ». Off by default. */
export function telemetryFlagSourceNote(field: BoolSettingField, envVar: string): string {
  const onOff = (v: boolean) => (v ? "on" : "off");
  const envDisplay = field.env != null ? `${envVar}=${onOff(field.env)}` : null;
  if (field.source === "stored") {
    return envDisplay
      ? `Source: stored setting. Env ${envDisplay} is set but overridden. Off by default.`
      : "Source: stored setting. Off by default.";
  }
  if (field.source === "env") return `Source: env ${envDisplay ?? envVar}. Off by default.`;
  return "Source: built-in default (off).";
}

/** Which tier the collector's address comes from (ADR-0090). No built-in address. */
export function telemetryEndpointSourceNote(field: StringSettingField): string {
  const envDisplay = field.env ? `PDO_TELEMETRY_OTLP_ENDPOINT=${field.env}` : null;
  if (field.source === "stored") {
    return envDisplay
      ? `Source: stored setting. Env ${envDisplay} is set but overridden.`
      : "Source: stored setting.";
  }
  if (field.source === "env") return `Source: env ${envDisplay ?? "PDO_TELEMETRY_OTLP_ENDPOINT"}.`;
  return "Source: none set.";
}

/** Which tier the Public URL comes from. No built-in value: unset, links stay on the
 *  daemon's own address. Discloses a shadowed `PDO_PUBLIC_URL` too. */
export function publicUrlSourceNote(field: StringSettingField): string {
  const envDisplay = field.env ? `PDO_PUBLIC_URL=${field.env}` : null;
  if (field.source === "stored") {
    return envDisplay
      ? `Source: stored setting (wins). Env ${envDisplay} is set but overridden.`
      : "Source: stored setting (overrides PDO_PUBLIC_URL).";
  }
  if (field.source === "env") return `Source: env ${envDisplay ?? "PDO_PUBLIC_URL"}.`;
  return "Source: none set (PDO_PUBLIC_URL unset).";
}

/** Which tier the clones folder comes from (CORE-97): stored, else the built-in default. */
export function clonesFolderSourceNote(field: StringSettingField | undefined): string {
  if (field?.source === "stored") return "Source: stored setting.";
  return `Source: built-in default (${field?.default ?? "~/pdo/repos"}).`;
}
