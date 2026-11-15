import fs from 'node:fs';
import { schema } from 'prosemirror-schema-basic';
import { Transform, Step } from 'prosemirror-transform';
import { EditorState, TextSelection } from 'prosemirror-state';
import { history, undo, redo } from 'prosemirror-history';
import { deleteSelection, splitBlock, toggleMark } from 'prosemirror-commands';

console.log('Generating golden test fixtures from real ProseMirror...');

const fixtures = {
  version: 'prosemirror-pin-1.0',
  model: [],
  transform: [],
  state: [],
  history: [],
  commands: [],
};

// 1. Model Fixtures
{
  const doc = schema.node('doc', null, [
    schema.node('paragraph', null, [
      schema.text('Hello '),
      schema.text('world', [schema.marks.em.create()]),
      schema.text('!'),
    ]),
  ]);
  fixtures.model.push({
    name: 'paragraph_with_em_mark',
    doc: doc.toJSON(),
    text_between_0_12: doc.textBetween(0, 12),
    slice_1_7: doc.slice(1, 7).toJSON(),
  });
}

// 2. Transform Fixtures
{
  const doc = schema.node('doc', null, [
    schema.node('paragraph', null, [schema.text('The quick brown fox')]),
  ]);
  const tr = new Transform(doc);
  tr.replaceWith(5, 10, schema.text('slow'));
  tr.addMark(1, 4, schema.marks.strong.create());

  fixtures.transform.push({
    name: 'replace_and_mark',
    initial_doc: doc.toJSON(),
    final_doc: tr.doc.toJSON(),
    steps: tr.steps.map((s) => s.toJSON()),
    step_maps: tr.mapping.maps.map((m) => m.ranges),
    mapped_pos_6: tr.mapping.map(6),
  });
}

// 3. State & Selection Fixtures
{
  const doc = schema.node('doc', null, [
    schema.node('paragraph', null, [schema.text('Selectable text')]),
  ]);
  let state = EditorState.create({ schema, doc });
  const sel = TextSelection.create(state.doc, 1, 6);
  let tr = state.tr.setSelection(sel);
  tr = tr.insertText('Changed ', 1, 6);
  const nextState = state.apply(tr);

  fixtures.state.push({
    name: 'selection_and_insert',
    initial_doc: doc.toJSON(),
    next_doc: nextState.doc.toJSON(),
    selection_after: {
      from: nextState.selection.from,
      to: nextState.selection.to,
    },
  });
}

// 4. History (Undo / Redo) Fixtures
{
  const doc = schema.node('doc', null, [
    schema.node('paragraph', null, [schema.text('Initial')]),
  ]);
  let state = EditorState.create({
    schema,
    doc,
    plugins: [history()],
  });

  let tr = state.tr.insertText(' edit', 8);
  state = state.apply(tr);
  const docAfterEdit = state.doc.toJSON();

  undo(state, (t) => {
    state = state.apply(t);
  });
  const docAfterUndo = state.doc.toJSON();

  redo(state, (t) => {
    state = state.apply(t);
  });
  const docAfterRedo = state.doc.toJSON();

  fixtures.history.push({
    name: 'basic_undo_redo',
    initial_doc: doc.toJSON(),
    after_edit: docAfterEdit,
    after_undo: docAfterUndo,
    after_redo: docAfterRedo,
  });
}

// 5. Commands Fixtures
{
  const doc = schema.node('doc', null, [
    schema.node('paragraph', null, [schema.text('Line 1')]),
    schema.node('paragraph', null, [schema.text('Line 2')]),
  ]);
  let state = EditorState.create({
    schema,
    doc,
    selection: TextSelection.create(doc, 4),
  });

  splitBlock(state, (t) => {
    state = state.apply(t);
  });

  fixtures.commands.push({
    name: 'split_block',
    initial_doc: doc.toJSON(),
    doc_after_split: state.doc.toJSON(),
  });
}

fs.writeFileSync(
  'fixtures/golden_fixtures.json',
  JSON.stringify(fixtures, null, 2),
);
console.log('SUCCESS: Wrote fixtures to fixtures/golden_fixtures.json');
