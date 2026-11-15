import { schema } from 'prosemirror-schema-basic';
import { Step } from 'prosemirror-transform';
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
console.log('Oracle Response:', parsed);

if (!parsed.success) {
  console.error('FAILED: Oracle reported error', parsed.error);
  process.exit(1);
}

const firstText = parsed.doc.content[0].content[0].text;
if (firstText !== 'Hi world') {
  console.error(`FAILED: Expected "Hi world", got "${firstText}"`);
  process.exit(1);
}

console.log('SUCCESS: Oracle correctly applied step and matched expected output!');
