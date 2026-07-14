// Phase 1: builds the dumbbell chart HTML from 01-subsetting-result.json (real
// numbers from 01-subsetting-measure.js, nothing fabricated) and screenshots it
// with Playwright in both light and dark color-scheme emulation, per
// representation-task-map.md's dataviz pass (form: "before -> after per item" =
// dumbbell, palette validated via validate_palette.js before this was written).

const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const dir = __dirname;
const repoRoot = path.join(dir, '..', '..');
const result = JSON.parse(fs.readFileSync(path.join(dir, '01-subsetting-result.json'), 'utf8'));

const W = 1200;
const H = 480;
const PAD = 56;
const AXIS_Y = 380;
const AXIS_X0 = PAD;
const AXIS_X1 = W - PAD;
const MAX_KB = 900; // clean round scale ceiling, both values fit inside it

const kb = (bytes) => bytes / 1024;
const xScale = (bytesVal) => AXIS_X0 + (kb(bytesVal) / MAX_KB) * (AXIS_X1 - AXIS_X0);

const beforeX = xScale(result.originalBytes);
const afterX = xScale(result.subsetBytes);
const dotY = 300;

const ticks = [0, 200, 400, 600, 800];
const tickMarks = ticks
  .map((t) => {
    const x = AXIS_X0 + (t / MAX_KB) * (AXIS_X1 - AXIS_X0);
    return `<line class="grid" x1="${x}" y1="${AXIS_Y - 4}" x2="${x}" y2="${AXIS_Y + 4}"/>
      <text class="tick" x="${x}" y="${AXIS_Y + 22}" text-anchor="middle">${t}</text>`;
  })
  .join('\n');

const fmtKB = (bytes) => `${Math.round(kb(bytes))} KB`;

const html = `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
  @font-face {
    font-family: 'Inter';
    src: url('${path.join(repoRoot, 'assets', 'fonts', 'inter', 'InterVariable.ttf')}');
    font-weight: 100 900;
  }
  :root {
    color-scheme: light;
    --surface: #fcfcfb;
    --text-primary: #0b0b0b;
    --text-secondary: #52514e;
    --text-muted: #898781;
    --gridline: #e1e0d9;
    --axis: #c3c2b7;
    --dot-before: #86b6ef;
    --dot-after: #2a78d6;
    --line: #2a78d6;
    --ring: #fcfcfb;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      color-scheme: dark;
      --surface: #1a1a19;
      --text-primary: #ffffff;
      --text-secondary: #c3c2b7;
      --text-muted: #898781;
      --gridline: #2c2c2a;
      --axis: #383835;
      --dot-before: #6da7ec;
      --dot-after: #3987e5;
      --line: #3987e5;
      --ring: #1a1a19;
    }
  }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    width: ${W}px;
    height: ${H}px;
    background: var(--surface);
    font-family: 'Inter', system-ui, sans-serif;
    position: relative;
    overflow: hidden;
  }
  .title {
    position: absolute;
    left: ${PAD}px;
    top: 48px;
    font-size: 22px;
    font-weight: 600;
    color: var(--text-primary);
  }
  .subtitle {
    position: absolute;
    left: ${PAD}px;
    top: 80px;
    font-size: 14px;
    color: var(--text-secondary);
  }
  .hero {
    position: absolute;
    left: ${PAD}px;
    top: 128px;
    font-size: 64px;
    font-weight: 700;
    color: var(--text-primary);
    font-variant-numeric: proportional-nums;
  }
  .hero-sub {
    position: absolute;
    left: ${PAD}px;
    top: 208px;
    font-size: 17px;
    color: var(--text-secondary);
  }
  .caption {
    position: absolute;
    left: ${PAD}px;
    bottom: 28px;
    font-size: 13px;
    color: var(--text-muted);
  }
  svg { position: absolute; top: 0; left: 0; }
  .grid { stroke: var(--gridline); stroke-width: 1; }
  .axis-line { stroke: var(--axis); stroke-width: 1; }
  .tick { font-size: 12px; fill: var(--text-muted); font-variant-numeric: tabular-nums; }
  .conn { stroke: var(--line); stroke-width: 2; stroke-linecap: round; }
  .dot-ring { stroke: var(--ring); stroke-width: 2; }
  .lbl-eyebrow { font-size: 12px; fill: var(--text-muted); }
  .lbl-value { font-size: 16px; font-weight: 600; fill: var(--text-primary); font-variant-numeric: proportional-nums; }
</style>
</head>
<body>
  <div class="title">${result.font}, subset to one paragraph's glyphs</div>
  <div class="subtitle">${result.uniqueGlyphsUsed} of ${result.totalGlyphs.toLocaleString()} glyphs used &middot; ${result.license}</div>

  <div class="hero">${result.reductionPct}% smaller</div>
  <div class="hero-sub">${fmtKB(result.originalBytes)} &rarr; ${fmtKB(result.subsetBytes)}, from a real subset_font_full() call</div>

  <svg width="${W}" height="${H}">
    <line class="axis-line" x1="${AXIS_X0}" y1="${AXIS_Y}" x2="${AXIS_X1}" y2="${AXIS_Y}"/>
    ${tickMarks}
    <text class="tick" x="${AXIS_X1}" y="${AXIS_Y + 40}" text-anchor="end">KB (linear)</text>

    <line class="conn" x1="${afterX}" y1="${dotY}" x2="${beforeX}" y2="${dotY}"/>

    <circle cx="${beforeX}" cy="${dotY}" r="9" class="dot-ring" fill="var(--dot-before)"/>
    <text class="lbl-eyebrow" x="${beforeX}" y="${dotY - 34}" text-anchor="end">Original</text>
    <text class="lbl-value" x="${beforeX}" y="${dotY - 16}" text-anchor="end">${fmtKB(result.originalBytes)}</text>

    <circle cx="${afterX}" cy="${dotY}" r="9" class="dot-ring" fill="var(--dot-after)"/>
    <text class="lbl-eyebrow" x="${afterX}" y="${dotY - 34}" text-anchor="start">Subsetted</text>
    <text class="lbl-value" x="${afterX}" y="${dotY - 16}" text-anchor="start">${fmtKB(result.subsetBytes)}</text>
  </svg>

  <div class="caption">Sample: a real ${result.sampleTextLength}-character body paragraph, not a pangram &ndash; pangrams inflate glyph coverage.</div>
</body>
</html>`;

const htmlPath = path.join(dir, '01-subsetting-chart.html');
fs.writeFileSync(htmlPath, html);

const outDir = path.join(repoRoot, 'assets', 'images');
fs.mkdirSync(outDir, { recursive: true });

(async () => {
  const browser = await chromium.launch();
  for (const scheme of ['light', 'dark']) {
    const page = await browser.newPage({
      viewport: { width: W, height: H },
      deviceScaleFactor: 2,
      colorScheme: scheme,
    });
    await page.goto('file://' + htmlPath);
    await page.waitForTimeout(50); // let the webfont apply
    const outPath = path.join(outDir, `01-subsetting-${scheme}.png`);
    await page.screenshot({ path: outPath });
    console.log('wrote', outPath);
    await page.close();
  }
  await browser.close();
})();
