// The montage of one variant: keep the windows around the markers, cut the
// waits outright, play the fast-forwarded stretches at ×8, crop on the panel
// that matters, frame it in the baked-in window chrome, and encode the GIF and
// its poster (the last frame — the end state).
//
// `planCuts` is pure (unit-tested); `renderVariant` shells out to ffmpeg,
// asynchronously: the event loop stays free, so a Ctrl+C mid-encode is handled
// at once — the running ffmpeg is killed on exit with the demo instance.

import { execFileSync, spawn } from "node:child_process";
import fs from "node:fs";
import path from "node:path";

export const GIF_FPS = 15;
export const TARGET_MS = { min: 8_000, max: 15_000 };
/** Keep-windows closer than this are merged instead of cut (a cut that short
 *  reads as a glitch, not an ellipsis). */
const MIN_GAP_MS = 350;

/**
 * From the recorded timeline to the list of segments the GIF plays, in order.
 * Normal-speed windows (markers, keeps, holds) win over fast stretches where
 * they overlap. Returns `{ segments: [{start, end, speed}], markers:
 * [{name, t}], durationMs, padMs }` — `t` and `durationMs` in GIF time, `padMs`
 * the freeze on the last frame that brings a short montage up to `min`.
 */
export function planCuts({ keeps, fasts = [], markers = [], duration, min = TARGET_MS.min }) {
  const clamp = ({ start, end, ...rest }) => ({ ...rest, start: Math.max(0, start), end: Math.min(duration, end) });
  const normal = mergeIntervals(keeps.map(clamp).filter((k) => k.end > k.start));

  const pieces = normal.map((n) => ({ start: n.start, end: n.end, speed: 1 }));
  for (const fast of fasts.map(clamp)) {
    for (const piece of subtract(fast, normal)) pieces.push({ ...piece, speed: fast.speed });
  }
  pieces.sort((a, b) => a.start - b.start);

  // Close the tiny gaps between two normal-speed pieces.
  const segments = [];
  for (const piece of pieces) {
    const last = segments.at(-1);
    if (last && last.speed === 1 && piece.speed === 1 && piece.start - last.end < MIN_GAP_MS) {
      last.end = Math.max(last.end, piece.end);
    } else if (piece.end - piece.start > 1) {
      segments.push({ ...piece });
    }
  }

  let out = 0;
  const mapped = segments.map((s) => {
    const m = { ...s, outStart: out };
    out += (s.end - s.start) / s.speed;
    return m;
  });
  const toGifTime = (t) => {
    const seg = mapped.find((s) => t >= s.start && t <= s.end) ?? mapped.find((s) => s.start >= t);
    if (!seg) return out;
    return seg.outStart + (Math.max(t, seg.start) - seg.start) / seg.speed;
  };
  const padMs = Math.max(0, min - out);
  return {
    segments,
    markers: markers.map((m) => ({ name: m.name, t: Math.round(toGifTime(m.t)) })),
    durationMs: Math.round(out + padMs),
    padMs: Math.round(padMs),
  };
}

function mergeIntervals(list) {
  const sorted = [...list].sort((a, b) => a.start - b.start);
  const out = [];
  for (const item of sorted) {
    const last = out.at(-1);
    if (last && item.start <= last.end) last.end = Math.max(last.end, item.end);
    else out.push({ start: item.start, end: item.end });
  }
  return out;
}

/** `interval` minus every interval of `holes` (sorted, disjoint). */
function subtract(interval, holes) {
  let rest = [{ start: interval.start, end: interval.end }];
  for (const hole of holes) {
    rest = rest.flatMap((r) => {
      if (hole.end <= r.start || hole.start >= r.end) return [r];
      const parts = [];
      if (hole.start > r.start) parts.push({ start: r.start, end: hole.start });
      if (hole.end < r.end) parts.push({ start: hole.end, end: r.end });
      return parts;
    });
  }
  return rest.filter((r) => r.end - r.start > 1);
}

/**
 * Where the content sits in the framed GIF: a window with a title bar (traffic
 * lights), a 1 px border and rounded corners, on the GitHub dark canvas with a
 * drop shadow. `crop` is the page region filmed, `gifWidth` the GIF's width.
 */
export function chromeLayout({ gifWidth, crop }) {
  const even = (n) => Math.round(n / 2) * 2;
  const margin = even(gifWidth * 0.025);
  const bar = even(gifWidth * 0.032);
  const border = 1;
  const windowWidth = gifWidth - 2 * margin;
  const contentWidth = windowWidth - 2 * border;
  const contentHeight = even((contentWidth * crop.height) / crop.width);
  const windowHeight = bar + contentHeight + border;
  return {
    width: gifWidth,
    height: even(margin + windowHeight + margin * 1.4),
    margin,
    bar,
    border,
    radius: Math.round(gifWidth * 0.011),
    windowWidth,
    windowHeight,
    content: { x: margin + border, y: margin + bar, width: contentWidth, height: contentHeight },
  };
}

/** The chrome as HTML: everything opaque except the content hole, which stays
 *  transparent so the film shows through (screenshot with omitBackground). */
