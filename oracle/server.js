import fs from 'node:fs';
import { schema } from 'prosemirror-schema-basic';
import { Step } from 'prosemirror-transform';

function applyStep(step, doc) {
  try {
    return step.apply(doc);
  } catch (error) {
    if (error instanceof RangeError) return { doc: null, failed: error.message };
    throw error;
  }
}

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

    const result = applyStep(step, doc);

    if (result.doc) {
      const stepMap = step.getMap();
      const positions = [];
      for (let pos = 0; pos <= doc.content.size; pos++) {
        for (const assoc of [-1, 1]) {
          const { pos: mapped, deleted, deletedBefore, deletedAfter, deletedAcross } =
            stepMap.mapResult(pos, assoc);
          positions.push([mapped, deleted, deletedBefore, deletedAfter, deletedAcross]);
        }
      }
      const inverse = step.invert(doc);
      const undone = applyStep(inverse, result.doc);
      const output = {
        success: true,
        doc: result.doc.toJSON(),
        step_map: stepMap.ranges,
        positions,
        inverse: inverse.toJSON(),
        undone: undone.doc?.toJSON() ?? null,
      };
      process.stdout.write(JSON.stringify(output));
    } else {
      const output = {
        success: false,
        error: result.failed || 'Step failed to apply',
      };
      process.stdout.write(JSON.stringify(output));
    }
  } catch (error) {
    console.error(error);
    process.exitCode = 1;
  }
}

main();
