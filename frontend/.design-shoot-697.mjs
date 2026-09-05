import { chromium } from "playwright";
const OUT = process.argv[2];
const browser = await chromium.launch({ channel: "chrome" });
const ctx = await browser.newContext({ viewport: { width: 1280, height: 800 }, deviceScaleFactor: 2, colorScheme: "dark" });
const page = await ctx.newPage();
async function open(q) {
  await page.goto(`http://localhost:5197/?${q}`, { waitUntil: "networkidle" });
  await page.waitForTimeout(800);
}
// Status-bar badge variants (crop the footer's right end)
for (const v of ["a", "b", "c"]) {
  await open(`proto=newer&badge=${v}`);
  const footer = page.locator("footer");
  const box = await footer.boundingBox();
  await page.screenshot({ path: `${OUT}/badge-${v}.png`, clip: { x: box.x + box.width - 520, y: box.y - 2, width: 520, height: box.height + 4 } });
}
await open(`proto=uptodate`);
{ const box = await page.locator("footer").boundingBox();
  await page.screenshot({ path: `${OUT}/badge-none-uptodate.png`, clip: { x: box.x + box.width - 520, y: box.y - 2, width: 520, height: box.height + 4 } }); }
// Settings section states
async function section(scenario, name, after) {
  await open(`proto=${scenario}&badge=b`);
  await page.locator('[data-testid="statusbar-version"]').click();
  await page.waitForSelector('[data-testid="setting-version-update"]');
  await page.waitForTimeout(600);
  if (after) await after();
  const el = page.locator('[data-section-id="version-update"]');
  await el.screenshot({ path: `${OUT}/${name}.png` });
}
await section("newer", "section-newer");
await section("uptodate", "section-uptodate");
await section("offline", "section-offline-after-check", async () => { await page.click('[data-testid="setting-version-check-now"]'); await page.waitForTimeout(1300); });
await section("disabled", "section-disabled");
await section("unknown", "section-unknown-method");
await section("newer", "section-checking", async () => { await page.click('[data-testid="setting-version-check-now"]'); await page.waitForTimeout(200); });
// Full settings page for context
await open(`proto=newer&badge=b`);
await page.locator('[data-testid="statusbar-version"]').click();
await page.waitForSelector('[data-testid="setting-version-update"]');
await page.waitForTimeout(700);
await page.screenshot({ path: `${OUT}/settings-general-full.png` });
await browser.close();
