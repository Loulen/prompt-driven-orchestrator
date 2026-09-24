#!/bin/sh
# Guard of the `prod-health-check` PDO trigger (cron `* * * * *`).
#   exit 0 — prod is degraded: fire the pipeline; stdout (the incident report)
#            becomes the run's input.
#   exit 1 — prod is healthy: skip this minute.
set -eu
cd "$(dirname "$0")"
. ./ops/prod-probe.env

if [ "$P95_MS" -le "$BUDGET_MS" ] && [ "$ERROR_RATE_PCT" -lt 1 ]; then
  echo "OK — checkout p95 ${P95_MS} ms, 5xx ${ERROR_RATE_PCT} %"
  exit 1
fi

echo "# Incident — checkout API degraded"
echo "- /api/checkout p95 = $((P95_MS / 1000)).$((P95_MS % 1000 / 100)) s (budget ${BUDGET_MS} ms)"
echo "- 5xx rate ${ERROR_RATE_PCT} % since ${DEGRADED_SINCE}"
echo "- last deploy: ${LAST_DEPLOY}"
