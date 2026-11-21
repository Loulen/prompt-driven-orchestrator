import { useMemo, type ComponentProps } from "react";
import ExtensionIcon from "../../../shared/extensions/ExtensionIcon";
import type { Icon } from "../../../shared/ui/kit";
import type { BugReportDestination } from "../../model/bugReport";
import { GithubMark } from "./GithubMark";

type IconProps = ComponentProps<Icon>;

/** An extension destination's icon, as the kit's `Icon`: its SVG, or its label's first letter. */
function extensionIcon(url: string | null | undefined, label: string): Icon {
  return function DestinationIcon({ size, className }: IconProps) {
    return (
      <span className={className} style={{ display: "inline-flex" }}>
        <ExtensionIcon url={url} label={label} size={typeof size === "number" ? size : 13} />
      </span>
    );
  };
}

/** The icon of each destination, by id, made once per list so a row keeps its icon across renders. */
export function useDestinationIcons(destinations: BugReportDestination[]): Map<string, Icon> {
  return useMemo(
    () =>
      new Map(
        destinations.map((d) => [d.id, d.extension ? extensionIcon(d.icon, d.label) : GithubMark] as const),
      ),
    [destinations],
  );
}
