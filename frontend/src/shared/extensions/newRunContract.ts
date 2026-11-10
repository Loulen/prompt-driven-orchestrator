/**
 * What an extension's UI opens the core's New Run form with (API 0.22), part
 * of the contract of `contract.ts`, which re-exports it. The seam itself is
 * `newRun.ts`.
 */

/**
 * What New Run opens with when an extension's UI opens it (API 0.22,
 * `openNewRun` of `@pdo/extension-ui`, `CommandHost.openNewRun`): every field
 * optional, each one shown in the form and left editable. A Projet the form
 * does not offer (unknown, or not allowed to the operator) is ignored;
 * without one, the form takes the first offered Projet holding
 * `repositories[0]`, else its default. `repositories[0]` is the primary
 * repository, the others its secondaries. The Pipeline stays the operator's
 * choice.
 */
export interface NewRunPrefill {
  /** The id of the Projet to launch in. */
  project?: string;
  /** The target repositories, the primary first. */
  repositories?: string[];
  /** The Run's name (auto-naming is turned off). */
  name?: string;
  /** The prompt of the entry node. */
  input?: string;
  /**
   * The id of the extension that opens the form: only its `run/new` sections
   * receive `data`. Declared, not checked: the extension's `validate_create`
   * is the boundary. `CommandHost.openNewRun` sets it.
   */
  extension?: string;
  /** For the caller's own `run/new` sections, as `RunNewContext.prefill`. */
  data?: unknown;
}
