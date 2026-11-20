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
const vsa = await import(pathToFileURL(join(legacyDir, 'methods/vsa.mjs')).href)

// Deterministic pseudo-random series: the fixture must be reproducible.
let seed = 42
const random = () => {
  seed = (seed * 1103515245 + 12345) % 2147483648
  return seed / 2147483648
}
const round = (value) => Math.round(value * 100) / 100

const bars = []
let price = 100
const firstOpen = Date.UTC(2026, 0, 1, 0, 0, 0)
for (let i = 0; i < 120; i++) {
  const open = price
  const close = round(open + (random() - 0.5) * 4)
  let high = round(Math.max(open, close) + random() * 2)
  let low = round(Math.min(open, close) - random() * 2)
  let volume = Math.round(random() * 1000) + 1
  if (i === 17) volume = 0 // zero-volume bar
  if (i === 33) high = low = close // degenerate range
  bars.push({ time: firstOpen + i * 60_000, open, high, low, close, volume })
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
  sessions: sessionCases(),
  buckets: bucketCases(),
}
function sessionCases() {
  // Two windows of 30-minute steps: the second crosses a US clock change, which
  // is where a naive fixed-offset conversion diverges from `Intl`.
  const cases = []
  const windows = [
    [Date.UTC(2026, 2, 7, 18, 0, 0), 96],
    [Date.UTC(2026, 9, 31, 4, 0, 0), 96],
  ]
  const zones = [
    ['0800-1600', 'Asia/Ho_Chi_Minh'],
    ['2200-0600', 'Asia/Ho_Chi_Minh'],
    ['0930-1600', 'America/New_York'],
    ['0000-0000', 'UTC'],
    ['0000-2359', ''],
    ['2200-0600', 'Europe/Berlin'],
    ['0800-1600', 'Not/AZone'],
  ]
  for (const [start, steps] of windows) {
    for (const [session, timezone] of zones) {
      for (let i = 0; i < steps; i++) {
        const timeMs = start + i * 30 * 60_000
        let expected
        try {
          expected = ta.sessionOk(timeMs, session, timezone)
        } catch {
          expected = false // unknown zone: the port suppresses instead of throwing
        }
        cases.push([timeMs, session, timezone, expected])
      }
    }
  }
  return cases
}

