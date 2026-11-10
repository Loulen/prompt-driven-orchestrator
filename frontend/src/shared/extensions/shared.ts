/**
 * React, shared with the extensions' UI bundles (ADR-0081, D11). A bundle
 * imports `react` as a bare specifier; the import map of `index.html` resolves
 * it to `public/shared/react.js`, which re-exports what this module puts on
 * `globalThis.__PDO_SHARED__`. One React for the front and every extension:
 * hooks, context and the tree work across the boundary.
 *
 * The shared module `@pdo/extension-ui` (amendment #32, `uiModule.ts`) goes the
 * same way, through `public/shared/pdo-extension-ui.js`; and so does yjs (API
 * 0.20), through `public/shared/yjs.js`: one copy, whose documents and types
 * the front and every extension share.
 */

import * as React from "react";
import * as ReactDOM from "react-dom";
import * as JsxRuntime from "react/jsx-runtime";
import * as Y from "yjs";
import type { ExtensionUiModule } from "./uiModule";

export interface SharedModules {
  react: typeof React;
  reactDom: typeof ReactDOM;
  jsxRuntime: typeof JsxRuntime;
  yjs: typeof Y;
  extensionUi?: ExtensionUiModule;
}

declare global {
  var __PDO_SHARED__: SharedModules | undefined;
}

/** Put the front's React, its yjs, and the shared module `extensionUi`,
 *  where the import map's shims read them. Idempotent. */
export function shareWithExtensions(extensionUi?: ExtensionUiModule): void {
  globalThis.__PDO_SHARED__ = {
    react: React,
    reactDom: ReactDOM,
    jsxRuntime: JsxRuntime,
    yjs: Y,
    extensionUi,
  };
}

/** The major version of the React the front runs (and shares). */
export function hostReactMajor(): number {
  return Number.parseInt(React.version.split(".")[0], 10);
}
