/** `2 drafts` / `1 draft` — pluralised counter. */
export function plural(n: number, word: string): string {
  return `${n} ${word}${n === 1 ? "" : "s"}`;
}
