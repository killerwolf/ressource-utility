// Exporte les maquettes PNG @2x depuis scenes.html avec Chrome headless.
// Usage : PP_DIR=/chemin/avec/puppeteer-core node design/render.mjs
import { createRequire } from "node:module";
import path from "node:path";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import { fileURLToPath } from "node:url";
const require = createRequire(process.env.PP_DIR + "/");
const puppeteer = require("puppeteer-core");
const here = path.dirname(fileURLToPath(import.meta.url));
const out = path.resolve(here, "../site/mockups");
const jobs = [
  ["menu", "dark", "menu-dark"], ["menu", "light", "menu-light"],
  ["processus", "dark", "processus"], ["processus-dialog", "dark", "processus-dialog"],
  ["bilan", "light", "bilan"], ["reglages", "light", "reglages"],
  ["bilan", "dark", "bilan-dark"], ["reglages", "dark", "reglages-dark"],
  ["processus", "light", "processus-light"], ["processus-dialog", "light", "processus-dialog-light"],
];
process.env.PATH += ":/opt/homebrew/bin";
const browser = await puppeteer.launch({ executablePath: "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome", headless: "new" });
const page = await browser.newPage();
for (const [scene, theme, name] of jobs) {
  await page.setViewport({ width: 1400, height: 1000, deviceScaleFactor: 2 });
  await page.goto(`file://${here}/scenes.html?scene=${scene}&theme=${theme}`);
  const el = await page.$(".desk");
  const raw = `${out}/${name}.raw.png`;
  await el.screenshot({ path: raw });
  // Palette 192 couleurs : ~3x plus léger, sans différence visible sur ces dégradés.
  spawnSync("ffmpeg", ["-y", "-loglevel", "error", "-i", raw, "-vf",
    "split[a][b];[a]palettegen=max_colors=192:stats_mode=diff[p];[b][p]paletteuse=dither=bayer:bayer_scale=3",
    `${out}/${name}.png`]);
  fs.unlinkSync(raw);
  console.log("ok", name);
}
await browser.close();
