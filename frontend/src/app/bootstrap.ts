import { initTheme } from "../hooks/useTheme";
import { extensionUiModule } from "../shared/extensions/uiModule";
import { shareWithExtensions } from "../shared/extensions/shared";
import { useEditStore } from "../pipeline/store/editStore";
import { startDraftSessions } from "../pipeline/store/draftSessions";

/**
 * Process-wide setup that must run once, before the first render.
 *
 * #759: adopt the stored theme and start following the OS. The inline guard in
 * `index.html` already stamped the same value before first paint; this call
 * takes ownership of it, so a later `prefers-color-scheme` flip is honoured
 * while the preference is `system`.
 *
 * ADR-0081: share the front's React, and the shared module `@pdo/extension-ui`
 * (amendment #32), with the extensions' UI bundles before any of them can load.
 *
 * ADR-0088: every pipeline tab edits its shared draft with the others.
 */
export function bootstrap(): void {
  initTheme();
  shareWithExtensions(extensionUiModule);
  startDraftSessions(useEditStore);
}
