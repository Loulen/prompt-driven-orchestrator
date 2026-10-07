// The front's shared module `@pdo/extension-ui`, for the extensions' UI bundles
// (ADR-0081, amendment #32): the import map of index.html points the bare
// specifier here, and the front puts the module on `globalThis.__PDO_SHARED__`
// before it renders anything (src/shared/extensions/uiModule.ts). A bundle lists
// and renders the contributions to the locations it hosts through it, and never
// ships its own copy.
const ExtensionUi = globalThis.__PDO_SHARED__.extensionUi;
export default ExtensionUi;
export const {
  version,
  major,
  useHostedContributions,
  runHostedCommand,
  HostedContribution,
  HostedContributions,
  useCurrentActor,
  useActorDisplay,
  openProbedSocket,
} = ExtensionUi;
