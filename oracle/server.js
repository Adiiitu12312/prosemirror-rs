import fs from 'node:fs';
import { schema } from 'prosemirror-schema-basic';
import { Step } from 'prosemirror-transform';

// Reads a JSON request from stdin: { doc: {...}, step: {...} }
function main() {
  const input = fs.readFileSync(0, 'utf-8');
  if (!input || !input.trim()) {
    process.exit(0);
  }

  try {
    const { doc: docJson, step: stepJson } = JSON.parse(input);
    const doc = schema.nodeFromJSON(docJson);
    const step = Step.fromJSON(schema, stepJson);

    const result = step.apply(doc);

    if (result.doc) {
      const stepMap = step.getMap();
      const output = {
        success: true,
        doc: result.doc.toJSON(),
        step_map: stepMap.ranges,
      };
      process.stdout.write(JSON.stringify(output));
    } else {
      const output = {
        success: false,
        error: result.failed || 'Step failed to apply',
      };
      process.stdout.write(JSON.stringify(output));
    }
  } catch (err) {
    const output = {
      success: false,
      error: err.message,
    };
    process.stdout.write(JSON.stringify(output));
  }
}

main();
