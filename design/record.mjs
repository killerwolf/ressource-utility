// Enregistre site/demo.html image par image (temps virtuel) puis encode avec ffmpeg.
// Usage : PP_DIR=... node design/record.mjs [fps=30] [frames_dir]
import { createRequire } from "node:module";
import path from "node:path";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
const require = createRequire(process.env.PP_DIR + "/");
const puppeteer = require("puppeteer-core");
const here = path.dirname(fileURLToPath(import.meta.url));
const fps = Number(process.argv[2] || 30);
const dir = process.argv[3] || "/private/tmp/claude-501/frames";
const only = process.env.ONLY ? process.env.ONLY.split(",").map(Number) : null;
fs.mkdirSync(dir, { recursive: true });
const browser = await puppeteer.launch({ executablePath: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", headless: "new" });
const page = await browser.newPage();
await page.setViewport({ width: 1280, height: 800, deviceScaleFactor: 1 });
await page.goto(`file://${here}/../site/demo.html?t=0`);
const dur = await page.evaluate(() => window.__dur);
const frames = only ? only.map((t) => [Math.round(t * fps), t]) : Array.from({ length: Math.round(dur * fps) }, (_, i) => [i, i / fps]);
for (const [i, t] of frames) {
  await page.evaluate((t) => window.__draw(t), t);
  await page.screenshot({ path: `${dir}/f${String(i).padStart(4, "0")}.png`, type: "png" });
}
await browser.close();
console.log("frames:", frames.length);
