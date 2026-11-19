import assert from "node:assert/strict"
import { readFileSync } from "node:fs"
import { collab, getVersion, receiveTransaction, sendableSteps } from "prosemirror-collab"
import { history, redo, undo } from "prosemirror-history"
import { schema } from "prosemirror-schema-basic"
import { EditorState } from "prosemirror-state"
import { Mapping, Step, StepMap, Transform } from "prosemirror-transform"

const expected = JSON.parse(readFileSync(new URL("./parity-exceptions.json", import.meta.url), "utf8"))

// Transform 1.12.7 src/map.ts, Mapping.appendMappingInverted.
function sliced_mapping_inverse_mirror() {
  const mapping = new Mapping([[2, 4, 0], [2, 0, 4], [0, 0, 1], [1, 0, 1]].map(range => new StepMap(range)))
  mapping.setMirror(0, 1)
  const slice = mapping.slice(0, 2)
  const inverse = slice.invert()
  const forward = slice.map(4, 1)
  return {
    mapCount: inverse.maps.length,
    mirror: [inverse.getMirror(0) ?? null, inverse.getMirror(1) ?? null],
    forward,
    roundTrip: inverse.map(forward, 1),
  }
}

function markOperations(step) {
  return step.stepType === "markSet"
    ? step.steps.map(markOperations)
    : [step.stepType, step.from, step.to, step.mark.attrs.href]
}

// Transform 1.12.7 src/mark_step.ts, MarkSetStep.invert and MarkSetStep.fromJSON.
function mark_set_inverse_json() {
  const link = href => schema.marks.link.create({ href })
  const before = schema.node("doc", null, [schema.node("paragraph", null, [schema.text("abc", [link("/old")])])])
  const transform = new Transform(before).addMark(1, 4, link("/new"))
  assert.equal(transform.steps.length, 1)
  const undoStep = transform.steps[0].invert(before)
  assert.ok(undoStep.apply(transform.doc).doc.eq(before))
  const redoStep = undoStep.invert(transform.doc)
  assert.ok(redoStep.apply(before).doc.eq(transform.doc))
  const wire = redoStep.toJSON()
  let decodeError
  assert.throws(() => Step.fromJSON(schema, wire), error => {
    assert.equal(error.name, "RangeError")
    decodeError = error.message
    return true
  })
  return { redo: markOperations(wire), decodeError }
}

// Collab 1.3.1 test/test-collab.ts, "supports redo", with the pinned History.
function collab_redo_selection() {
  const peers = [0, 1].map(clientID => EditorState.create({ schema, plugins: [history(), collab({ clientID })] }))
  const steps = [], clientIDs = []
  function sync(index) {
    const state = peers[index], version = getVersion(state)
    if (version !== steps.length) {
      peers[index] = state.apply(receiveTransaction(state, steps.slice(version), clientIDs.slice(version)))
    }
  }
  function dispatch(index, transaction) {
    peers[index] = peers[index].apply(transaction)
    sync(index)
    const sendable = sendableSteps(peers[index])
    assert.equal(sendable.version, steps.length)
    steps.push(...sendable.steps)
    clientIDs.push(...sendable.steps.map(() => sendable.clientID))
    sync(1 - index)
  }
  function type(index, text) {
    const state = peers[index]
    dispatch(index, state.tr.insertText(text, state.selection.head))
  }

  type(0, "A")
  type(1, "B")
  type(0, "C")
  assert.ok(undo(peers[1], transaction => dispatch(1, transaction)))
  const afterUndo = peers[1].doc.textContent
  assert.ok(redo(peers[1], transaction => dispatch(1, transaction)))
  const afterRedo = peers[1].doc.textContent
  const redoCursor = peers[1].selection.head
  type(1, "D")
  type(0, "E")
  const finalTexts = peers.map(peer => peer.doc.textContent)
  for (const text of finalTexts) assert.notEqual(text, expected.collab_redo_selection.originalFinalText)
  return { afterUndo, afterRedo, redoCursor, finalTexts }
}

const cases = new Map([
  ["sliced_mapping_inverse_mirror", sliced_mapping_inverse_mirror],
  ["mark_set_inverse_json", mark_set_inverse_json],
  ["collab_redo_selection", collab_redo_selection],
])
const names = process.argv.slice(2)
for (const name of names.length ? names : cases.keys()) {
  assert.ok(cases.has(name), `Unknown parity exception ${name}`)
  const observed = cases.get(name)()
  assert.deepEqual(observed, expected[name].upstream ?? expected[name].pinned, name)
  console.log(`${name} ${JSON.stringify(observed)}`)
}
