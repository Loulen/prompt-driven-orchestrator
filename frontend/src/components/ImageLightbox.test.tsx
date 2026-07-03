import { render, screen, fireEvent } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";

import ImageLightbox from "./ImageLightbox";

describe("ImageLightbox", () => {
  it("renders the image at the given index", () => {
    render(
      <ImageLightbox
        images={["/runs/r1/artifact?path=a.png"]}
        index={0}
        onClose={() => {}}
      />,
    );
    const img = screen.getByTestId("lightbox-image");
    expect(img.getAttribute("src")).toBe("/runs/r1/artifact?path=a.png");
  });

  it("closes on Escape", () => {
    const onClose = vi.fn();
    render(<ImageLightbox images={["/x.png"]} index={0} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when the backdrop is clicked", () => {
    const onClose = vi.fn();
    render(<ImageLightbox images={["/x.png"]} index={0} onClose={onClose} />);
    fireEvent.click(screen.getByTestId("image-lightbox"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes when the image itself is clicked", () => {
    const onClose = vi.fn();
    render(<ImageLightbox images={["/x.png"]} index={0} onClose={onClose} />);
    fireEvent.click(screen.getByTestId("lightbox-image"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("closes via the close button", () => {
    const onClose = vi.fn();
    render(<ImageLightbox images={["/x.png"]} index={0} onClose={onClose} />);
    fireEvent.click(screen.getByTestId("lightbox-close"));
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("does not close on unrelated keys", () => {
    const onClose = vi.fn();
    render(<ImageLightbox images={["/x.png"]} index={0} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "a" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("removes its keydown listener on unmount", () => {
    const onClose = vi.fn();
    const { unmount } = render(
      <ImageLightbox images={["/x.png"]} index={0} onClose={onClose} />,
    );
    unmount();
    fireEvent.keyDown(window, { key: "Escape" });
    expect(onClose).not.toHaveBeenCalled();
  });

  it("ArrowRight advances to the next image", () => {
    render(
      <ImageLightbox images={["/a.png", "/b.png", "/c.png"]} index={0} onClose={() => {}} />,
    );
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(screen.getByTestId("lightbox-image").getAttribute("src")).toBe("/b.png");
  });

  it("ArrowLeft retreats to the previous image", () => {
    render(
      <ImageLightbox images={["/a.png", "/b.png", "/c.png"]} index={1} onClose={() => {}} />,
    );
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    expect(screen.getByTestId("lightbox-image").getAttribute("src")).toBe("/a.png");
  });

  it("clamps at the end (ArrowRight on the last image is a no-op, no wrap)", () => {
    const onClose = vi.fn();
    render(
      <ImageLightbox images={["/a.png", "/b.png", "/c.png"]} index={2} onClose={onClose} />,
    );
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(screen.getByTestId("lightbox-image").getAttribute("src")).toBe("/c.png");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("clamps at the start (ArrowLeft on the first image is a no-op, no wrap)", () => {
    const onClose = vi.fn();
    render(
      <ImageLightbox images={["/a.png", "/b.png", "/c.png"]} index={0} onClose={onClose} />,
    );
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    expect(screen.getByTestId("lightbox-image").getAttribute("src")).toBe("/a.png");
    expect(onClose).not.toHaveBeenCalled();
  });

  it("is inert for a single image: arrows do nothing, chevrons/counter absent", () => {
    const onClose = vi.fn();
    render(<ImageLightbox images={["/a.png"]} index={0} onClose={onClose} />);
    fireEvent.keyDown(window, { key: "ArrowRight" });
    fireEvent.keyDown(window, { key: "ArrowLeft" });
    expect(screen.getByTestId("lightbox-image").getAttribute("src")).toBe("/a.png");
    expect(onClose).not.toHaveBeenCalled();
    expect(screen.queryByTestId("lightbox-prev")).toBeNull();
    expect(screen.queryByTestId("lightbox-next")).toBeNull();
    expect(screen.queryByTestId("lightbox-counter")).toBeNull();
  });

  it("shows a counter and chevrons whose disabled state tracks the ends", () => {
    render(
      <ImageLightbox images={["/a.png", "/b.png", "/c.png"]} index={0} onClose={() => {}} />,
    );
    expect(screen.getByTestId("lightbox-counter").textContent).toBe("1 of 3");
    // At index 0: prev disabled, next enabled.
    expect((screen.getByTestId("lightbox-prev") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("lightbox-next") as HTMLButtonElement).disabled).toBe(false);

    // Clicking next advances the counter and the image.
    fireEvent.click(screen.getByTestId("lightbox-next"));
    expect(screen.getByTestId("lightbox-counter").textContent).toBe("2 of 3");
    expect(screen.getByTestId("lightbox-image").getAttribute("src")).toBe("/b.png");

    // Advance to the last image: next becomes disabled.
    fireEvent.click(screen.getByTestId("lightbox-next"));
    expect(screen.getByTestId("lightbox-counter").textContent).toBe("3 of 3");
    expect((screen.getByTestId("lightbox-next") as HTMLButtonElement).disabled).toBe(true);
    expect((screen.getByTestId("lightbox-prev") as HTMLButtonElement).disabled).toBe(false);
  });

  it("clicking a chevron does not close the viewer", () => {
    const onClose = vi.fn();
    render(
      <ImageLightbox images={["/a.png", "/b.png"]} index={0} onClose={onClose} />,
    );
    fireEvent.click(screen.getByTestId("lightbox-next"));
    expect(onClose).not.toHaveBeenCalled();
  });

  it("does not react to arrows after unmount", () => {
    const onClose = vi.fn();
    const { unmount } = render(
      <ImageLightbox images={["/a.png", "/b.png"]} index={0} onClose={onClose} />,
    );
    unmount();
    // No crash / no state effect: the listener was removed on unmount.
    fireEvent.keyDown(window, { key: "ArrowRight" });
    expect(onClose).not.toHaveBeenCalled();
  });
});
