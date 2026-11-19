import assert from "node:assert/strict"
import { baseKeymap, selectAll, setBlockType, toggleMark, wrapIn } from "prosemirror-commands"
import { closeHistory, history, redo, undo, undoDepth } from "prosemirror-history"
import { ellipsis, emDash, inputRules, smartQuotes, undoInputRule } from "prosemirror-inputrules"
import { Fragment, Slice } from "prosemirror-model"
import { schema } from "prosemirror-schema-basic"
import { EditorState, NodeSelection, TextSelection } from "prosemirror-state"
import { Decoration, DecorationSet } from "prosemirror-view"

// The editor model is exercised directly. Clipboard interchange is represented
// by text and a Slice, and native IME/touch events by their model transactions.
const paragraph = (text = "") => schema.nodes.paragraph.create(null, text ? schema.text(text) : null)
const document = (...nodes) => schema.node("doc", null, nodes)
const text = text => ({ kind: "text", text })
const select = (anchor, head = anchor) => ({ kind: "select", anchor, head })
const key = (key, control = false, shift = false) => ({ kind: "key", key, control, shift })
const paste = text => ({ kind: "paste", text })
const clipboard = action => ({ kind: "clipboard", action })
const compose = (phase, text = "", from = 0, to = from) => ({ kind: "composition", phase, text, from, to })
const touch = (phase, pos, anchor, head) => ({ kind: "touch", phase, pos, anchor, head })

function normalizedTransaction(tr) {
  return {
    steps: tr.steps.map(step => step.toJSON()),
    selection: tr.selection.toJSON(),
    selectionSet: tr.selectionSet,
    composition: tr.getMeta("composition") ?? null,
    pointer: tr.getMeta("pointer") ?? null,
  }
}

function plainSlice(state, text) {
  if (!text) return null
  const context = state.selection.$from
  const normalized = text.replace(/\r\n?/g, "\n")
  if (context.parent.type.spec.code) return new Slice(Fragment.from(schema.text(normalized)), 0, 0)
  const marks = context.marks()
  const blocks = normalized.split(/\n+/).map(line =>
    schema.nodes.paragraph.create(null, line ? schema.text(line, marks) : null))
  return Slice.maxOpen(Fragment.fromArray(blocks), true)
}

function prepareReference(initial, selection, spans) {
  const rules = inputRules({ rules: [emDash, ellipsis, ...smartQuotes] })
  let state = EditorState.create({ doc: initial, selection, plugins: [rules, history({ depth: 1000 })] })
  let decorations = DecorationSet.create(initial, spans.map(span => {
    const spec = { id: span.id }
    if (span.kind === "widget") return Decoration.widget(span.from, () => null, { ...spec, side: span.side })
    if (span.kind === "node") return Decoration.node(span.from, span.to, { role: "review" }, spec)
    return Decoration.inline(span.from, span.to, { role: "review" }, { ...spec, inclusiveStart: true })
  }))
  let nativeClipboard = null
  let preedit = null
  let compositionId = 0
  let tick = 1000
  let transactions = []

  const dispatch = tr => {
    const applied = state.applyTransaction(closeHistory(tr.setTime(++tick)))
    for (const next of applied.transactions) {
      if (next.docChanged) decorations = decorations.map(next.mapping, next.doc)
      transactions.push(normalizedTransaction(next))
    }
    state = applied.state
  }
  const view = { get state() { return state }, dispatch, composing: false }

  function setSelection(selection, pointer = false) {
    if (selection.eq(state.selection)) return
    const tr = state.tr.setSelection(selection)
    if (pointer) tr.setMeta("pointer", true)
    dispatch(tr)
  }

  function keyDown(event) {
    if (event.control) {
      const command = {
        a: selectAll,
        b: toggleMark(schema.marks.strong),
        i: toggleMark(schema.marks.em),
        "0": setBlockType(schema.nodes.paragraph),
        "2": setBlockType(schema.nodes.heading, { level: 2 }),
        q: wrapIn(schema.nodes.blockquote),
        z: event.shift ? redo : undo,
      }[event.key]
      if (command?.(state, dispatch)) return
    }
    if (event.key === "Backspace" && undoInputRule(state, dispatch)) return
    baseKeymap[event.key]?.(state, dispatch)
  }

  function insertSlice(slice, plain = false) {
    if (slice) {
      const single = !slice.openStart && !slice.openEnd && slice.content.childCount === 1
      const tr = single ? state.tr.replaceSelectionWith(slice.content.firstChild, plain) : state.tr.replaceSelection(slice)
      dispatch(tr.setMeta("paste", true).setMeta("uiEvent", "paste"))
    }
  }

  function step(event) {
    transactions = []
    switch (event.kind) {
      case "text": {
        const { from, to } = state.selection
        if (!rules.props.handleTextInput(view, from, to, event.text)) dispatch(state.tr.insertText(event.text))
        break
      }
      case "select":
        setSelection(TextSelection.between(state.doc.resolve(event.anchor), state.doc.resolve(event.head), 1))
        break
      case "node_selection":
        setSelection(NodeSelection.create(state.doc, event.pos))
        break
      case "key":
        keyDown(event)
        break
      case "paste":
        insertSlice(plainSlice(state, event.text), true)
        break
      case "clipboard":
        if (event.action === "paste") {
          insertSlice(nativeClipboard?.slice)
        } else if (!state.selection.empty) {
          const slice = state.selection.content()
          nativeClipboard = { slice, text: slice.content.textBetween(0, slice.content.size, "\n\n") }
          if (event.action === "cut") dispatch(state.tr.deleteSelection().setMeta("uiEvent", "cut"))
        }
        break
      case "composition":
        if (event.phase === "start") {
          preedit = { from: state.selection.from, to: state.selection.to }
          compositionId++
        } else if (event.phase === "cancel") {
          preedit = null
        } else if (event.phase === "commit") {
          assert.ok(preedit, "commit must have an active preedit")
          dispatch(state.tr.insertText(event.text, preedit.from, preedit.to).setMeta("composition", compositionId))
          preedit = null
        }
        break
      case "touch":
        setSelection(TextSelection.between(state.doc.resolve(event.anchor), state.doc.resolve(event.head), 1), true)
        break
      default:
        throw new Error(`Unknown event ${event.kind}`)
    }
    state.doc.check()
    for (const range of state.selection.ranges) {
      assert.equal(range.$from.doc, state.doc)
      assert.equal(range.$to.doc, state.doc)
    }
    const snapshot = {
      doc: state.doc.toJSON(),
      selection: state.selection.toJSON(),
      storedMarks: state.storedMarks?.map(mark => mark.toJSON()) ?? null,
      decorations: decorations.find().map(span => [span.spec.id, span.from, span.to])
        .sort((a, b) => a[0] - b[0] || a[1] - b[1] || a[2] - b[2]),
      clipboard: nativeClipboard?.text ?? null,
    }
    if (event.kind === "composition" || event.kind === "touch") snapshot.transactions = transactions
    return snapshot
  }

  return { get state() { return state }, step }
}

