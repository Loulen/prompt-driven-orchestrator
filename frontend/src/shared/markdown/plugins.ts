/**
 * What every markdown render of the core shares: GFM, deep links turned into
 * links (`remarkExtensionLinks`), and the `a` that follows them
 * (`MarkdownLink`). Module-scope constants on purpose: react-markdown remounts a
 * custom component whose identity changes (#369).
 */

import type { Components } from "react-markdown";
import remarkGfm from "remark-gfm";
import MarkdownLink from "./MarkdownLink";
import remarkExtensionLinks from "./remarkExtensionLinks";

export const REMARK_PLUGINS = [remarkGfm, remarkExtensionLinks];

export const MARKDOWN_COMPONENTS: Components = { a: MarkdownLink };
