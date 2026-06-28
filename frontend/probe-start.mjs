import { chromium } from "@playwright/test";
const base = "http://127.0.0.1:5305";
const NAME = "e2e-start-probe";

// Override tmux so the worker session is a harmless sleep.
const browser = await chromium.launch();
const page = await browser.newPage();
const logs = [];
page.on("console", (m) => { if (m.type()==="error") logs.push(m.text()); });

await page.goto(base + "/");
await page.getByText("Daemon: connected").waitFor({ timeout: 10000 });

// Create run via multipart
const resp = await page.request.post(base + "/runs", {
  multipart: { pipeline: NAME, input: "hello from start node test" },
});
console.log("create run status:", resp.status());
const json = await resp.json();
const runId = json.run_id;
console.log("run_id:", runId);

// Find run entry. Try short and full.
await page.waitForTimeout(800);
const shortCount = await page.getByText(runId.slice(0,8)).count();
const fullCount = await page.getByText(runId).count();
console.log("run entry short count:", shortCount, "full count:", fullCount);
await page.getByText(runId.slice(0,8)).first().click({ timeout: 5000 });
await page.waitForTimeout(1000);

const rf = await page.locator(".react-flow").count();
console.log("react-flow count:", rf);
const cards = await page.getByTestId("node-card").allTextContents();
console.log("node-card texts:", cards);
const startNodeOld = await page.locator(".start-node").count();
console.log(".start-node (old) count:", startNodeOld);

// Click the Start card
await page.getByText("Start", { exact: true }).first().click();
await page.waitForTimeout(500);
const insp = await page.locator(".start-inspector").count();
console.log(".start-inspector count:", insp);
if (insp > 0) {
  const inspText = await page.locator(".start-inspector").innerText();
  console.log("inspector text:\n", inspText);
  const inputTxt = await page.locator(".start-input-text").innerText().catch(()=>"<none>");
  console.log("start-input-text:", JSON.stringify(inputTxt));
}
console.log("console errors:", logs);
await browser.close();
