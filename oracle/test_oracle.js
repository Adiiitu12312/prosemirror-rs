import assert from 'node:assert/strict';
import { execSync } from 'node:child_process';

console.log('Testing Node ProseMirror Oracle...');

const sampleDoc = {
  type: 'doc',
  content: [
    {
      type: 'paragraph',
      content: [{ type: 'text', text: 'Hello world' }],
    },
  ],
};

const sampleStep = {
  stepType: 'replace',
  from: 1,
  to: 6,
  slice: {
    content: [{ type: 'text', text: 'Hi' }],
  },
};

const input = JSON.stringify({ doc: sampleDoc, step: sampleStep });

const output = execSync('node oracle/server.js', {
  input,
  encoding: 'utf-8',
});

const parsed = JSON.parse(output);
assert.equal(parsed.success, true, parsed.error);
assert.equal(parsed.doc.content[0].content[0].text, 'Hi world');
assert.deepEqual(parsed.step_map, [1, 5, 2]);
assert.deepEqual(parsed.positions[3], [1, true, false, true, false]);
assert.deepEqual(parsed.undone, sampleDoc);

const rejected = JSON.parse(execSync('node oracle/server.js', {
  input: JSON.stringify({ doc: sampleDoc, step: { stepType: 'replace', from: 0, to: 13 } }),
  encoding: 'utf-8',
}));
assert.equal(rejected.success, false);
assert.match(rejected.error, /Invalid content for node doc/);

console.log('SUCCESS: Oracle correctly applied step and matched expected output!');