function replay(name, initial, events, spans = [], undoRestores = false) {
  const selection = TextSelection.create(initial, 1)
  const reference = prepareReference(initial, selection, spans)
  const steps = []
  const add = event => steps.push({ event, expected: reference.step(event) })
  for (const event of typeof events === "function" ? events(reference) : events) add(event)
  if (undoRestores) {
    while (undoDepth(reference.state)) add(key("z", true))
    assert.ok(reference.state.doc.eq(initial), `All undo operations must restore the initial document for ${name}`)
  }
  return { name, initial: initial.toJSON(), selection: selection.toJSON(), spans, steps, undoRestores }
}

const semantic = [
  replay("input rules and editing shortcuts", document(paragraph()), [
    text("Hello"), text("-"), text("-"), key("Backspace"), text(" "),
    text("."), text("."), text("."), text(" "), text('"'), text("quoted"), text('"'),
    select(1, 6), key("b", true), key("i", true), key("z", true), key("z", true, true),
  ]),
  replay("splitting, joining, block formatting, and plain paste", document(paragraph("alpha"), paragraph("beta")), [
    select(3), key("Enter"), key("Backspace"), select(3), key("Enter"),
    key("2", true), key("0", true), key("q", true),
    select(1), paste("one\r\n\r\ntwo"), key("z", true), key("z", true, true),
    key("a", true), text("new document"),
  ]),
  replay("node selection and clipboard round trip", document(schema.nodes.paragraph.create(null, [
    schema.text("before"),
    schema.nodes.image.create({ src: "sample.png", alt: "sample" }),
    schema.text("after"),
  ])), [
    { kind: "node_selection", pos: 7 }, clipboard("copy"),
    select(4), clipboard("paste"), select(1, 3), clipboard("cut"),
    key("z", true), key("z", true, true), select(1), clipboard("paste"),
  ]),
  replay("preedit and touch transactions", document(paragraph("first second")), [
    touch("long_press", 9, 7, 13), touch("drag_start", 13, 7, 13),
    touch("drag", 9, 7, 9), touch("drag_end", 6, 7, 6),
    select(1, 6), compose("start"), compose("update", "漢", 1),
    compose("update", "漢🙂", 3), compose("commit", "漢🙂"),
    compose("start"), compose("update", "not committed", 2, 4), compose("cancel"),
    key("z", true),
  ]),
]

const seeds = [1, 7, 29, 42, 177, 2026, 3719, 65537]

function randomSource(seed) {
  return modulus => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0
    return seed % modulus
  }
}

function textPositions(doc) {
  const positions = []
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return
    const run = [pos + 1]
    node.forEach((child, offset) => {
      if (child.isText) {
        let end = pos + 1 + offset
        for (const character of child.text) {
          end += character.length
          run.push(end)
        }
      } else {
        run.push(pos + 1 + offset + child.nodeSize)
      }
    })
    positions.push(run)
    return false
  })
  return positions
}

function randomEvent(state, random) {
  const runs = textPositions(state.doc)
  assert.ok(runs.length)
  const run = runs[random(runs.length)]
  const cursor = run[random(run.length)]
  switch (random(11)) {
    case 0: return text(["x", " Ω", "🙂", "e\u0301", "中", "-"][random(6)])
    case 1: return select(cursor)
    case 2: return select(cursor, run[random(run.length)])
    case 3: return runs.length < 10 ? key("Enter") : select(cursor)
    case 4: return state.selection.empty ? select(cursor) : key("Backspace")
    case 5: return key("b", true)
    case 6: return key("i", true)
    case 7: return paste(["copy", "a\nb", "🧪", "longer"][random(4)])
    case 8: return key("z", true)
    case 9: return key("z", true, true)
    default: return key(random(2) ? "0" : "2", true)
  }
}

const random = seeds.map(seed => replay(`seed ${seed}`, document(paragraph("alpha beta"), paragraph("gamma delta")),
  function* (reference) {
    const next = randomSource(seed)
    for (let index = 0; index < 48; index++) yield randomEvent(reference.state, next)
  }, [
    { kind: "inline", id: 1, from: 1, to: 6 },
    { kind: "widget", id: 2, from: 4, side: -1 },
    { kind: "widget", id: 3, from: 12, side: 1 },
    { kind: "node", id: 4, from: 12, to: 25 },
  ], true))

process.stdout.write(JSON.stringify([...semantic, ...random]))
