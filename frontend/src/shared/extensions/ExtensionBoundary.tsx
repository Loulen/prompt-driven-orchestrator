import { Component, type ReactNode } from "react";

interface Props {
  extensionId: string;
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
    if (this.state.failed) {
      return (
        <span
          className="text-fg-4"
          style={{ fontSize: "10.5px" }}
          data-testid={`extension-failed-${this.props.extensionId}`}
        >
          This element of the extension {this.props.extensionId} failed.
        </span>
      );
    }
    return this.props.children;
  }
}