function bucketCases() {
  const ratios = { rP: 2.2, rVH: 1.8, rH: 1.2, rN: 0.8, rL: 0.4 }
  const averages = [null, 0, -5, 100]
  const volumes = [0, 39.9, 40, 79.9, 80, 119.9, 120, 179.9, 180, 219.9, 220, 1000]
  const cases = []
  for (const average of averages) {
    for (const volume of volumes) {
      cases.push([volume, average, ta.vsaBucket(volume, average, ratios)])
    }
  }
  return cases
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

// -----------------------------------------------------------------------------
// VSA method vectors
// -----------------------------------------------------------------------------

// A hand-built scenario that produces every event kind: selling exhaustion,
// buying climax, setups, no supply, no demand, and the Wyckoff events.
function vsaBars() {
  const bars = []
  let time = Date.UTC(2026, 0, 1, 0, 0, 0)
  const push = (open, high, low, close, volume) => {
    bars.push({ time, open, high, low, close, volume })
    time += 60_000
  }

  // A: decline from 100 to 92.
  for (let i = 0; i < 20; i++) {
    const open = 100 - i * 0.4
    const close = open - 0.3
    push(open, open + 0.25, close - 0.25, close, 500)
  }
  // B: the swing low, then a bounce.
  push(91.5, 91.8, 90.0, 91.0, 520)
  for (let i = 0; i < 5; i++) {
    const open = 91.2 + i * 0.35
    const close = open + 0.2
    push(open, open + 0.3, open - 0.2, close, 520)
  }
  // C: drift down to 91.
  for (let i = 0; i < 10; i++) {
    const open = 93.5 - i * 0.25
    const close = open - 0.15
    push(open, open + 0.2, close - 0.25, close, 500)
  }
  // D: rally to 96.
  for (let i = 0; i < 5; i++) {
    const open = 91.2 + i * 1.1
    const close = open + 0.7
    push(open, open + 0.4, open - 0.2, close, 560)
  }
  // E: selling exhaustion and its quiet retest.
  push(90.6, 91.4, 89.8, 91.2, 2400)
  push(90.9, 91.6, 90.1, 91.4, 400)
  // F: rally to the resistance area.
  for (let i = 0; i < 13; i++) {
    const open = 91.5 + i * 0.7
    const close = open + 0.5
    push(open, open + 0.4, open - 0.2, close, 600)
  }
  // G: pull back so the swing high is confirmed.
  for (let i = 0; i < 5; i++) {
    const open = 100.2 - i * 0.45
    const close = open - 0.3
    push(open, open + 0.2, close - 0.2, close, 560)
  }
  // H: two quiet bars between the swing high and the climax.
  push(99.2, 99.4, 99.0, 99.1, 520)
  push(99.1, 99.3, 98.9, 99.0, 520)
  // I: buying climax, its quiet retest, and a no-demand bar.
  push(100.2, 101.0, 99.5, 100.0, 2600)
  push(100.0, 100.6, 99.7, 99.8, 420)
  push(102.3, 102.4, 100.2, 101.0, 300)
  // J: sign of strength and its pullback.
  push(100.9, 102.2, 100.8, 102.0, 2000)
  push(101.1, 101.5, 101.0, 101.4, 400)
  // K: decline back to the support area.
  for (let i = 0; i < 10; i++) {
    const open = 101.2 - i * 1.1
    const close = open - 0.8
    push(open, open + 0.2, close - 0.2, close, 560)
  }
  // L: sign of weakness, its pullback, and a no-supply bar.
  push(89.9, 90.0, 88.7, 88.8, 2000)
  push(89.6, 89.8, 89.3, 89.4, 400)
  push(90.4, 91.0, 90.2, 90.9, 300)
  // M: quiet drift so the ATR decays.
  for (let i = 0; i < 25; i++) {
    const open = 90.9 + i * 0.02
    const close = open + 0.02
    push(open, open + 0.06, open - 0.04, close, 500)
  }
  // N: a bar close to the support.
  push(90.05, 90.1, 90.0, 90.08, 300)
  // O: trailing bars.
  for (let i = 0; i < 13; i++) {
    const open = 90.08 + i * 0.02
    const close = open + 0.02
    push(open, open + 0.06, open - 0.04, close, 500)
  }
  return bars
}

const vsaScenarios = {
  defaults: {},
  scenario: { pivLen: 5, lvFresh: 120 },
  wyckoffSpring: {
    pivLen: 5,
    lvFresh: 120,
    signalMode: 'wyckoff',
    wyckoffEvent: 'spring',
  },
  wyckoffSos: {
    pivLen: 5,
    lvFresh: 120,
    signalMode: 'wyckoff',
    wyckoffEvent: 'sos',
  },
  wyckoffLps: {
    pivLen: 5,
    lvFresh: 120,
    signalMode: 'wyckoff',
    wyckoffEvent: 'lps',
  },
  limitPivot: {
    pivLen: 5,
    lvFresh: 120,
    entryMode: 'limit',
    tpMode: 'pivot',
  },
  session: {
    pivLen: 5,
    lvFresh: 120,
    sessOn: true,
    sess: '0730-0830',
    tz: 'Asia/Ho_Chi_Minh',
  },
}

const round8 = (value) =>
  typeof value === 'number' ? Math.round(value * 1e8) / 1e8 : value

const vsaVectors = {
  source: 'engine/methods/vsa.mjs',
  bars: vsaBars(),
  scenarios: Object.fromEntries(
    Object.entries(vsaScenarios).map(([name, options]) => {
      const result = vsa.analyze(vsaBars(), options)
      return [
        name,
        {
          options,
          events: result.events,
          setups: result.setups,
          scores: result.scores,
          flags: result.flags,
          series: {
            bucket: result.series.bucket,
            volumeAverage: result.series.volMA.map(round8),
            volumeRatio: result.series.ratio.map(round8),
            isPurple: result.series.isPurple,
            quietSetup: result.series.quietST,
            quietLow: result.series.quietLow,
            eventOk: result.series.evtOk,
            sessionOk: result.series.sessOk,
            nearSupport: result.series.nearSup,
            nearResistance: result.series.nearRes,
            svFresh: result.series.svFresh,
            bcFresh: result.series.bcFresh,
            spring: result.series.spring,
            utad: result.series.utad,
            sos: result.series.sos,
            sow: result.series.sow,
            lps: result.series.lps,
            lpsy: result.series.lpsy,
            purpleCount: result.series.pur20.map(round8),
            effortRun: result.series.eomRun,
            lastEvent: result.series.lastEvt,
            feeR: result.series.feeR.map(round8),
          },
        },
      ]
    }),
  ),
}

writeFileSync(
  join(here, 'vsa_vectors.json'),
  `${JSON.stringify(vsaVectors, null, 1)}\n`,
)
console.log(
  'wrote ta_vectors.json, indicator_vectors.json and vsa_vectors.json from',
  legacyDir,
)
