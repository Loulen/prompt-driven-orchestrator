import { chromium } from "playwright";
const [,, outPrefix, ...steps] = process.argv;
const browser = await chromium.launch({ channel: "chrome" });
const page = await browser.newPage({ viewport: { width: 1440, height: 900 }, colorScheme: "dark" });
page.on("console", (m) => { if (m.text().includes("[land]")) console.log(m.text()); });
await page.goto("http://localhost:5199/");
await page.waitForTimeout(1500);
let i = 0;
for (const step of steps) {
  const eq = step.indexOf("="); const kind = step.slice(0, eq); const arg = step.slice(eq + 1);
  if (kind === "click") await page.click(arg);
  if (kind === "clicktext") await page.getByText(arg, { exact: false }).first().click();
  if (kind === "wait") await page.waitForTimeout(Number(arg));
  if (kind === "key") await page.keyboard.press(arg);
  if (kind === "shot") { await page.waitForTimeout(400); await page.screenshot({ path: `${outPrefix}-${arg}.png` }); }
}
await browser.close();
