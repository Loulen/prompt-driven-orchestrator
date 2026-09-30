import { Info } from "lucide-react";
import { hostReactMajor } from "./shared";
import { useExtensions } from "./store";
import { UI_MODULE_MAJOR } from "./uiModuleVersion";

/**
 * The banner of the UI bundles that were not loaded because they were built for
 * another major of React than the front's, or of the shared module
 * `@pdo/extension-ui`. Styled like the other advisory
 * banners (the lint banner). Nothing when every bundle is compatible.
 */
export default function ExtensionBanner() {
  const incompatible = useExtensions().filter((e) => e.status === "incompatible");
  if (incompatible.length === 0) return null;
  const host = hostReactMajor();
  return (
    <div data-testid="extension-banner" className="flex flex-col gap-1 px-3 pt-2">
      {incompatible.map(({ entry, incompatibility }) => (
        <div
          key={entry.id}
          role="status"
          data-testid={`extension-banner-${entry.id}`}
          className="flex items-start gap-2 rounded border border-st-await/30 bg-st-await/5 px-2 py-1.5 text-fg-3"
          style={{ fontSize: "10.5px" }}
        >
          <Info size={12} className="mt-0.5 shrink-0 text-st-await" />
          {incompatibility === "ui-module" ? (
            <span>
              The interface of the extension {entry.name} was not loaded: it was built for the PDO
              extension UI module {entry.ui?.ui_module_major ?? "?"}, and PDO runs version{" "}
              {UI_MODULE_MAJOR}. Rebuild the extension against pdo-extension-api {UI_MODULE_MAJOR}.
            </span>
          ) : (
            <span>
              The interface of the extension {entry.name} was not loaded: it was built for React{" "}
              {entry.ui?.react_major ?? "?"}, and PDO runs React {host}. Rebuild the extension
              against React {host}.
            </span>
          )}
        </div>
      ))}
    </div>
  );
}
