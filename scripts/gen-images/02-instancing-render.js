// Phase 2: builds the instancing-grid HTML from 02-instancing-result.json (real
// per-weight subset font files, base64-embedded, from 02-instancing-build.js) and
// screenshots it with Playwright in both light and dark color-scheme emulation.
// Same visual tokens as 01-subsetting-render.js so the README's Examples gallery
// reads as one system, not four unrelated images.

const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const dir = __dirname;
const repoRoot = path.join(dir, '..', '..');
const result = JSON.parse(fs.readFileSync(path.join(dir, '02-instancing-result.json'), 'utf8'));

const W = 1200;
const H = 460;
const PAD = 56;

const fontFaces = result.instances
  .map(
    (inst) => `
  @font-face {
    font-family: 'InterInstance${inst.weight}';
    src: url(data:font/ttf;base64,${inst.fontBase64});
  }`,
  )
  .join('\n');

const columns = result.instances
  .map(
    (inst) => `
    <div class="col">
      <div class="specimen-box"><span class="specimen" style="font-family:'InterInstance${inst.weight}'">${result.sample}</span></div>
      <div class="weight-label">${inst.weight}</div>
    </div>`,
  )
  .join('\n');

const html = `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
  ${fontFaces}
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
    top: 148px;
    width: ${W - PAD * 2}px;
    height: 220px;
    display: flex;
  }
  .col {
    flex: 1;
    display: flex;
    flex-direction: column;
    align-items: center;
  }
  .specimen-box {
    height: 180px;
    display: flex;
    align-items: flex-end;
    justify-content: center;
  }
  .specimen {
    font-size: 108px;
    line-height: 1;
    color: var(--text-primary);
  }
  .weight-label {
    margin-top: 12px;
    font-size: 15px;
    font-weight: 600;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
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
  <div class="title">${result.font} instanced at five weights</div>
  <div class="subtitle">Each column is a real, separate subset_font_full() output at opsz ${result.opsz} &ndash; not a CSS font-weight approximation of one file.</div>

  <div class="grid">
    ${columns}
  </div>

  <div class="caption">${result.license} &middot; wght axis ${result.axesConfirmed.wght[0]}&ndash;${result.axesConfirmed.wght[1]}, opsz axis ${result.axesConfirmed.opsz[0]}&ndash;${result.axesConfirmed.opsz[1]} &ndash; both read from the font's real fvar table.</div>
</body>
</html>`;

const htmlPath = path.join(dir, '02-instancing-chart.html');
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
    const outPath = path.join(outDir, `02-instancing-${scheme}.png`);
    await page.screenshot({ path: outPath });
    console.log('wrote', outPath);
    await page.close();
  }
  await browser.close();
})();
