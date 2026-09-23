// Scene « Skill bank » (README row 9, #857). No live agent and no network:
// the skills are imported from a LOCAL folder, the fixture repo `qa-skills`
// (fixture/qa-skills/, copied as a git repo to ~/code/qa-skills in the demo
// HOME). They land in a folder named after the source. Then a skill goes to
// the `reviewer` of the demo pipeline `implement-review`, from the node's
// skill picker, where it reads as an active skill.
//
//   a — write one by hand (Paste SKILL.md, at the bank's root), import, then
//       add the handwritten one to `reviewer` (the published one).
//   b — import, then add an imported one (annotate-screenshots: the reviewer
//       annotates its screenshots with Pillow) to `reviewer`.

import { execFileSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { sleep } from "../lib/demo-instance.mjs";
import { DEMO_PIPELINE } from "../lib/demo-pipeline.mjs";
import { assertNoAgent, forbidAgentLaunch, waitForRuns } from "./_no-agent.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const SOURCE = "~/code/qa-skills";
const IMPORTED = ["a11y-audit", "annotate-screenshots", "playwright-capture", "visual-diff"];
const HANDMADE = "shop-review-checklist";
const HANDMADE_MD = `---
name: ${HANDMADE}
description: What to check on every shop-app change before a pass verdict.
---

# Shop review checklist

- Prices stay in cents end to end, formatted by formatPrice.
- The cart badge matches the cart, including after a reload.
- No change to style.css unless the task asks for one.
`;

// The import modal is ~920 px wide at this viewport: no crop, the page reads 1:1.
const VIEWPORT = { width: 960, height: 600 };
const LAYOUT = { "pdo.layout.run": { left: 18, center: 40, right: 42 } };

const node = (page, name) => page.locator(".react-flow__node").filter({ hasText: name }).first();

/** The bank as the fresh instance seeds it; each variant starts from it (the
 *  previous one imported and created skills in the same instance). */
let baseline = null;

async function resetBank(instance) {
  const bank = await instance.api("GET", "/settings/skills");
  for (const skill of bank.skills.filter((s) => !baseline.skills.has(s.id))) {
    await instance.api("DELETE", `/settings/skills/${encodeURIComponent(skill.id)}`);
  }
  for (const folder of bank.folders.filter((f) => !baseline.folders.has(f.id))) {
    await instance.api("DELETE", `/settings/skill-folders/${encodeURIComponent(folder.id)}`);
  }
}

/** Canvas open on the demo pipeline with `reviewer` selected, then Settings ›
 *  Agents › Skills › Open skill bank. All off camera: the GIF opens on the bank. */
async function openBank(ctx) {
  const { page } = ctx;
  await resetBank(ctx.instance);
  await forbidAgentLaunch(page);
  await ctx.goto("/");
  await waitForRuns(page);
  await page.getByTestId("left-tab-library").click();
  await page.getByText(DEMO_PIPELINE.id, { exact: true }).first().click();
  await node(page, "reviewer").click();
  await page.getByTestId("node-skill-selector").waitFor();
  await page.getByTestId("open-settings").click();
  await page.getByTestId("settings-category-agents").click();
  await page.getByTestId("settings-section-skills").click();
  await page.getByTestId("setting-open-skill-bank").click();
  await page.getByTestId("skill-bank-panel").waitFor();
  await sleep(700);
}

/** Add › Import from a source…, the local fixture repo, scan, import all. */
async function importFromFolder(ctx) {
  const { page } = ctx;
  await ctx.keep(async () => {
    await ctx.click(page.getByTestId("skill-add"), { duration: 500 });
    await ctx.click(page.getByTestId("skill-add-menu").getByText("Import from a source"), { duration: 380 });
    const input = page.getByTestId("import-source-input");
    await input.waitFor();
    await input.focus();
    await page.keyboard.type(SOURCE, { delay: 20 });
    await sleep(150);
    await page.keyboard.press("Enter");
  });
  // The scan is cut: the GIF goes from Enter to the list of found skills.
  await page.getByTestId("import-found-count").waitFor({ timeout: 30_000 });
  await sleep(400);
  const modal = page.getByTestId("import-skills-modal");
  await ctx.keep(async () => {
    const candidates = page.getByTestId("import-candidates");
    // Every candidate is ticked; they will land in a folder named after the source.
    await ctx.hover(candidates.getByText("annotate-screenshots", { exact: true }), { duration: 450, pause: 200 });
    await ctx.hover(modal.getByTestId("import-folder-name"), { duration: 380, pause: 150 });
    await ctx.click(modal.getByRole("button", { name: `Import ${IMPORTED.length} skills` }), { duration: 450 });
  });
  // The copy into the bank is cut: the GIF goes from the click to the new folder.
  await modal.waitFor({ state: "detached", timeout: 30_000 });
  await page.getByTestId("skill-bank-panel").locator('[data-testid^="tree-folder-"]').filter({ hasText: "qa-skills" }).waitFor();
  await sleep(250);
  ctx.mark("import", { before: 100, after: 1000 });
}

/** Close Settings, the bank with it; back to `reviewer`'s skill picker. The
 *  inspector's scroll down to the picker is cut. */
async function backToReviewer(ctx) {
  const { page } = ctx;
  await ctx.keep(async () => {
    await ctx.click(page.getByRole("button", { name: "Close settings" }), { duration: 480 });
    await page.getByTestId("settings-surface").waitFor({ state: "detached" });
  });
  const picker = page.getByTestId("node-skill-selector");
  await picker.evaluate((el) => el.scrollIntoView({ block: "center" }));
  await sleep(250);
  return picker;
}

/** Open the picker and pick `name` (folders start expanded); a pick closes
 *  the picker, and the effective list under it then shows the skill (marked). */
async function pickSkill(ctx, picker, name) {
  const { page } = ctx;
  const popover = page.getByTestId("node-skill-selector-popover");
  const option = popover.getByRole("button", { name, exact: true });
  await ctx.keep(async () => {
    await ctx.click(picker, { duration: 500 });
    await popover.waitFor();
    await option.scrollIntoViewIfNeeded();
    await sleep(200);
    await ctx.click(option, { duration: 480 });
  });
  await popover.waitFor({ state: "detached" });
  await page.getByTestId("node-skill-selector-effective").getByText(name, { exact: true }).waitFor();
  await sleep(200);
  ctx.mark("skill-added", { before: 100, after: 400 });
}

export default {
  name: "skills",
  title: "Skill bank",
  needs: ["history"],
  live: [],
  async setup(instance) {
    // The source: a local git repo (no network), as `~/code/qa-skills`.
    const repo = path.join(instance.home, "code", "qa-skills");
    fs.cpSync(path.join(here, "..", "fixture", "qa-skills"), repo, { recursive: true });
    const git = (...args) => execFileSync("git", args, { cwd: repo, stdio: "ignore", env: { ...process.env, HOME: instance.home } });
    git("init", "-q", "-b", "main");
    git("config", "user.email", "demo@example.com");
    git("config", "user.name", "Demo");
    git("config", "commit.gpgsign", "false");
    git("add", ".");
    git("commit", "-q", "-m", "qa-skills: screenshots, a11y and visual diff");
    const bank = await instance.api("GET", "/settings/skills");
    baseline = { skills: new Set(bank.skills.map((s) => s.id)), folders: new Set(bank.folders.map((f) => f.id)) };
  },
  variants: [
    {
      id: "a",
      label: "Write one by hand, import from a folder, add it to reviewer",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      markers: ["skill-created", "import", "skill-added"],
      async play(ctx) {
        const { page } = ctx;
        await openBank(ctx);
        await ctx.hold(400);
        const paste = page.getByTestId("paste-skill-modal");
        await ctx.keep(async () => {
          await ctx.click(page.getByTestId("skill-add"), { duration: 500 });
          await ctx.click(page.getByTestId("skill-add-menu").getByText("Paste SKILL.md"), { duration: 380 });
          await paste.waitFor();
          // A paste: the text lands at once, the checks light up.
          await page.getByTestId("paste-skill-text").fill(HANDMADE_MD);
          await sleep(500);
          await ctx.click(page.getByTestId("paste-skill-create"), { duration: 450 });
        });
        // The write is cut: the GIF goes from Create to the skill in the bank.
        await paste.waitFor({ state: "detached", timeout: 15_000 });
        await page.getByTestId("skill-detail-name").getByText(HANDMADE, { exact: true }).waitFor();
        await sleep(250);
        ctx.mark("skill-created", { before: 100, after: 800 });
        await importFromFolder(ctx);
        const picker = await backToReviewer(ctx);
        await pickSkill(ctx, picker, HANDMADE);
        await ctx.hold(1500);
        assertNoAgent(ctx.instance);
      },
    },
    {
      id: "b",
      label: "Import from a folder, add an imported one to reviewer",
      viewport: VIEWPORT,
      localStorage: LAYOUT,
      markers: ["import", "skill-added"],
      async play(ctx) {
        await openBank(ctx);
        await ctx.hold(400);
        await importFromFolder(ctx);
        const picker = await backToReviewer(ctx);
        await pickSkill(ctx, picker, "annotate-screenshots");
        await ctx.hold(1500);
        assertNoAgent(ctx.instance);
      },
    },
  ],
};
