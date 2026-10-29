/** One `field: value` item of an artifact's frontmatter strip. */
export default function FrontmatterRow({ field, value }: { field: string; value: unknown }) {
  const display =
    typeof value === "object" ? JSON.stringify(value) : String(value);
  return (
    <div className="art__fm-item">
      <dt>{field}</dt>
      <dd title={display}>{display}</dd>
    </div>
  );
}
