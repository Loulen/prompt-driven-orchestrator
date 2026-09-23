The prod health check just fired: your input is its incident report (the degraded endpoint, its p95 and 5xx rate, the last deploy).
Find the cause. Read the logs and the diff of the last deploy, and reproduce the failing call if you can.
In `incidents`, set `Incident_found` to `true` if the incident is real and caused by our code, with the cause, the evidence and the fix you suggest. Set it to `false` for a false alarm or an outside cause, and say why.
