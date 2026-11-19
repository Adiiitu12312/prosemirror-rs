import { readFileSync } from "node:fs"

const trialCount = 3
const reference = JSON.parse(readFileSync(new URL("editor-reference.json", import.meta.url), "utf8"))
const measurementPath = process.argv[2]
if (!measurementPath) throw new Error("Usage: node benchmarks/check-editor.mjs MEASUREMENTS.jsonl")

const key = ({ host, blocks, operation }) => `${host}/${blocks}/${operation}`
const operationCounts = new Map([
  ["open", 1], ["type_1000_characters", 1000], ["paste_500_blocks", 1],
  ["undo_200", 200], ["redo_200", 200],
])
const results = new Map()
for (const line of readFileSync(measurementPath, "utf8").trim().split(/\r?\n/)) {
  const row = JSON.parse(line)
  const name = key(row)
  if (row.count !== operationCounts.get(row.operation) || !Number.isFinite(row.mean_ms) || row.mean_ms <= 0) {
    throw new Error(`Invalid benchmark result for ${name}`)
  }
  if (!results.has(name)) results.set(name, [])
  results.get(name).push(row.mean_ms)
}

let failed = false
for (const row of reference.measurements) {
  const name = key(row)
  const trials = results.get(name)
  if (trials?.length !== trialCount) throw new Error(`Expected ${trialCount} trials for ${name}`)
  results.delete(name)
  trials.sort((a, b) => a - b)
  const median = trials[1]
  const limit = Math.min(row.baseline_ms * 1.2, row.target_ms ?? Infinity)
  const exceeded = median > limit
  failed ||= exceeded
  const outcome = exceeded ? "FAIL" : "PASS"
  console.log(`${outcome} ${name} median ${median.toFixed(3)} ms, limit ${limit.toFixed(3)} ms`)
}

if (results.size !== 0) throw new Error(`Unexpected benchmark results: ${[...results.keys()].join(", ")}`)
if (failed) process.exitCode = 1
