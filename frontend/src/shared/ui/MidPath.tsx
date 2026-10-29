/**
 * A path on one line, cut in its middle: the folders shrink with an ellipsis,
 * the last segment stays whole while it fits
 * (`~/work/clients/very-lo…/shop-web`). The full path is in the tooltip
 * unless `title` is false (the owner carries it).
 */
export default function MidPath({
  path,
  className,
  title = true,
  testId,
}: {
  path: string;
  className?: string;
  title?: boolean;
  testId?: string;
}) {
  const trimmed = path.replace(/\/+$/, "");
  const cut = trimmed.lastIndexOf("/");
  const head = cut > 0 ? trimmed.slice(0, cut) : "";
  const tail = cut > 0 ? trimmed.slice(cut) : trimmed || path;
  return (
    <span className={className ? `ln-path ${className}` : "ln-path"} title={title ? path : undefined} data-testid={testId}>
      {head && (
        <span className="ln-path__head" data-testid={testId ? `${testId}-head` : undefined}>
          {head}
        </span>
      )}
      <span className="ln-path__tail" data-testid={testId ? `${testId}-tail` : undefined}>
        {tail}
      </span>
    </span>
  );
}
