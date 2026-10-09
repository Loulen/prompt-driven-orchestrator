/**
 * Where the caret at `index` of a textarea is drawn, relative to the
 * textarea's border box: a hidden mirror with the same box and font holds the
 * text up to `index`, and the position of what follows is read from it. Zero
 * where nothing is laid out (a test's DOM).
 */

const MIRRORED = [
  "boxSizing",
  "width",
  "borderTopWidth",
  "borderRightWidth",
  "borderBottomWidth",
  "borderLeftWidth",
  "paddingTop",
  "paddingRight",
  "paddingBottom",
  "paddingLeft",
  "fontStyle",
  "fontVariant",
  "fontWeight",
  "fontSize",
  "fontFamily",
  "lineHeight",
  "letterSpacing",
  "wordSpacing",
  "textTransform",
  "textIndent",
  "tabSize",
] as const;

export function caretOffset(el: HTMLTextAreaElement, index: number): { top: number; left: number; height: number } {
  const style = window.getComputedStyle(el);
  const mirror = document.createElement("div");
  for (const name of MIRRORED) mirror.style[name] = style[name];
  mirror.style.position = "absolute";
  mirror.style.visibility = "hidden";
  mirror.style.top = "0";
  mirror.style.left = "-9999px";
  mirror.style.whiteSpace = "pre-wrap";
  mirror.style.overflowWrap = "break-word";
  mirror.textContent = el.value.slice(0, index);
  const marker = document.createElement("span");
  marker.textContent = el.value.slice(index) || ".";
  mirror.appendChild(marker);
  document.body.appendChild(mirror);
  const top = marker.offsetTop - el.scrollTop;
  const left = marker.offsetLeft - el.scrollLeft;
  document.body.removeChild(mirror);
  const height = parseFloat(style.lineHeight) || (parseFloat(style.fontSize) || 12) * 1.5;
  return { top, left, height };
}
