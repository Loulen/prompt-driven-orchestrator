import type { AnchorHTMLAttributes, MouseEvent } from "react";
import { followDeepLink, isDeepLink, isRunLink } from "../extensions/deepLink";

/**
 * The `a` of every markdown render of the core. A deep link
 * (`#/ext/<extension>/<location>/<path>`) opens the extension's page, from any
 * surface, the Review page included (ADR-0081, amendment #29); a link toward a
 * Run (`#/runs/<id>`, CORE-1) selects it the same way; an absolute
 * http(s) link leaves the app in a new tab (#698); any other link is left as is.
 */
export default function MarkdownLink({
  href,
  children,
  ...props
}: AnchorHTMLAttributes<HTMLAnchorElement> & { node?: unknown }) {
  // react-markdown hands its hast `node` too: never an attribute of the <a>.
  const { node, ...rest } = props;
  void node;
  if (isDeepLink(href) || isRunLink(href)) {
    const onClick = (event: MouseEvent<HTMLAnchorElement>) => {
      if (event.button !== 0 || event.metaKey || event.ctrlKey || event.shiftKey) return;
      event.preventDefault();
      followDeepLink(href);
    };
    return (
      <a href={href} {...rest} onClick={onClick} data-deep-link="">
        {children}
      </a>
    );
  }
  const external = typeof href === "string" && /^https?:\/\//i.test(href);
  return (
    <a href={href} {...rest} {...(external ? { target: "_blank", rel: "noreferrer" } : {})}>
      {children}
    </a>
  );
}
