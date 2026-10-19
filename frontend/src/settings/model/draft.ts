/**
 * The instance form of the Settings surface as the inputs hold it: the draft, what differs
 * from the loaded settings, and the PUT payload it becomes. Pure — the surface keeps the
 * draft as overrides on top of the loaded settings, and the dirty dot, the Save payload and
 * the unsaved-changes guard all read these same comparisons.
 */

import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { SkillRef } from "../../shared/generated/SkillRef";
import type { UpdateSettingsRequest } from "../../shared/generated/UpdateSettingsRequest";
import type { InstanceSettings } from "./settings";
import type { SettingsFieldId } from "./settingsCategories";

/** The editable instance form, as strings/values the inputs hold. */
export interface DraftValues {
  capStr: string;
  ttlStr: string;
  guardStr: string;
  /** #779: the per-run attachment budget (MB), as typed. */
  attachmentsStr: string;
  model: string | null;
  defaultHarness: string;
  agentChoice: AgentChoice | null;
  instanceSkills: SkillRef[];
  harnessModels: Record<string, string>;
  defaultSandbox: string;
  autocompleteTurnEnd: boolean;
  defaultAutoName: boolean;
  managerEnabled: boolean;
  managerProfile: string;
  /** #751: may an agent's `--resolved` reply resolve a review comment directly? */
  reviewAgentCanResolve: boolean;
  /** The stored Public URL, as typed; `""` is unset. */
  publicUrl: string;
}

export function seedFrom(settings: InstanceSettings): DraftValues {
  return {
    capStr: String(settings.session_cap.effective),
    ttlStr: String(settings.reaper_ttl_secs.effective),
    guardStr: String(settings.guard_timeout_secs.effective),
    attachmentsStr: String(settings.max_attachments_mb.effective),
    model: settings.default_model.effective,
    defaultHarness: settings.default_harness.effective ?? "",
    agentChoice: settings.agent_choice ?? null,
    instanceSkills: settings.skills ?? [],
    harnessModels: { ...settings.default_harness_model.stored },
    defaultSandbox: settings.default_sandbox.effective ?? "off",
    autocompleteTurnEnd: settings.autocomplete_turn_end.effective,
    defaultAutoName: settings.default_auto_name.effective,
    managerEnabled: settings.manager_enabled.effective,
    // "" = « Follow the Run » (the clear sentinel on the wire too).
    managerProfile: settings.manager_profile.stored ?? "",
    reviewAgentCanResolve: settings.review_agent_can_resolve.effective,
    publicUrl: settings.public_url.stored ?? "",
  };
}

