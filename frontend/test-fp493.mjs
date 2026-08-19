import { chromium } from 'playwright';
import fs from 'fs';

const DAEMON_URL = 'http://127.0.0.1:9999';
const SCREENSHOTS_DIR = '/tmp/fp493-screenshots';

if (!fs.existsSync(SCREENSHOTS_DIR)) {
  fs.mkdirSync(SCREENSHOTS_DIR, { recursive: true });
}

(async () => {
  const browser = await chromium.launch({ headless: true });
  const page = await browser.newPage();

  try {
    console.log('1. Opening daemon UI...');
    await page.goto(DAEMON_URL, { waitUntil: 'networkidle' });
    await page.screenshot({ path: `${SCREENSHOTS_DIR}/01-dashboard.png` });
    console.log('   Screenshot: 01-dashboard.png');

    console.log('2. Creating new run (waiting for UI to be ready)...');
    await page.waitForSelector('[data-testid="new-run-button"]', { timeout: 5000 }).catch(() => {
      console.log('   (button not found, trying alternative selectors)');
    });

    // Try different ways to open new run modal
    const buttons = await page.locator('button').filter({ hasText: /new|run|create/ }).all();
    if (buttons.length > 0) {
      await buttons[0].click();
      await page.waitForTimeout(1000);
    }

    // Look for pipeline selector
    const pipelineInputs = await page.locator('input').filter({ hasText: /pipeline|test-fp493/ }).all();
    if (pipelineInputs.length > 0) {
      await pipelineInputs[0].fill('test-fp493');
      await page.waitForTimeout(500);

      // Click the option if available
      const options = await page.locator('[role="option"]').all();
      if (options.length > 0) {
        await options[0].click();
      }
    }

    // Click create/run button
    const runButtons = await page.locator('button').filter({ hasText: /run|create|launch/ }).all();
    if (runButtons.length > 0) {
      await runButtons[0].click();
      await page.waitForTimeout(2000);
    }

    await page.screenshot({ path: `${SCREENSHOTS_DIR}/02-after-create.png` });
    console.log('   Screenshot: 02-after-create.png');

    // Extract run ID from URL if available
    const url = page.url();
    const runMatch = url.match(/runs\/([^/]+)/);
    if (runMatch) {
      const runId = runMatch[1];
      console.log(`3. Found run ID: ${runId}`);

      // Wait for run to load
      await page.waitForTimeout(2000);

      // Check if surfaces are loading properly
      const panels = await page.locator('[data-panel]').all();
      console.log(`   Found ${panels.length} panels on page`);

      await page.screenshot({ path: `${SCREENSHOTS_DIR}/03-run-view.png` });
      console.log('   Screenshot: 03-run-view.png');

      // Test that endpoints respond to the live run
      console.log('4. Testing endpoints on live run...');

      // Test node_diff (needs a node_id)
      const response1 = await page.request.get(`${DAEMON_URL}/runs/${runId}/nodes/planner/diff`);
      console.log(`   node_diff: ${response1.status()}`);

      // Test artifact
      const response2 = await page.request.get(`${DAEMON_URL}/runs/${runId}/artifact?path=planner/iter-1/plan.md`);
      console.log(`   artifact: ${response2.status()}`);

      console.log('5. Testing that no JS errors occurred...');
      const messages = [];
      page.on('console', msg => {
        if (msg.type() === 'error') {
          messages.push(msg.text());
        }
      });

      if (messages.length > 0) {
        console.log(`   ⚠️ ${messages.length} console errors found`);
      } else {
        console.log('   ✅ No console errors');
      }
    }

  } catch (error) {
    console.error('Error:', error);
  } finally {
    await browser.close();
    console.log('6. Browser closed');
    console.log(`\nScreenshots saved to: ${SCREENSHOTS_DIR}`);
  }
})();
