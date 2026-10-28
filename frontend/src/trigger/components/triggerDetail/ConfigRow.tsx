/** One labelled row of a Trigger's configuration. */
export default function ConfigRow({
  icon,
  label,
  children,
}: {
  icon: React.ReactNode;
  label: string;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-center gap-2">
      <span className="flex w-20 shrink-0 items-center gap-1.5 text-text-3">
        {icon}
        {label}
      </span>
      <div className="min-w-0 flex-1 truncate">{children}</div>
    </div>
  );
}
