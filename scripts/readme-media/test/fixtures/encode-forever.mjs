// A long ffmpeg encode behind an exit guard like the demo instance's: SIGINT
// → process.exit(130). Used by montage.test.mjs to check a Ctrl+C mid-encode.
import { ffmpeg } from "../../lib/montage.mjs";

process.on("SIGINT", () => process.exit(130));
setTimeout(() => console.log("encoding"), 300);
await ffmpeg(["-f", "lavfi", "-i", "testsrc=size=640x360:rate=30", "-t", "600", "-f", "null", "-"]);
