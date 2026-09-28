/**
 * React, shared with the extensions' UI bundles (ADR-0081, D11). A bundle
 * imports `react` as a bare specifier; the import map of `index.html` resolves
 * it to `public/shared/react.js`, which re-exports what this module puts on
 * `globalThis.__PDO_SHARED__`. One React for the front and every extension:
 * hooks, context and the tree work across the boundary.
 */

import * as React from "react";
import * as ReactDOM from "react-dom";
import * as JsxRuntime from "react/jsx-runtime";

export interface SharedModules {
  react: typeof React;
  reactDom: typeof ReactDOM;
  jsxRuntime: typeof JsxRuntime;
}

declare global {
  var __PDO_SHARED__: SharedModules | undefined;
}

/** Put the front's React where the import map's shims read it. Idempotent. */
export function shareReactWithExtensions(): void {
  globalThis.__PDO_SHARED__ = { react: React, reactDom: ReactDOM, jsxRuntime: JsxRuntime };
}

/** The major version of the React the front runs (and shares). */
export function hostReactMajor(): number {
  return Number.parseInt(React.version.split(".")[0], 10);
}
