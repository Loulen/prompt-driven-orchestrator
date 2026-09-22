// The synthetic cursor every README GIF shows (design: « White arrow with a
// dark outline, and a ring in PDO accent green #10b981 on each click. About
// 22 px in a 960 px GIF, pressed state (scale 0.88) during a drag, and a dashed
// drag line in edge-selected color #fdba74 »).
//
// Headless Chromium draws no pointer, so the page draws one: an init script
// (runs before the app on every document) adds an overlay that follows the
// real mouse events Playwright dispatches. `size` is in CSS px of the page and
// is chosen per variant so the arrow is ~22 px once the crop is scaled to the
// GIF (see Recorder.cursorSize).

export const CURSOR_STYLE = {
  ring: "#10b981",
  dragLine: "#fdba74",
  pressedScale: 0.88,
  targetGifPx: 22,
};

/** The init script, as a function Playwright serialises (`addInitScript`). */
export function cursorInitScript({ size, ring, dragLine, pressedScale }) {
  const install = () => {
    if (document.getElementById("__demo-cursor")) return;
    const ns = "http://www.w3.org/2000/svg";
    const layer = document.createElement("div");
    layer.id = "__demo-cursor-layer";
    layer.style.cssText = "position:fixed;inset:0;pointer-events:none;z-index:2147483647;";

    const line = document.createElementNS(ns, "svg");
    line.setAttribute("width", "100%");
    line.setAttribute("height", "100%");
    line.style.cssText = "position:absolute;inset:0;overflow:visible;";
    const path = document.createElementNS(ns, "line");
    path.setAttribute("stroke", dragLine);
    path.setAttribute("stroke-width", String(Math.max(2, size / 9)));
    path.setAttribute("stroke-dasharray", `${size / 3} ${size / 4.5}`);
    path.setAttribute("stroke-linecap", "round");
    path.style.display = "none";
    line.append(path);

    const cursor = document.createElement("div");
    cursor.id = "__demo-cursor";
    cursor.style.cssText = `position:absolute;left:0;top:0;width:${size}px;height:${size}px;transform-origin:0 0;` + "transition:transform 90ms ease-out;display:none;will-change:transform;";
    cursor.innerHTML =
      `<svg width="${size}" height="${size}" viewBox="0 0 24 24" style="overflow:visible;filter:drop-shadow(0 1px 2px rgba(0,0,0,.55))">` +
      '<path d="M3 2 L3 19.5 L7.6 15.4 L10.6 22 L13.8 20.6 L10.9 14.1 L17 14.1 Z" fill="#ffffff" stroke="#11151a" stroke-width="1.6" stroke-linejoin="round"/>' +
      "</svg>";
    layer.append(line, cursor);
    document.documentElement.append(layer);

    let x = -100;
    let y = -100;
    let pressed = false;
    let dragFrom = null;
    const place = () => {
      cursor.style.transform = `translate(${x}px, ${y}px) scale(${pressed ? pressedScale : 1})`;
    };
    const ringAt = (cx, cy) => {
      const halo = document.createElement("div");
      const d = size * 1.9;
      halo.style.cssText =
        `position:absolute;left:${cx - d / 2}px;top:${cy - d / 2}px;width:${d}px;height:${d}px;border-radius:50%;` +
        `border:${Math.max(2, size / 8)}px solid ${ring};box-shadow:0 0 ${size / 2}px ${ring}66;opacity:.95;` +
        "transform:scale(.35);transition:transform 420ms ease-out, opacity 480ms ease-in;";
      layer.insertBefore(halo, cursor);
      requestAnimationFrame(() => {
        halo.style.transform = "scale(1)";
        halo.style.opacity = "0";
      });
      setTimeout(() => halo.remove(), 600);
    };
    const onMove = (event) => {
      x = event.clientX;
      y = event.clientY;
      cursor.style.display = "block";
      if (dragFrom) {
        const far = Math.hypot(x - dragFrom.x, y - dragFrom.y) > 6;
        path.style.display = far ? "block" : "none";
        path.setAttribute("x1", String(dragFrom.x));
        path.setAttribute("y1", String(dragFrom.y));
        path.setAttribute("x2", String(x));
        path.setAttribute("y2", String(y));
      }
      place();
    };
    window.addEventListener("pointermove", onMove, true);
    window.addEventListener("mousemove", onMove, true);
    window.addEventListener(
      "pointerdown",
      (event) => {
        onMove(event);
        pressed = true;
        dragFrom = { x: event.clientX, y: event.clientY };
        ringAt(event.clientX, event.clientY);
        place();
      },
      true,
    );
    window.addEventListener(
      "pointerup",
      () => {
        pressed = false;
        dragFrom = null;
        path.style.display = "none";
        place();
      },
      true,
    );
    // A scene can place the cursor before its first move (after a navigation).
    window.__demoCursorAt = (px, py) => onMove({ clientX: px, clientY: py });
  };
  if (document.documentElement) install();
  else document.addEventListener("DOMContentLoaded", install, { once: true });
  // A framework replacing <html>'s children must not take the layer away.
  setInterval(() => {
    if (document.documentElement && !document.getElementById("__demo-cursor")) install();
  }, 250);
}
