// What the scoped and the full gate both write: the test commands outside cargo,
// and the way a long list of files is shown.

export const VITEST = "cd frontend && pnpm test";

/** The node tests of the scripts matching `glob` (`scripts/<name>/test/*.test.mjs`). */
export const nodeTests = (glob) => `node --disable-warning=ExperimentalWarning --test '${glob}'`;

/** `files`, the first `shown` of them, then how many more. */
export const listed = (files, shown = 3) =>
  files.length > shown ? `${files.slice(0, shown).join(", ")} (+${files.length - shown} more)` : files.join(", ");
