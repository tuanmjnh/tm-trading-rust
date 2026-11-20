// Regenerates ta_vectors.json from the legacy engine's ta.mjs so the Rust port
// is compared against the implementation being replaced, not against a second
// hand-written expectation.
//
// Usage: node generate_ta_vectors.mjs [path-to-engine/ta.mjs]

import { writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const legacyPath = resolve(
  process.argv[2] ?? join(here, '../../../../../tm-trading/engine/ta.mjs'),
)
const ta = await import(pathToFileURL(legacyPath).href)

// Deterministic pseudo-random series: the fixture must be reproducible.
let seed = 42
const random = () => {
  seed = (seed * 1103515245 + 12345) % 2147483648
  return seed / 2147483648
}
const round = (value) => Math.round(value * 100) / 100

const bars = []
let price = 100
for (let i = 0; i < 120; i++) {
  const open = price
  const close = round(open + (random() - 0.5) * 4)
  let high = round(Math.max(open, close) + random() * 2)
  let low = round(Math.min(open, close) - random() * 2)
  let volume = Math.round(random() * 1000) + 1
  if (i === 17) volume = 0 // zero-volume bar
  if (i === 33) high = low = close // degenerate range
  bars.push({ open, high, low, close, volume })
  price = close
}

// Missing closes exercise `na` propagation in the values-based primitives.
const closes = bars.map((bar) => bar.close)
closes[7] = null
closes[58] = null

const expected = {
  sma20: ta.sma(closes, 20),
  ema12: ta.ema(closes, 12),
  rma14: ta.rma(closes, 14),
  trueRange: ta.trueRange(bars),
  atr14: ta.atr(bars, 14),
  rollingSum10: ta.rollingSum(closes, 10),
  rsi14: ta.rsi(closes, 14),
  stdev20: ta.stdev(closes, 20),
  macd12_26_9: ta.macd(closes, 12, 26, 9),
  vwap: ta.vwap(bars),
  obv: ta.obv(bars),
  cmf20: ta.cmf(bars, 20),
  donchian20: ta.donchian(bars, 20),
}

writeFileSync(
  join(here, 'ta_vectors.json'),
  `${JSON.stringify({ source: 'engine/ta.mjs', closes, bars, expected }, null, 2)}\n`,
)
console.log('wrote ta_vectors.json from', legacyPath)
