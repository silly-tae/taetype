// Phase 3: builds the color-glyph strip HTML from 03-color-result.json (real PNG
// bytes straight from get_glyph_bitmap(), nothing composited or faked) and
// screenshots it with Playwright in both light and dark. Same visual tokens as
// phases 1-2 so the README's Examples gallery reads as one system.

const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const dir = __dirname;
const repoRoot = path.join(dir, '..', '..');
const result = JSON.parse(fs.readFileSync(path.join(dir, '03-color-result.json'), 'utf8'));

const W = 1200;
const H = 380;
const PAD = 56;

const columns = result.glyphs
  .map(
    (g) => `
    <div class="col">
      <div class="glyph-box"><img src="data:image/png;base64,${g.pngBase64}" alt="${g.name}"></div>
      <div class="glyph-label">${g.name}</div>
    </div>`,
  )
  .join('\n');

const html = `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
  :root {
    color-scheme: light;
    --surface: #fcfcfb;
    --text-primary: #0b0b0b;
    --text-secondary: #52514e;
    --text-muted: #898781;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      color-scheme: dark;
      --surface: #1a1a19;
      --text-primary: #ffffff;
      --text-secondary: #c3c2b7;
      --text-muted: #898781;
    }
  }
  * { margin: 0; padding: 0; box-sizing: border-box; }
  body {
    width: ${W}px;
    height: ${H}px;
    background: var(--surface);
    font-family: system-ui, sans-serif;
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
    width: ${W - PAD * 2}px;
  }
  .grid {
    position: absolute;
    left: ${PAD}px;
    top: 140px;
    width: ${W - PAD * 2}px;
    display: flex;
  }
  .col {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .glyph-box {
    width: 120px;
    height: 120px;
    display: flex;
    align-items: center;
    justify-content: center;
  }
  .glyph-box img {
    width: 109px;
    height: 109px;
    image-rendering: -webkit-optimize-contrast;
  }
  .glyph-label {
    margin-top: 14px;
    font-size: 14px;
    color: var(--text-secondary);
    text-align: center;
  }
  .caption {
    position: absolute;
    left: ${PAD}px;
    bottom: 28px;
    font-size: 13px;
    color: var(--text-muted);
  }
</style>
</head>
<body>
  <div class="title">${result.font}, real glyph bitmaps</div>
  <div class="subtitle">Each image is get_glyph_bitmap()'s actual PNG output, drawn with zero compositing &ndash; a ${result.tableFormat} strike pulled straight out of the font.</div>

  <div class="grid">
    ${columns}
  </div>

  <div class="caption">${result.license} &middot; nearest-strike selection (asked for 160ppem, font's largest real strike is ${result.glyphs[0].ppem}ppem).</div>
</body>
</html>`;

const htmlPath = path.join(dir, '03-color-chart.html');
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
    await page.waitForTimeout(50);
    const outPath = path.join(outDir, `03-color-${scheme}.png`);
    await page.screenshot({ path: outPath });
    console.log('wrote', outPath);
    await page.close();
  }
  await browser.close();
})();
