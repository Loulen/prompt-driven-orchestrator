import { frontmatterValue } from "../../model/frontmatter";

/** One `field: value` item of an artifact's frontmatter strip. */
export default function FrontmatterRow({ field, value }: { field: string; value: unknown }) {
  const display = frontmatterValue(value);
  return (
    <div className="art__fm-item">
      <dt>{field}</dt>
      <dd title={display}>{display}</dd>
    </div>
  );
}
