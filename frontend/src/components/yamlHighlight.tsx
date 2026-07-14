import type { ReactNode } from "react";

// Syntax-highlight a YAML document into React nodes (#100, #345). Shared by the
// pipeline YAML tab (`PipelineInfoPanel`) and the node-export modal
// (`ExportNodeYamlModal`). It lives in its own module so neither component file
// exports both a component and this helper, which would break React Fast
// Refresh (`react-refresh/only-export-components`).
export function highlightYaml(yaml: string, errorLine?: number): ReactNode {
  const lines = yaml.split("\n");
  return lines.map((line, i) => {
    const lineNum = i + 1;
    const isError = errorLine != null && lineNum === errorLine;
    const commentIdx = line.indexOf("#");
    return (
      <span
        key={i}
        className={isError ? "bg-st-failed/20" : undefined}
        data-line={lineNum}
      >
        {commentIdx >= 0 ? (
          <>
            {highlightLine(line.slice(0, commentIdx))}
            <span className="text-fg-4 italic">{line.slice(commentIdx)}</span>
          </>
        ) : (
          highlightLine(line)
        )}
        {i < lines.length - 1 ? "\n" : null}
      </span>
    );
  });
}

function highlightLine(line: string): ReactNode {
  const keyMatch = line.match(/^(\s*-?\s*)([a-zA-Z_][\w]*)\s*:/);
  if (keyMatch) {
    const [, indent, key] = keyMatch;
    const rest = line.slice(keyMatch[0].length);
    return (
      <>
        {indent}
        <span className="text-acc">{key}</span>
        <span className="text-fg-4">:</span>
        {highlightValue(rest)}
      </>
    );
  }

  const listMatch = line.match(/^(\s*-\s+)(.*)/);
  if (listMatch) {
    const [, prefix, rest] = listMatch;
    return (
      <>
        <span className="text-fg-4">{prefix}</span>
        {highlightValue(rest)}
      </>
    );
  }

  return line;
}

function highlightValue(value: string): ReactNode {
  const trimmed = value.trimStart();
  const leading = value.slice(0, value.length - trimmed.length);

  if (/^".*"$/.test(trimmed) || /^'.*'$/.test(trimmed)) {
    return <>{leading}<span className="text-st-await">{trimmed}</span></>;
  }

  if (/^(true|false|null)$/.test(trimmed)) {
    return <>{leading}<span className="text-st-running">{trimmed}</span></>;
  }

  if (/^-?\d+(\.\d+)?$/.test(trimmed)) {
    return <>{leading}<span className="text-st-done">{trimmed}</span></>;
  }

  if (/^\{.*\}$/.test(trimmed)) {
    return <>{leading}<span className="text-fg-3">{trimmed}</span></>;
  }

  return <>{leading}<span className="text-fg-2">{trimmed}</span></>;
}
