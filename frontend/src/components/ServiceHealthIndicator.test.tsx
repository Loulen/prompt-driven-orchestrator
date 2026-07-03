import { render, screen } from "@testing-library/react";
import { describe, it, expect } from "vitest";
import ServiceHealthIndicator from "./ServiceHealthIndicator";

describe("ServiceHealthIndicator (#156)", () => {
  it("shows the amber ephemeral pill only when persistent is false", () => {
    render(
      <ServiceHealthIndicator service={{ supervisor: "none", persistent: false }} />,
    );
    const pill = screen.getByTestId("service-ephemeral-pill");
    expect(pill).toHaveTextContent("ephemeral");
    // Same amber token as reconnecting dot / near-cap counter — never red.
    expect(pill.className).toContain("text-st-await");
    expect(pill.getAttribute("title")).toContain("pdo service install");
  });

  it("renders nothing when the daemon is persistent (silence-when-healthy)", () => {
    const { container } = render(
      <ServiceHealthIndicator service={{ supervisor: "systemd", persistent: true }} />,
    );
    expect(screen.queryByTestId("service-ephemeral-pill")).toBeNull();
    expect(container).toBeEmptyDOMElement();
  });

  it("renders nothing when persistence is unknown (null)", () => {
    render(
      <ServiceHealthIndicator service={{ supervisor: "none", persistent: null }} />,
    );
    expect(screen.queryByTestId("service-ephemeral-pill")).toBeNull();
  });

  it("renders nothing when the service field is absent (daemon not yet responded)", () => {
    render(<ServiceHealthIndicator service={undefined} />);
    expect(screen.queryByTestId("service-ephemeral-pill")).toBeNull();
  });
});
