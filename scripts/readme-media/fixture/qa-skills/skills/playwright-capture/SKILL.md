---
name: playwright-capture
description: Capture full-page screenshots of a local web app with Playwright, at desktop and mobile widths.
---

# Playwright capture

Serve the app, then capture each page at 1280 px and 390 px wide with `page.screenshot({ fullPage: true })`.
Wait for the network to go idle before each capture.
