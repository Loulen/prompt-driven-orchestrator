/**
 * The controls of the shared design system (ADR-0091): buttons, choices,
 * inputs and the form row. One look each, the one retained by the design
 * (`kit.*` choices); styles in kit.css, tokens only.
 */
import * as React from "react";
import { CircleX } from "lucide-react";
import { Tooltip } from "./tip";
import type { Icon } from "./tones";
import { cx } from "./util";

// ================================================================== button

export interface ButtonProps extends React.ButtonHTMLAttributes<HTMLButtonElement> {
  icon?: Icon;
  /** primary: the one main action of a view (ruby). danger: a destructive confirm. ghost: no border until hover. */
  tone?: "default" | "primary" | "ghost" | "danger";
  /** 24 px (`md`) or 20 px (`sm`). */
  size?: "md" | "sm";
}

/** A text button: hairline, no fill. */
export const Button = React.forwardRef<HTMLButtonElement, ButtonProps>(function Button(
  { icon: Icon, tone = "default", size = "md", className, children, type = "button", ...rest },
  ref,
) {
  return (
    <button
      ref={ref}
      type={type}
      className={cx(
        "d-btn",
        tone === "primary" && "d-btn--primary",
        tone === "ghost" && "d-btn--ghost",
        tone === "danger" && "d-btn--danger",
        size === "sm" && "d-btn--sm",
        className,
      )}
      {...rest}
    >
      {Icon && <Icon size={size === "sm" ? 12 : 14} strokeWidth={1.5} aria-hidden />}
      {children}
    </button>
  );
});

// ================================================================== icon button

export interface IconButtonProps extends Omit<React.ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  icon: Icon;
  /** Accessible name and tooltip: the button has no visible text. */
  label: string;
  kbd?: string;
  /** A toggle that is on (`aria-pressed`). */
  pressed?: boolean;
  size?: "md" | "sm";
  /** A small count in the corner. */
  badge?: number | null;
  tone?: "danger";
  /** One short extra line in the tooltip. */
  description?: string;
}

/** An icon-only button, 24 px (20 px `sm`), its name and shortcut in a tooltip. */
export const IconButton = React.forwardRef<HTMLButtonElement, IconButtonProps>(function IconButton(
  { icon: Icon, label, kbd, pressed, size = "md", badge, tone, description, className, type = "button", ...rest },
  ref,
) {
  return (
    <Tooltip label={label} kbd={kbd} description={description}>
      <button
        ref={ref}
        type={type}
        aria-label={label}
        aria-pressed={pressed}
        className={cx("d-icon", size === "sm" && "d-icon--sm", tone === "danger" && "d-icon--danger", className)}
        {...rest}
      >
        <Icon size={size === "sm" ? 12 : 14} strokeWidth={1.5} aria-hidden />
        {badge ? <span className="d-icon__badge">{badge > 99 ? "99+" : badge}</span> : null}
      </button>
    </Tooltip>
  );
});

// ================================================================== segmented

export interface SegmentedItem<T extends string = string> {
  value: T;
  label?: string;
  icon?: Icon;
  /** Tooltip, and the name of an icon-only item. */
  title?: string;
  disabled?: boolean;
}

export interface SegmentedProps<T extends string = string> {
  items: SegmentedItem<T>[];
  value: T;
  onChange: (value: T) => void;
  /** Accessible name of the group. */
  label: string;
  size?: "md" | "sm";
  disabled?: boolean;
}

/** Two to four exclusive choices, inset, no dividers. Arrow keys move and select, Home and End jump (one tab stop). */
export function Segmented<T extends string>({ items, value, onChange, label, size = "md", disabled }: SegmentedProps<T>) {
  const onKey = (e: React.KeyboardEvent<HTMLDivElement>) => {
    const dir = e.key === "ArrowRight" || e.key === "ArrowDown" ? 1 : e.key === "ArrowLeft" || e.key === "ArrowUp" ? -1 : 0;
    const jump = e.key === "Home" || e.key === "End";
    if ((!dir && !jump) || disabled) return;
    e.preventDefault();
    const on = items.filter((i) => !i.disabled);
    const next = jump
      ? on[e.key === "Home" ? 0 : on.length - 1]
      : on[(on.findIndex((i) => i.value === value) + dir + on.length) % on.length];
    if (!next) return;
    onChange(next.value);
    e.currentTarget.querySelectorAll<HTMLElement>('[role="radio"]')[items.indexOf(next)]?.focus();
  };
  return (
    <div
      className={cx("d-seg", size === "sm" && "d-seg--sm")}
      role="radiogroup"
      aria-label={label}
      aria-disabled={disabled || undefined}
      onKeyDown={onKey}
    >
      {items.map((it) => {
        const Icon = it.icon;
        const on = it.value === value;
        return (
          <button
            key={it.value}
            type="button"
            role="radio"
            aria-checked={on}
            tabIndex={on ? 0 : -1}
            disabled={disabled || it.disabled}
            aria-label={it.label ? undefined : it.title}
            title={it.title ?? it.label}
            className="d-seg__btn"
            onClick={() => onChange(it.value)}
          >
            {Icon && <Icon size={size === "sm" ? 12 : 13} strokeWidth={1.5} aria-hidden />}
            {it.label && <span>{it.label}</span>}
          </button>
        );
      })}
    </div>
  );
}

