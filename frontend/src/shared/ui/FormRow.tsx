import type { ReactNode } from "react";

/**
 * A row of a label-left form (New Run, the Project modal): the label in a
 * fixed column, then the control(s) on one line. `top` aligns the label with
 * the first line of a tall control. The label's `title` carries the help: no
 * sentence under the field.
 */
export default function FormRow({
  label,
  htmlFor,
  title,
  top,
  children,
  testId,
}: {
  label: ReactNode;
  htmlFor?: string;
  title?: string;
  top?: boolean;
  children: ReactNode;
  testId?: string;
}) {
  return (
    <div className={top ? "ln-row ln-row--top" : "ln-row"} data-testid={testId}>
      {htmlFor ? (
        <label className="ln-row__label" htmlFor={htmlFor} title={title}>
          {label}
        </label>
      ) : (
        <span className="ln-row__label" title={title}>
          {label}
        </span>
      )}
      <div className="ln-row__ctl">{children}</div>
    </div>
  );
}

/** A caps tag and a rule between the sections of a label-left form. */
export function FormSection({ label, mark, children, testId }: { label?: ReactNode; mark?: ReactNode; children: ReactNode; testId?: string }) {
  return (
    <section className="ln-sec" data-testid={testId}>
      {label && (
        <span className="d-caps ln-sec__tag">
          {label}
          {mark}
        </span>
      )}
      {children}
    </section>
  );
}
