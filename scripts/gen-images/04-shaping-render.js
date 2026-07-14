// Phase 4: draws taetype's own naive-vs-shaped glyph runs, glyph by glyph, at
// taetype's own real advances (PUA-remapped fonts from 04-shaping-build.js) —
// not a browser reshaping a real font, which would only prove the FONT has
// ligatures. Same visual tokens as phases 1-3.

const fs = require('fs');
const path = require('path');
const { chromium } = require('playwright');

const dir = __dirname;
const repoRoot = path.join(dir, '..', '..');
const result = JSON.parse(fs.readFileSync(path.join(dir, '04-shaping-result.json'), 'utf8'));

const W = 1200;
const H = 640;
const PAD = 56;
const FONT_SIZE = 96; // px
const SCALE = FONT_SIZE / 1000; // taetype advances are in 1000-upm units

// vertical budget, computed up front so nothing collides (bracket+label sit
// BELOW the shaped glyph row, so the shaped row needs real clearance before
// the stats line, which needs its own clearance before the caption)
const Y_UNSHAPED_LABEL = 160;
const Y_UNSHAPED_GLYPHS = 184;
const Y_SHAPED_LABEL = 330;
const Y_SHAPED_GLYPHS = 354;
const Y_STATS = 540;
const Y_CAPTION_FROM_BOTTOM = 28;

function glyphSpans(gids, advances, puaBase = 0xe000) {
  let x = 0;
  return gids.map((_, i) => {
    const span = { char: String.fromCodePoint(puaBase + i), x };
    x += advances[i] * SCALE;
    return span;
  });
}

const naiveSpans = glyphSpans(result.naive.gids, result.naive.widths);
const shapedSpans = glyphSpans(result.shaped.gids, result.shaped.advances);

const naiveHtml = naiveSpans.map((s) => `<span style="left:${s.x}px">${s.char}</span>`).join('');
const shapedHtml = shapedSpans.map((s) => `<span style="left:${s.x}px">${s.char}</span>`).join('');

// bracket + label under the ligature glyph, if one was found
let ligatureAnnotation = '';
if (result.shaped.ligatureIndex !== -1) {
  const i = result.shaped.ligatureIndex;
  const glyphX = shapedSpans[i].x;
  const glyphW = result.shaped.advances[i] * SCALE;
  ligatureAnnotation = `
    <div class="bracket" style="left:${glyphX}px; width:${glyphW}px"></div>
    <div class="bracket-label" style="left:${glyphX}px; width:${glyphW}px">"${result.shaped.ligatureSpan}" &rarr; 1 glyph</div>`;
}

const html = `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<style>
  @font-face { font-family: 'Naive'; src: url(data:font/ttf;base64,${result.naive.fontBase64}); }
  @font-face { font-family: 'Shaped'; src: url(data:font/ttf;base64,${result.shaped.fontBase64}); }
  :root {
    color-scheme: light;
    --surface: #fcfcfb;
    --text-primary: #0b0b0b;
    --text-secondary: #52514e;
    --text-muted: #898781;
    --accent: #2a78d6;
  }
  @media (prefers-color-scheme: dark) {
    :root {
      color-scheme: dark;
      --surface: #1a1a19;
      --text-primary: #ffffff;
      --text-secondary: #c3c2b7;
      --text-muted: #898781;
      --accent: #3987e5;
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
  .row-label {
    position: absolute;
    left: ${PAD}px;
    font-size: 13px;
    font-weight: 600;
    color: var(--text-muted);
    text-transform: uppercase;
    letter-spacing: 0.04em;
  }
  .glyph-row {
    position: absolute;
    left: ${PAD}px;
    height: ${FONT_SIZE * 1.3}px;
  }
  .glyph-row span {
    position: absolute;
    top: 0;
    font-size: ${FONT_SIZE}px;
    line-height: 1;
    color: var(--text-primary);
    white-space: pre;
  }
  #naive-glyphs span { font-family: 'Naive'; }
  #shaped-glyphs span { font-family: 'Shaped'; }
  .bracket {
    position: absolute;
    top: ${FONT_SIZE * 1.08}px;
    height: 6px;
    border-bottom: 2px solid var(--accent);
    border-left: 2px solid var(--accent);
    border-right: 2px solid var(--accent);
  }
  .bracket-label {
    position: absolute;
    top: ${FONT_SIZE * 1.08 + 14}px;
    font-size: 13px;
    color: var(--accent);
    text-align: center;
    white-space: nowrap;
  }
  .stats {
    position: absolute;
    left: ${PAD}px;
    top: ${Y_STATS}px;
    font-size: 14px;
    color: var(--text-secondary);
    font-variant-numeric: tabular-nums;
  }
  .caption {
    position: absolute;
    left: ${PAD}px;
    bottom: ${Y_CAPTION_FROM_BOTTOM}px;
    font-size: 13px;
    color: var(--text-muted);
  }
</style>
</head>
<body>
  <div class="title">${result.font}, "${result.word}" &ndash; unshaped vs. shape_text()</div>
  <div class="subtitle">Each glyph below is drawn individually at taetype's own real advances (Private-Use-Area remapped fonts) &ndash; not the browser reshaping text on its own.</div>

  <div class="row-label" style="top:${Y_UNSHAPED_LABEL}px">Unshaped (per-character cmap only)</div>
  <div class="glyph-row" id="naive-glyphs" style="top:${Y_UNSHAPED_GLYPHS}px">${naiveHtml}</div>

  <div class="row-label" style="top:${Y_SHAPED_LABEL}px">Shaped (real shape_text() output)</div>
  <div class="glyph-row" id="shaped-glyphs" style="top:${Y_SHAPED_GLYPHS}px">
    ${shapedHtml}
    ${ligatureAnnotation}
  </div>

  <div class="stats">${result.naive.gids.length} glyphs, ${Math.round(result.naive.totalWidth)} units &rarr; ${result.shaped.gids.length} glyphs, ${Math.round(result.shaped.totalWidth)} units</div>

  <div class="caption">${result.license} &middot; glyph positions are shape_text()'s real advances, not CSS text layout.</div>
</body>
</html>`;

const htmlPath = path.join(dir, '04-shaping-chart.html');
fs.writeFileSync(htmlPath, html);

const outDir = path.join(repoRoot, 'assets', 'images');
fs.mkdirSync(outDir, { recursive: true });

(async () => {
  const browser = await chromium.launch();

  // verify both fonts actually loaded before trusting any screenshot — the
  // exact check that caught phase 2's cmap/name/post failures
  const probe = await browser.newPage();
  probe.on('console', (msg) => console.log('CONSOLE:', msg.text().slice(0, 200)));
  await probe.goto('file://' + htmlPath);
  const status = await probe.evaluate(async () => {
    await document.fonts.ready;
    const out = [];
    document.fonts.forEach((f) => out.push({ family: f.family, status: f.status }));
    return out;
  });
  console.log('font status:', status);
  if (status.some((s) => s.status !== 'loaded')) throw new Error('a demo font failed to load — see console output above');
  await probe.close();

  for (const scheme of ['light', 'dark']) {
    const page = await browser.newPage({
      viewport: { width: W, height: H },
      deviceScaleFactor: 2,
      colorScheme: scheme,
    });
    await page.goto('file://' + htmlPath);
    await page.waitForTimeout(50);
    const outPath = path.join(outDir, `04-shaping-${scheme}.png`);
    await page.screenshot({ path: outPath });
    console.log('wrote', outPath);
    await page.close();
  }
  await browser.close();
})();
