import { test, expect, type Page } from "@playwright/test";
import * as fs from "node:fs/promises";
import * as path from "node:path";
import { fileURLToPath } from "node:url";

// S5 (ADR-0091): the shared design system in a real browser, against the real
// embedded front. The e2e daemon is the shipped binary, with no extension: this
// spec plays the design-system test extension (crates/pdo/tests/test_extension/
// kit.js) on the network, a page drawn with nothing but the primitives of
// `@pdo/extension-ui` and the tokens. What is under test is the front: it hands
// the primitives to the bundle, paints them in its tokens, and the page follows
// the theme; the token sheet and the fonts are served by the binary.

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const BUNDLE_PATH = path.resolve(__dirname, "..", "..", "crates/pdo/tests/test_extension/kit.js");

/** `GET /ext` as the daemon would answer it for the design-system test extension. */
const DECLARATION = [
  {
    id: "kit",
    name: "Kit",
    version: "0.0.0",
    ui: {
      bundle_url: "/ext/kit/ui.js",
      react_major: 19,
      ui_module_major: 0,
      locations: [],
      contributions: [{ location: "topbar/start", id: "deploy", label: "Deploy", type: "page", component: "KitPage" }],
      commands: [],
    },
  },
];

async function playKitExtension(page: Page) {
  // The interface's default scale, 120 % (the suite runs at 100 %).
  await page.addInitScript(() => localStorage.setItem("pdo.ui.scale", "120"));
  const bundle = await fs.readFile(BUNDLE_PATH, "utf8");
  await page.route("**/ext", (route) => route.fulfill({ json: DECLARATION }));
  await page.route("**/ext/kit/ui.js", (route) => route.fulfill({ body: bundle, contentType: "text/javascript" }));
}

/** The computed value of `property` on the element `testId`. */
function computed(page: Page, testId: string, property: string) {
  return page.getByTestId(testId).evaluate((el, p) => getComputedStyle(el).getPropertyValue(p), property);
}

/** A role's colour as the browser resolves it on <html>. */
function role(page: Page, name: string) {
  return page.evaluate((n) => {
    const probe = document.createElement("span");
    probe.style.color = `var(${n})`;
    document.body.appendChild(probe);
    const value = getComputedStyle(probe).color;
    probe.remove();
    return value;
  }, name);
}

test("an extension page made of the primitives only looks native and follows the theme", async ({ page }) => {
  const external: string[] = [];
  page.on("request", (request) => {
    const url = new URL(request.url());
    if (url.hostname !== "127.0.0.1" && url.hostname !== "localhost") external.push(request.url());
  });
  await playKitExtension(page);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });

  // Its entry opens the page.
  await page.getByTestId("extension-nav-kit:deploy").click();
  const kit = page.getByTestId("kit-page");
  await expect(kit).toBeVisible();
  await expect(page.getByTestId("kit-env-title")).toHaveText("qa");
  await expect(kit.getByRole("alert")).toContainText("Health check timed out");

  // The primitives answer as the core's do: tabs, a menu, a modal, a toast.
  await kit.getByRole("tab", { name: /History/ }).click();
  await expect(kit.getByTestId("kit-deploy-row")).toHaveCount(50);
  expect(await page.evaluate(() => document.getElementById("root")!.currentCSSZoom)).toBeCloseTo(1.2, 2);
  const more = kit.getByRole("button", { name: "More" });
  await more.click();
  const menu = page.getByRole("menu", { name: "qa" });
  await expect(menu.getByRole("menuitem", { name: /Copy URL/ })).toBeVisible();
  // Under the zoom, the menu still opens right under its trigger, its right edge on the trigger's.
  const trigger = (await more.boundingBox())!;
  const panel = (await page.locator(".d-pop").boundingBox())!;
  expect(Math.abs(panel.y - (trigger.y + trigger.height))).toBeLessThan(4);
  expect(Math.abs(panel.x + panel.width - (trigger.x + trigger.width))).toBeLessThan(4);
  await page.keyboard.press("Escape");
  await expect(menu).toHaveCount(0);
  await kit.getByRole("button", { name: "Deploy", exact: true }).click();
  const modal = page.getByRole("dialog", { name: "Deploy qa" });
  await expect(modal).toBeVisible();
  await modal.getByRole("button", { name: "Deploy", exact: true }).click();
  await expect(page.getByTestId("toast-info")).toContainText("Deploying qa v2.14.9");

  // Settings: a field's error at the row end.
  await kit.getByRole("tab", { name: "Settings" }).click();
  const replicas = kit.getByLabel("Replicas");
  await replicas.fill("40");
  await expect(kit.getByRole("alert")).toHaveText("Must be 1 to 20");
  await expect(replicas).toHaveAttribute("aria-invalid", "true");
  // A field stretches its text input, never its switch nor its segmented control.
  const fieldWidth = (await kit.locator(".d-field__ctl").first().boundingBox())!.width;
  const autoDeploy = (await kit.getByRole("switch", { name: "Auto-deploy" }).boundingBox())!;
  expect(autoDeploy.width).toBeLessThan(40);
  const strategy = (await kit.getByRole("radiogroup", { name: "Strategy" }).boundingBox())!;
  expect(strategy.width).toBeLessThan(fieldWidth / 2);

  // In the core's tokens: the page and its primary button paint with the roles.
  expect(await computed(page, "kit-page", "background-color")).toBe(await role(page, "--surface-0"));
  const primary = kit.getByRole("button", { name: "Save" });
  const primaryBackground = () => primary.evaluate((el) => getComputedStyle(el).backgroundColor);
  expect(await primaryBackground()).toBe(await role(page, "--primary"));
  const darkSurface = await computed(page, "kit-page", "background-color");

  // The theme switches: the page follows at once, in the light roles.
  await page.evaluate(() => document.documentElement.setAttribute("data-theme", "light"));
  const lightSurface = await computed(page, "kit-page", "background-color");
  expect(lightSurface).not.toBe(darkSurface);
  expect(lightSurface).toBe(await role(page, "--surface-0"));
  expect(await primaryBackground()).toBe(await role(page, "--primary"));

  // Archivo, served by the binary; no font from a third party.
  expect(await page.evaluate(() => document.fonts.check('13px "Archivo"'))).toBe(true);
  expect(await computed(page, "kit-page", "font-family")).toContain("Archivo");
  expect(external).toEqual([]);
});

