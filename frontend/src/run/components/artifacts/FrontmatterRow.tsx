/** One `field: value` line of an artifact's frontmatter card. */
export default function FrontmatterRow({ field, value }: { field: string; value: unknown }) {
  const display =
    typeof value === "object" ? JSON.stringify(value) : String(value);
  return (
    <>
      <span className="text-fg-3">{field}</span>
      <span className="text-fg">{display}</span>
    </>
  );
}
