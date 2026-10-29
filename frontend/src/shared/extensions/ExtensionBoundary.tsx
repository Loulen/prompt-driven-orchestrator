import { Component, type ReactNode } from "react";

interface Props {
  extensionId: string;
  /** A small mark instead of a sentence: where a sentence would break a row (a decoration). */
  compact?: boolean;
  /** What shows instead of the failed element, when given (the text a render was drawing). */
  fallback?: ReactNode;
  children: ReactNode;
}

/**
 * Around every element an extension renders: a component that throws takes
 * down its own element, never the page it sits in.
 */
export default class ExtensionBoundary extends Component<Props, { failed: boolean }> {
  state = { failed: false };

  static getDerivedStateFromError() {
    return { failed: true };
  }

  componentDidCatch(error: unknown) {
    console.warn(`extension ${this.props.extensionId}: a component failed`, error);
  }

  render() {
    if (this.state.failed && this.props.fallback !== undefined) return this.props.fallback;
    if (this.state.failed && this.props.compact) {
      return (
        <span
          className="inline-grid h-3.5 w-3.5 shrink-0 place-items-center rounded-full border border-line-strong text-2xs leading-none text-text-3"
          title={`This element of the extension ${this.props.extensionId} failed.`}
          data-testid={`extension-failed-${this.props.extensionId}`}
        >
          !
        </span>
      );
    }
    if (this.state.failed) {
      return (
        <span
          className="text-xs text-text-3"
          data-testid={`extension-failed-${this.props.extensionId}`}
        >
          This element of the extension {this.props.extensionId} failed.
        </span>
      );
    }
    return this.props.children;
  }
}
