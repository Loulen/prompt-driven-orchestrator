

export function Breadcrumb({
  crumbs,
  testid = "stats-cost-breadcrumb",
}: {
  crumbs: { label: string; onClick?: () => void }[];
  /** The Performance « By model » axis reuses the same breadcrumb under its own
   *  test id (#737). */
  testid?: string;
}) {
  return (
    <div
      className="mb-3 text-fg-4"
      style={{ fontSize: "10.5px" }}
      data-testid={testid}
    >
      {crumbs.map((crumb, index) => (
        <span key={`${crumb.label}-${index}`}>
          {index > 0 ? " / " : ""}
          {crumb.onClick ? (
            <button
              type="button"
              aria-label={`Back to ${crumb.label}`}
              onClick={crumb.onClick}
              className="hover:text-fg-2 underline decoration-dotted underline-offset-2"
            >
              {crumb.label}
            </button>
          ) : (
            <span className="text-fg-3">{crumb.label}</span>
          )}
        </span>
      ))}
    </div>
  );
}
