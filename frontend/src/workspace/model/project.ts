/**
 * The Projet as the UI reads it: the generated wire type, with the agentic
 * profile and the skills selection it carries typed by the contexts that own
 * them (the workspace stores them without reading them).
 */

import type { AgentChoice } from "../../shared/generated/AgentChoice";
import type { Project as ProjectOf } from "../../shared/generated/Project";
import type { SkillRef } from "../../shared/generated/SkillRef";

export type Project = ProjectOf<AgentChoice, SkillRef>;
