export function SectionHead({
  title,
  count,
  onAdd,
}: {
  title: string;
  count?: number;
  onAdd?: () => void;
}) {
  return (
    <div className="flex items-center justify-between border-b border-line-soft pb-1 pt-1">
      <span className="font-medium text-fg-3" style={{ fontSize: "10px", textTransform: "uppercase", letterSpacing: "0.05em" }}>
        {title}
        {count !== undefined && (
          <span className="ml-1.5 rounded bg-bg-4 px-1 text-fg-4">{count}</span>
        )}
      </span>
      {onAdd && (
        <button
          onClick={onAdd}
          className="cursor-pointer text-fg-4 hover:text-acc"
          style={{ fontSize: "10px" }}
        >
          + Add
        </button>
      )}
    </div>
  );
}

export function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <label className="mb-0.5 block text-fg-3" style={{ fontSize: "10px" }}>
        {label}
      </label>
      {children}
    </div>
  );
}
