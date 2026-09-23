---
name: annotate-screenshots
description: Annotate app screenshots with Pillow — boxes, arrows and numbered callouts on what changed.
---

# Annotate screenshots

1. Capture the page before and after the change (see `playwright-capture`).
2. Open each PNG with Pillow and draw a 3 px box around every changed area.
3. Number the boxes and write one line per number under the image.
4. Save as `<page>-annotated.png` and hand the files over in an `image_list` output.
