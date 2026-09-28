/**
 * The entity attributes (CONTEXT.md § Extensions): prefixed key → value, as the
 * daemon returns them with an entity. A filter keeps an entity when it carries
 * every attribute asked for — the rule of `?attr.<id>/<key>=<value>` on the
 * list endpoints.
 */

export type Attributes = { [key in string]?: string };

export function matchesAttributes(carried: Attributes | undefined, filter: Attributes): boolean {
  return Object.entries(filter).every(([key, value]) => carried?.[key] === value);
}

export function isAttributeFilterActive(filter: Attributes): boolean {
  return Object.keys(filter).length > 0;
}