// ================================================================== switch, checkbox

export interface SwitchProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Accessible name and tooltip. */
  label: string;
  disabled?: boolean;
}

/** A square on/off switch, ruby when on. Space toggles. */
export function Switch({ checked, onChange, label, disabled }: SwitchProps) {
  return (
    <button
      type="button"
      role="switch"
      aria-checked={checked}
      aria-label={label}
      title={label}
      disabled={disabled}
      className="d-switch"
      onClick={() => onChange(!checked)}
    >
      <span className="d-switch__knob" aria-hidden />
    </button>
  );
}

export interface CheckboxProps {
  checked: boolean;
  onChange: (checked: boolean) => void;
  /** Accessible name (and tooltip) when there is no visible text. */
  label: string;
  /** Visible text after the box. */
  children?: React.ReactNode;
  disabled?: boolean;
  /** Some, not all (a select-all box). */
  mixed?: boolean;
}

/** The native box, in the ruby accent. */
export function Checkbox({ checked, onChange, label, children, disabled, mixed }: CheckboxProps) {
  const ref = React.useRef<HTMLInputElement>(null);
  React.useEffect(() => {
    if (ref.current) ref.current.indeterminate = !!mixed;
  }, [mixed]);
  return (
    <label className="d-check" title={label} data-disabled={disabled || undefined}>
      <input
        ref={ref}
        type="checkbox"
        checked={checked}
        disabled={disabled}
        aria-label={children ? undefined : label}
        onChange={(e) => onChange(e.target.checked)}
      />
      {children}
    </label>
  );
}

// ================================================================== text inputs

export interface TextInputProps extends React.InputHTMLAttributes<HTMLInputElement> {
  mono?: boolean;
  /** The failed edge, and `aria-invalid`. */
  invalid?: boolean;
  /** 20 px instead of 24. */
  small?: boolean;
}

/** A one-line input, 24 px. */
export const TextInput = React.forwardRef<HTMLInputElement, TextInputProps>(function TextInput(
  { mono, invalid, small, className, ...p },
  ref,
) {
  return (
    <input
      ref={ref}
      {...p}
      aria-invalid={invalid || p["aria-invalid"] || undefined}
      className={cx("d-input", mono && "d-input--mono", small && "d-input--sm", className)}
    />
  );
});

export interface TextAreaProps extends React.TextareaHTMLAttributes<HTMLTextAreaElement> {
  mono?: boolean;
  invalid?: boolean;
}

/** A multi-line input, resizable in height. */
export const TextArea = React.forwardRef<HTMLTextAreaElement, TextAreaProps>(function TextArea(
  { mono, invalid, className, ...p },
  ref,
) {
  return (
    <textarea
      ref={ref}
      {...p}
      aria-invalid={invalid || p["aria-invalid"] || undefined}
      className={cx("d-input d-area", mono && "d-input--mono", className)}
    />
  );
});

// ================================================================== field

export interface FieldProps {
  label: string;
  /** Help goes in the label's tooltip, never under the field. */
  help?: string;
  /** Short, 2 to 5 words: "Must be 1 to 20". */
  error?: string | null;
  /** The control. It gets `id`, `invalid` and `aria-describedby`. */
  children: React.ReactElement;
}

/** A form row: the label in a 96 px column, the control, the error at the row end. */
export function Field({ label, help, error, children }: FieldProps) {
  const id = React.useId();
  const errId = `${id}-err`;
  const props = children.props as { id?: string };
  const ctlId = props.id ?? id;
  const ctl = React.cloneElement(children as React.ReactElement<Record<string, unknown>>, {
    id: ctlId,
    invalid: error ? true : undefined,
    "aria-describedby": error ? errId : undefined,
  });
  return (
    <div className="d-field">
      <label htmlFor={ctlId} className="d-field__label" title={help ?? label} data-help={help ? "" : undefined}>
        {label}
      </label>
      <div className="d-field__ctl">
        {ctl}
        {error && (
          <span id={errId} className="d-err" role="alert">
            <CircleX size={12} strokeWidth={1.5} aria-hidden />
            {error}
          </span>
        )}
      </div>
    </div>
  );
}