export function chromeHtml(layout) {
  const { margin, bar, radius, windowWidth, windowHeight, width, height } = layout;
  const dot = (color) => `<span style="width:${bar * 0.36}px;height:${bar * 0.36}px;border-radius:50%;background:${color};display:inline-block"></span>`;
  return `<!doctype html><html><body style="margin:0;width:${width}px;height:${height}px;background:transparent;overflow:hidden">
<div style="position:absolute;left:${margin}px;top:${margin}px;width:${windowWidth}px;height:${windowHeight}px;border-radius:${radius}px;
  box-shadow:0 ${margin * 0.45}px ${margin * 1.1}px rgba(0,0,0,.6), 0 0 0 ${width * 2}px #0d1117;overflow:hidden">
  <div style="height:${bar}px;background:#1a1e25;border-bottom:1px solid #2a313b;display:flex;align-items:center;gap:${bar * 0.24}px;padding-left:${bar * 0.42}px;box-sizing:border-box">
    ${dot("#ff5f57")}${dot("#febc2e")}${dot("#28c840")}
  </div>
</div>
<div style="position:absolute;left:${margin}px;top:${margin}px;width:${windowWidth}px;height:${windowHeight}px;border-radius:${radius}px;border:1px solid #2a313b;box-sizing:border-box"></div>
</body></html>`;
}

export async function renderChrome(browser, layout, file) {
  const page = await browser.newPage({ viewport: { width: layout.width, height: layout.height } });
  try {
    await page.setContent(chromeHtml(layout));
    await page.screenshot({ path: file, omitBackground: true });
  } finally {
    await page.close();
  }
}

/** Cut, crop, frame and encode one variant. Returns the probed GIF facts. */
export async function renderVariant({ video, plan, crop, layout, chromePng, workDir, gifFile, posterFile, fps = GIF_FPS }) {
  fs.mkdirSync(workDir, { recursive: true });
  const n = plan.segments.length;
  if (n === 0) throw new Error("nothing to keep: the scene set no marker, keep or hold");
  const s = (ms) => (ms / 1000).toFixed(3);
  const parts = [`[0:v]split=${n}${plan.segments.map((_, i) => `[s${i}]`).join("")}`];
  plan.segments.forEach((seg, i) => {
    parts.push(`[s${i}]trim=start=${s(seg.start)}:end=${s(seg.end)},setpts=(PTS-STARTPTS)/${seg.speed}[v${i}]`);
  });
  parts.push(`${plan.segments.map((_, i) => `[v${i}]`).join("")}concat=n=${n}:v=1:a=0[cat]`);
  const c = layout.content;
  parts.push(
    `[cat]crop=${crop.width}:${crop.height}:${crop.x}:${crop.y},scale=${c.width}:${c.height}:flags=lanczos,fps=${fps},` +
      `tpad=stop_mode=clone:stop_duration=${s(plan.padMs + 250)}[content]`,
  );
  parts.push(`color=c=0x0d1117:s=${layout.width}x${layout.height}:r=${fps}[bg]`);
  parts.push(`[bg][content]overlay=x=${c.x}:y=${c.y}:shortest=1[base]`);
  parts.push(`[base][1:v]overlay=0:0:shortest=1,format=rgb24[out]`);

  const montage = path.join(workDir, "montage.mkv");
  await ffmpeg(["-i", video, "-loop", "1", "-framerate", String(fps), "-i", chromePng, "-filter_complex", parts.join(";"), "-map", "[out]", "-c:v", "ffv1", montage]);
  await ffmpeg([
    "-i",
    montage,
    "-vf",
    "split[a][b];[a]palettegen=stats_mode=diff:max_colors=256[p];[b][p]paletteuse=dither=sierra2_4a:diff_mode=rectangle",
    "-loop",
    "0",
    gifFile,
  ]);
  await ffmpeg(["-sseof", "-0.4", "-i", montage, "-update", "1", "-q:v", "2", posterFile]);
  return probe(gifFile);
}

const running = new Set();
process.on("exit", () => {
  for (const child of running) child.kill("SIGKILL");
});

export function ffmpeg(args) {
  return new Promise((resolve, reject) => {
    const child = spawn("ffmpeg", ["-y", "-hide_banner", "-loglevel", "error", ...args], { stdio: ["ignore", "inherit", "inherit"] });
    running.add(child);
    child.on("error", (error) => {
      running.delete(child);
      reject(error);
    });
    child.on("exit", (code, signal) => {
      running.delete(child);
      if (code === 0) resolve();
      else reject(new Error(`ffmpeg exited with ${signal ?? `code ${code}`}`));
    });
  });
}

/** Duration (s), dimensions and weight of a produced GIF. */
export function probe(file) {
  const out = JSON.parse(
    execFileSync("ffprobe", ["-v", "error", "-show_entries", "format=duration:stream=width,height", "-of", "json", file], { encoding: "utf8" }),
  );
  return {
    durationS: Number(Number(out.format.duration).toFixed(2)),
    width: out.streams[0].width,
    height: out.streams[0].height,
    bytes: fs.statSync(file).size,
  };
}
