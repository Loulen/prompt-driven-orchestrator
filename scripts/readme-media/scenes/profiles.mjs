// Scene « Agent profiles » (README row 8, #857). No live agent: both nodes of
// the demo pipeline `implement-review` follow ONE agent profile, « daily
// driver » (claude · opus · medium). One edit of that profile in Settings ›
// Agents › Agent profiles, and `implementer` and `reviewer` both switch,
// without editing either node.
//
//   a — the profile's model: opus → fable (the published one).
//   b — the profile's harness: claude → copilot, on gpt-5.6-sol.
//
// The demo library's copy of the pipeline pins each node's harness and model
// (its target, fixture/targets/implement-review.yaml); this scene rewrites that
// copy, in the demo HOME only, so both nodes follow the profile instead.

import fs from "node:fs";
import path from "node:path";
import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_PIPELINE } from "../lib/demo-pipeline.mjs";
import { assertNoAgent, forbidAgentLaunch, waitForRuns } from "./_no-agent.mjs";

const VIEWPORT = { width: 960, height: 570 };
// The inspector wide enough for the Agent control to read in full.
const LAYOUT = { "pdo.layout.run": { left: 18, center: 40, right: 42 } };
// Zoomed past the Settings rail and the pipelines list (both ~175 px): the
// profile list, the canvas and the inspector at ×1.22.
const CROP = { x: 176, y: 0, width: 784, height: 570 };
const PROFILE = { name: "daily driver", harness: "claude", model: "opus", effort: "medium" };
// The other rows of a lived-in profile list (the models of the Stats scene).
const OTHER_PROFILES = [
  { name: "fable high", harness: "claude", model: "fable", effort: "high" },
  { name: "glm flash", harness: "pi", model: "z-ai/glm-5.3-flash" },
];
const NODES = ["implementer", "reviewer"];

/** Both work nodes follow `profileId` instead of pinning a harness. */
function followProfile(yaml, profileId) {
  const pinned = /^( +)pin_harness: claude\n\1harnesses:\n\1 {2}claude: \{[^\n]*\}\n/gm;
  const out = yaml.replace(pinned, `$1agent_choice:\n$1  mode: profile\n$1  profile_id: ${profileId}\n`);
  const count = (out.match(/profile_id: /g) ?? []).length;
  if (count !== NODES.length) throw new Error(`expected ${NODES.length} pinned nodes in the demo pipeline, rewrote ${count}`);
  return out;
}

async function waitForFollowers(instance, profileId) {
  const deadline = Date.now() + 15_000;
  while (Date.now() < deadline) {
    const { pipeline } = await instance.api("GET", `/pipelines/${DEMO_PIPELINE.id}`);
    const following = pipeline.nodes.filter((n) => n.agent_choice?.mode === "profile" && n.agent_choice.profile_id === profileId);
    if (following.length === NODES.length) return;
    await sleep(250);
  }
  throw new Error(`the demo pipeline's nodes do not follow profile ${profileId} (pipeline watcher?)`);
}

const node = (page, name) => page.locator(".react-flow__node").filter({ hasText: name }).first();

/** The profile as the scene starts it (the previous variant edited it). */
let profileId = null;

/** The pipeline on the canvas, `implementer` selected, its Agent control in view. Off camera. */
async function openCanvas(ctx) {
  const { page } = ctx;
  await ctx.instance.api("PUT", `/settings/agent-profiles/${profileId}`, PROFILE);
  await forbidAgentLaunch(page);
  await ctx.goto("/");
  await waitForRuns(page);
  await page.getByTestId("left-tab-library").click();
  await page.getByText(DEMO_PIPELINE.id, { exact: true }).first().click();
  await node(page, "reviewer").waitFor();
  await node(page, "implementer").click();
  await showAgent(page, PROFILE.model);
  await sleep(500);
}

/** Bring the selected node's Agent control into view, once it reads `model`. */
async function showAgent(page, model) {
  const control = page.getByTestId("node-agent-control");
  await control.getByText(new RegExp(`· ${model.replace(/[.*+?^${}()|[\]\\/]/g, "\\$&")} ·`)).waitFor({ timeout: 10_000 });
  // Mid-panel, not flush with its bottom edge.
  await control.evaluate((el) => el.scrollIntoView({ block: "center" }));
  return control;
}

/** Wait until `locator` stops moving (the settings page loads, then scrolls
 *  smoothly to the section): a click aimed at a moving row lands elsewhere. */
async function settled(locator) {
  await locator.waitFor({ timeout: 15_000 });
  await sleep(300);
  let last = null;
  let still = 0;
  for (let i = 0; i < 60 && still < 4; i++) {
    const box = await locator.boundingBox();
    still = last && box && Math.abs(box.y - last.y) < 0.5 ? still + 1 : 0;
    last = box;
    await sleep(100);
  }
}

/** Settings › Agents › Agent profiles, then the profile's editor modal. The page
 *  loads and the rail click (outside the crop) are cut. */