/** A Public URL the daemon will accept: an origin `http(s)://host[:port]`, no path. */
export function isPublicUrl(raw: string): boolean {
  return /^https?:\/\/[^\s/?#:@][^\s/?#@]*\/*$/i.test(raw.trim());
}

/** The per-harness map the daemon will store: trimmed, empty entries dropped. */
function normaliseHarnessModels(map: Record<string, string>): Record<string, string> {
  const next: Record<string, string> = {};
  for (const [name, raw] of Object.entries(map)) {
    const v = raw.trim();
    if (v) next[name] = v;
  }
  return next;
}

function sameMap(a: Record<string, string>, b: Record<string, string>): boolean {
  return (
    Object.keys(a).length === Object.keys(b).length &&
    Object.entries(a).every(([k, v]) => b[k] === v)
  );
}

function numericDirty(str: string, effective: number): boolean {
  const t = str.trim();
  if (t === "") return true;
  return Number(t) !== effective;
}

/**
 * Which fields of the draft differ from the loaded settings. The same comparisons the
 * Save payload uses, so a dirty dot and a PUT key always agree.
 */
export function computeDirty(values: DraftValues, settings: InstanceSettings): Set<SettingsFieldId> {
  const dirty = new Set<SettingsFieldId>();
  if (numericDirty(values.capStr, settings.session_cap.effective)) dirty.add("session-cap");
  if (numericDirty(values.ttlStr, settings.reaper_ttl_secs.effective)) dirty.add("reaper-ttl");
  if (numericDirty(values.guardStr, settings.guard_timeout_secs.effective)) {
    dirty.add("guard-timeout");
  }
  if (numericDirty(values.attachmentsStr, settings.max_attachments_mb.effective)) {
    dirty.add("max-attachments-mb");
  }
  if (values.autocompleteTurnEnd !== settings.autocomplete_turn_end.effective) {
    dirty.add("autocomplete-turn-end");
  }
  if (values.defaultAutoName !== settings.default_auto_name.effective) {
    dirty.add("default-auto-name");
  }
  if (values.reviewAgentCanResolve !== settings.review_agent_can_resolve.effective) {
    dirty.add("review-agent-can-resolve");
  }
  if (values.model !== settings.default_model.effective) dirty.add("default-model");
  if (values.defaultHarness !== (settings.default_harness.effective ?? "")) {
    dirty.add("default-harness");
  }
  if (JSON.stringify(values.agentChoice) !== JSON.stringify(settings.agent_choice ?? null)) {
    dirty.add("agent-choice");
  }
  const storedSkillIds = (settings.skills ?? []).map((skill) => skill.id).join("\u0000");
  if (values.instanceSkills.map((skill) => skill.id).join("\u0000") !== storedSkillIds) {
    dirty.add("skills");
  }
  if (
    !sameMap(normaliseHarnessModels(values.harnessModels), settings.default_harness_model.stored)
  ) {
    dirty.add("harness-models");
  }
  if (values.defaultSandbox !== settings.default_sandbox.effective) dirty.add("default-sandbox");
  if (values.managerEnabled !== settings.manager_enabled.effective) dirty.add("manager-enabled");
  if (values.managerProfile !== (settings.manager_profile.stored ?? "")) {
    dirty.add("manager-profile");
  }
  if (values.publicUrl.trim() !== (settings.public_url.stored ?? "")) dirty.add("public-url");
  return dirty;
}

/**
 * The default sandbox names a staging profile that no longer exists. The select keeps the
 * stored name under a « missing » option (never a silent rewrite) and Save refuses it.
 */
export function missingDefaultProfile(values: DraftValues, settings: InstanceSettings): boolean {
  return (
    values.defaultSandbox !== "off" &&
    values.defaultSandbox !== "" &&
    !settings.sandbox_profiles.some((p) => p.name === values.defaultSandbox)
  );
}

/**
 * Validate the draft and build the PUT payload: only the fields that changed, each in the
 * wire form the daemon stores. An error names the first invalid field.
 */
export function buildPatch(
  values: DraftValues,
  settings: InstanceSettings,
  dirty: Set<SettingsFieldId>,
): { patch: UpdateSettingsRequest } | { error: string } {
  if (missingDefaultProfile(values, settings)) {
    return {
      error:
        `No staging profile named \`${values.defaultSandbox}\` any more. Pick another one (or ` +
        "`off`) — a Run that falls back to a missing profile fails at launch.",
    };
  }
  const patch: UpdateSettingsRequest = {};

  const capT = values.capStr.trim();
  const cap = Number(capT);
  if (capT === "" || !Number.isInteger(cap) || cap < 1) {
    if (capT !== "" || dirty.has("session-cap")) {
      return { error: "Session cap must be a whole number ≥ 1." };
    }
  } else if (cap !== settings.session_cap.effective) patch.session_cap = cap;

  const ttlT = values.ttlStr.trim();
  const ttl = Number(ttlT);
  if (ttlT === "" || !Number.isInteger(ttl) || ttl < 1) {
    if (ttlT !== "" || dirty.has("reaper-ttl")) {
      return { error: "Reaper TTL must be a whole number ≥ 1 second." };
    }
  } else if (ttl !== settings.reaper_ttl_secs.effective) patch.reaper_ttl_secs = ttl;

  const guardT = values.guardStr.trim();
  const guard = Number(guardT);
  if (guardT === "" || !Number.isInteger(guard) || guard < 1 || guard > 600) {
    if (guardT !== "" || dirty.has("guard-timeout")) {
      return { error: "Guard timeout must be a whole number between 1 and 600 seconds." };
    }
  } else if (guard !== settings.guard_timeout_secs.effective) {
    patch.guard_timeout_secs = guard;
  }

  // #779: the attachment budget — the daemon refuses outside 1–4096 MB.
  const attT = values.attachmentsStr.trim();
  const att = Number(attT);
  if (attT === "" || !Number.isInteger(att) || att < 1 || att > 4096) {
    if (attT !== "" || dirty.has("max-attachments-mb")) {
      return { error: "Max attachments per run must be a whole number between 1 and 4096 MB." };
    }
  } else if (att !== settings.max_attachments_mb.effective) {
    patch.max_attachments_mb = att;
  }

  // Model: `null` (Default) clears via the "" sentinel; a string sets it.
  if (values.model !== settings.default_model.effective) {
    patch.default_model = values.model ?? "";
  }
  // #550: the harness axis. `""` clears the default harness (same sentinel as the model).
  if (values.defaultHarness !== (settings.default_harness.effective ?? "")) {
    patch.default_harness = values.defaultHarness;
  }
  if (JSON.stringify(values.agentChoice) !== JSON.stringify(settings.agent_choice ?? null)) {
    patch.agent_choice = values.agentChoice;
  }
  // #669: sent whole when the id list changed (an empty list clears the tier).
  const storedSkillIds = (settings.skills ?? []).map((skill) => skill.id).join("\u0000");
  if (values.instanceSkills.map((skill) => skill.id).join("\u0000") !== storedSkillIds) {
    patch.skills = values.instanceSkills;
  }
  // #616 (correctif 1): the per-harness map is sent WHOLE, so a harness whose row was
  // not touched (or not even shown) keeps its stored default.
  const nextModels = normaliseHarnessModels(values.harnessModels);
  if (!sameMap(nextModels, settings.default_harness_model.stored)) {
    patch.default_harness_model = nextModels;
  }
  // Default sandbox (#410): a concrete variant, only sent when it changed.
  if (values.defaultSandbox !== settings.default_sandbox.effective) {
    patch.default_sandbox = values.defaultSandbox;
  }
  // Plain bools (#469, #338): `false` PERSISTS as a stored `0` — never a clear sentinel.
  if (values.autocompleteTurnEnd !== settings.autocomplete_turn_end.effective) {
    patch.autocomplete_turn_end = values.autocompleteTurnEnd;
  }
  if (values.defaultAutoName !== settings.default_auto_name.effective) {
    patch.default_auto_name = values.defaultAutoName;
  }
  // #751: same plain-bool discipline — `false` persists as a stored `0`.
  if (values.reviewAgentCanResolve !== settings.review_agent_can_resolve.effective) {
    patch.review_agent_can_resolve = values.reviewAgentCanResolve;
  }
  // Manager on demand: the flag is a plain bool (`false` persists as a stored
  // `0`); the pin is a profile NAME with `""` as the clear sentinel (« Follow
  // the Run »).
  if (values.managerEnabled !== settings.manager_enabled.effective) {
    patch.manager_enabled = values.managerEnabled;
  }
  if (values.managerProfile !== (settings.manager_profile.stored ?? "")) {
    patch.manager_profile = values.managerProfile.trim();
  }
  // `""` clears the stored Public URL (the env tier, then none, decide again).
  const publicUrl = values.publicUrl.trim();
  if (publicUrl !== (settings.public_url.stored ?? "")) {
    if (publicUrl !== "" && !isPublicUrl(publicUrl)) {
      return { error: "Public URL must be an http:// or https:// origin (host and port, no path)." };
    }
    patch.public_url = publicUrl;
  }
  return { patch };
}
