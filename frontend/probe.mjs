import { chromium } from "@playwright/test";

const base = "http://127.0.0.1:5305";
const NAME = "e2e-schema-probe";

const browser = await chromium.launch();
const page = await browser.newPage();
const logs = [];
page.on("console", (m) => logs.push(`[${m.type()}] ${m.text()}`));
await page.goto(base + "/");
await page.getByText("Daemon: connected").waitFor({ timeout: 10000 });

// Library tab
await page.getByRole("tab", { name: "Library" }).click();
await page.waitForTimeout(800);

// Is the pipeline row visible?
const rowCount = await page.getByText(NAME).count();
console.log("rows matching NAME:", rowCount);

// list visible pipeline names
const names = await page.locator("aside .truncate.font-medium").allTextContents();
console.log("visible pipeline rows (first 20):", names.slice(0,20));

await page.getByText(NAME).first().click({ timeout: 5000 });
await page.waitForTimeout(1000);

// node-card text reviewer?
const revCount = await page.getByText("reviewer", { exact: true }).count();
console.log("reviewer node-card count:", revCount);
const cards = await page.getByTestId("node-card").count();
console.log("node-card count:", cards);
const allCardText = await page.getByTestId("node-card").allTextContents();
console.log("node-card texts:", allCardText);

// tab bar open tab?
const tabs = await page.locator("[data-testid^='pipeline-tab'],[role='tab']").allTextContents().catch(()=>[]);
console.log("tabs:", tabs);

console.log("CONSOLE ERRORS:", logs.filter(l=>l.startsWith("[error]")));
await browser.close();