async function openProfile(ctx) {
  const { page } = ctx;
  await ctx.keep(() => ctx.click(page.getByTestId("open-settings"), { duration: 550 }));
  await page.getByTestId("settings-category-agents").waitFor();
  await ctx.click(page.getByTestId("settings-category-agents"), { duration: 300 });
  const panel = page.getByTestId("agent-profiles-panel");
  const row = panel.getByText(PROFILE.name, { exact: true });
  // The section list scrolls only once the page is loaded.
  await row.waitFor({ timeout: 15_000 });
  await sleep(300);
  const height = ctx.variant.viewport.height;
  // The section click is kept; the smooth scroll that follows, and any retry,
  // are cut (they last as long as the machine is busy).
  for (let attempt = 0; attempt < 3; attempt++) {
    const click = () => ctx.click(page.getByTestId("settings-section-agent-profiles"), { duration: attempt === 0 ? 400 : 150 });
    await (attempt === 0 ? ctx.keep(click) : click());
    await settled(row);
    const box = await row.boundingBox();
    if (box && box.y > 60 && box.y + box.height < height - 70) break;
  }
  // The editor is a modal (#899), opened from the row's Edit pencil.
  await ctx.keep(() => ctx.click(panel.getByRole("button", { name: `Edit ${PROFILE.name}` }), { duration: 500 }));
  const modal = page.getByTestId("agent-profile-modal");
  await modal.getByTestId("agent-profile-model-trigger").waitFor({ timeout: 10_000 });
  await sleep(200);
  return panel;
}

/** Save the profile (kept), wait for its row to read the new combination
 *  (cut), and mark it. */
async function saveProfile(ctx, panel, combination) {
  await ctx.keep(() => ctx.click(ctx.page.getByTestId("agent-profile-save"), { duration: 500 }));
  await ctx.page.getByTestId("agent-profile-modal").waitFor({ state: "detached", timeout: 15_000 });
  await panel.getByText(combination, { exact: true }).waitFor({ timeout: 15_000 });
  await sleep(200);
  ctx.mark("profile-changed", { before: 100, after: 900 });
}

/** Back on the canvas: `implementer`, then `reviewer`, both read `model`.
 *  The clicks are kept, the reloads they trigger are cut. */
async function showSwitchedNodes(ctx, model) {
  const { page } = ctx;
  await ctx.keep(() => ctx.click(page.getByRole("button", { name: "Close settings" }), { duration: 500 }));
  await page.getByTestId("settings-surface").waitFor({ state: "detached" });
  let control = await showAgent(page, model);
  await ctx.keep(async () => {
    await ctx.hover(control, { duration: 550, pause: 400 });
    await ctx.click(node(page, "reviewer"), { duration: 550 });
  });
  control = await showAgent(page, model);
  await sleep(200);
  await ctx.keep(() => ctx.hover(control, { duration: 450, pause: 150 }));
  ctx.mark("nodes-switched", { before: 300, after: 500 });
  // Both nodes were read back from the page on the way (showAgent waits for
  // `model` on each): the switch is real, not staged. Nothing moves after the
  // hold — the video runs a beat past it, into the poster.
  await ctx.hold(1500);
  assertNoAgent(ctx.instance);
}

export default {
  name: "profiles",
  title: "Agent profiles",
  needs: ["history"],
  live: [],
  async setup(instance) {
    const profile = await instance.api("POST", "/settings/agent-profiles", PROFILE);
    profileId = profile.id;
    for (const other of OTHER_PROFILES) await instance.api("POST", "/settings/agent-profiles", other);
    const file = path.join(instance.home, ".pdo", "pipelines", `${DEMO_PIPELINE.id}.yaml`);
    fs.writeFileSync(file, followProfile(fs.readFileSync(file, "utf8"), profile.id));
    await waitForFollowers(instance, profile.id);
  },
  variants: [
    {
      id: "a",
      label: "The profile's model: opus → fable",
      viewport: VIEWPORT,
      crop: CROP,
      localStorage: LAYOUT,
      markers: ["profile-changed", "nodes-switched"],
      async play(ctx) {
        const { page } = ctx;
        await openCanvas(ctx);
        await ctx.hover(page.getByTestId("node-agent-control"), { duration: 600, pause: 200 });
        await ctx.hold(600);
        const panel = await openProfile(ctx);
        await ctx.keep(async () => {
          await ctx.click(page.getByTestId("agent-profile-model-trigger"), { duration: 450 });
          await ctx.click(page.getByTestId("agent-profile-model-option-fable"), { duration: 450 });
          await sleep(250);
        });
        await saveProfile(ctx, panel, "claude · fable · medium");
        await showSwitchedNodes(ctx, "fable");
      },
    },
    {
      id: "b",
      label: "The profile's harness: claude → copilot",
      viewport: VIEWPORT,
      crop: CROP,
      localStorage: LAYOUT,
      markers: ["profile-changed", "nodes-switched"],
      async play(ctx) {
        const { page } = ctx;
        await openCanvas(ctx);
        await ctx.hover(page.getByTestId("node-agent-control"), { duration: 500, pause: 150 });
        await ctx.hold(300);
        const panel = await openProfile(ctx);
        await ctx.keep(async () => {
          await ctx.click(page.getByTestId("agent-profile-harness"), { duration: 330 });
          await ctx.click(page.getByTestId("agent-profile-harness-menu").getByText("copilot", { exact: true }), { duration: 330 });
          await ctx.click(page.getByTestId("agent-profile-model-trigger"), { duration: 350 });
          // copilot's list scrolls inside the modal: bring the option in first.
          const option = page.getByTestId("agent-profile-model-option-gpt-5.6-sol");
          await option.scrollIntoViewIfNeeded();
          await ctx.click(option, { duration: 350 });
          await sleep(200);
        });
        await saveProfile(ctx, panel, "copilot · gpt-5.6-sol · —");
        await showSwitchedNodes(ctx, "gpt-5.6-sol");
      },
    },
  ],
};
