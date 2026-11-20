// Regenerates ta_vectors.json and indicator_vectors.json from the legacy engine
// so the Rust port is compared against the implementation being replaced, not a
// second hand-written expectation.
//
// Usage: node generate_vectors.mjs [path-to-legacy-engine-dir]

import { writeFileSync } from 'node:fs'
import { dirname, join, resolve } from 'node:path'
import { fileURLToPath, pathToFileURL } from 'node:url'

const here = dirname(fileURLToPath(import.meta.url))
const legacyDir = resolve(
  process.argv[2] ?? join(here, '../../../../../tm-trading/engine'),
)
const ta = await import(pathToFileURL(join(legacyDir, 'ta.mjs')).href)
const indicators = await import(pathToFileURL(join(legacyDir, 'indicators.mjs')).href)

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

const taVectors = {
  source: 'engine/ta.mjs',
  bars,
  closes,
  expected: {
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
    psar: ta.sar(bars),
  },
}

const indicatorVectors = {
  source: 'engine/indicators.mjs',
  bars,
  catalog: indicators.INDICATORS.map((def) => ({
    id: def.id,
    version: def.version,
    kind: def.kind,
    warmup: def.warmup,
    params: Object.entries(def.paramsSchema).map(([name, schema]) => ({
      name,
      default: schema.default,
      min: schema.min,
      max: schema.max,
    })),
    outputs: def.outputs.map((output) => ({
      key: output.key,
      type: output.type,
      color: output.color,
    })),
  })),
  outputs: Object.fromEntries(
    indicators.INDICATORS.map((def) => [
      def.id,
      def.calculate(bars, indicators.resolveParams(def)),
    ]),
  ),
}

writeFileSync(join(here, 'ta_vectors.json'), `${JSON.stringify(taVectors, null, 2)}\n`)
writeFileSync(
  join(here, 'indicator_vectors.json'),
  `${JSON.stringify(indicatorVectors, null, 2)}\n`,
)
console.log('wrote ta_vectors.json and indicator_vectors.json from', legacyDir)