test("the theme chosen in Settings repaints the extension's page", async ({ page }) => {
  await playKitExtension(page);
  await page.goto("/");
  await expect(page.getByLabel("Daemon: connected")).toBeVisible({ timeout: 10_000 });
  await page.getByTestId("extension-nav-kit:deploy").click();
  const dark = await computed(page, "kit-page", "background-color");
  await page.keyboard.press("Escape");
  await expect(page.getByTestId("kit-page")).toHaveCount(0);

  await page.getByTestId("open-settings").click();
  await page.getByRole("radio", { name: /Light/ }).click();
  await page.keyboard.press("Escape");
  await page.getByTestId("extension-nav-kit:deploy").click();
  const light = await computed(page, "kit-page", "background-color");
  expect(light).not.toBe(dark);
  expect(light).toBe(await role(page, "--surface-0"));
  expect(await page.evaluate(() => document.documentElement.getAttribute("data-theme"))).toBe("light");
});

test("the token sheet and the fonts are served at their stable address", async ({ request }) => {
  const sheet = await request.get("/shared/tokens.css");
  expect(sheet.status()).toBe(200);
  expect(sheet.headers()["content-type"]).toContain("text/css");
  const css = await sheet.text();
  expect(css).toContain("--surface-0:");
  expect(css).toContain('[data-theme="light"]');
  for (const [, file] of css.matchAll(/url\("\/shared\/fonts\/([^"]+)"\)/g)) {
    const font = await request.get(`/shared/fonts/${file}`);
    expect(font.status(), file).toBe(200);
    expect(font.headers()["content-type"], file).toContain("font/woff2");
  }
  const licence = await request.get("/shared/fonts/Archivo-OFL.txt");
  expect(await licence.text()).toContain("SIL Open Font License, Version 1.1");
});

test("a page outside the interface that links the token sheet paints in the app's palette", async ({ page }) => {
  await page.goto("/");
  const appSurface = await role(page, "--surface-0");
  // An extension's page outside the interface: only the token sheet, no module.
  await page.route("**/ext/kit/outside", (route) =>
    route.fulfill({
      contentType: "text/html",
      body: `<!doctype html><html data-theme="dark"><head><link rel="stylesheet" href="/shared/tokens.css"></head>
        <body style="margin:0;background:var(--surface-0);color:var(--text);font-family:var(--font-ui)">
        <p data-testid="outside">Sign in</p></body></html>`,
    }),
  );
  await page.goto("/ext/kit/outside");
  await expect(page.getByTestId("outside")).toBeVisible();
  expect(await page.evaluate(() => getComputedStyle(document.body).backgroundColor)).toBe(appSurface);
});
