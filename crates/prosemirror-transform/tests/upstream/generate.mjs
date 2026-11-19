// Regenerate with Node 22.15+ from the repository root:
// node crates/prosemirror-transform/tests/upstream/generate.mjs node_modules/.prosemirror-upstream/transform-1.12.7
// The source checkout and installed package must be prosemirror-transform 1.12.7.
// An optional second argument points to a directory with isolated node_modules.
import assert from "node:assert/strict";
import {readFileSync, writeFileSync} from "node:fs";
import {registerHooks, stripTypeScriptTypes} from "node:module";
import {resolve} from "node:path";
import {fileURLToPath, pathToFileURL} from "node:url";
const [, , upstream, packageRoot] = process.argv;
const version = "1.12.7";
assert.ok(upstream, `Pass a local prosemirror-transform ${version} checkout`);
assert.equal(JSON.parse(readFileSync(resolve(upstream, "package.json"), "utf8")).version, version);
const testsUrl = pathToFileURL(resolve(upstream, "test")).href + "/";
function moduleUrl(name) {
  if (!packageRoot) return import.meta.resolve(name);
  const packageDir = resolve(packageRoot, "node_modules", name);
  const spec = JSON.parse(readFileSync(resolve(packageDir, "package.json"), "utf8"));
  return pathToFileURL(resolve(packageDir, spec.module)).href;
}
const transformUrl = moduleUrl("prosemirror-transform");
assert.equal(JSON.parse(readFileSync(new URL("../package.json", transformUrl), "utf8")).version, version);
const transform = await import(transformUrl);
const {Fragment, Mark, MarkType, Node, NodeRange, NodeType, ResolvedPos, Slice} = await import(moduleUrl("prosemirror-model"));
const istUrl = "data:text/javascript," + encodeURIComponent(`
  import assert from "node:assert/strict";
  function ist(actual, expected, compare, message) {
    if (arguments.length === 1) assert.ok(actual);
    else if (typeof compare === "function") assert.ok(compare(actual, expected), message || String(actual) + " != " + String(expected));
    else if (compare === ">") assert.ok(actual > expected, message);
    else assert.equal(actual, expected, compare);
  }
  ist.throws = assert.throws;
  export default ist;
`);
const tracedExports = "data:text/javascript," + encodeURIComponent(`
  import * as original from ${JSON.stringify(transformUrl)};
  export * from ${JSON.stringify(transformUrl)};
  export function canSplit(...args) { return globalThis.traceQuery("canSplit", original.canSplit, args); }
  export function dropPoint(...args) { return globalThis.traceQuery("dropPoint", original.dropPoint, args); }
  export function findWrapping(...args) { return globalThis.traceQuery("findWrapping", original.findWrapping, args); }
  export function liftTarget(...args) { return globalThis.traceQuery("liftTarget", original.liftTarget, args); }
`);

const schemas = [];
const schemaIds = new WeakMap();
function schemaIndex(schema) {
  if (schemaIds.has(schema)) return schemaIds.get(schema);
  const info = {topNode: schema.topNodeType.name, nodes: [], marks: []};
  schema.spec.nodes.forEach((name, spec) => info.nodes.push([name, spec]));
  schema.spec.marks.forEach((name, spec) => info.marks.push([name, spec]));
  schemas.push(info);
  schemaIds.set(schema, schemas.length - 1);
  return schemas.length - 1;
}

function encode(value) {
  if (value == null) return null;
  if (value instanceof NodeRange) {
    const {start, end, depth, startIndex, endIndex} = value;
    return {start, end, depth, startIndex, endIndex};
  }
  if (value instanceof NodeType || value instanceof MarkType) return {name: value.name};
  if (value instanceof Node || value instanceof Mark || value instanceof Slice
      || value instanceof Fragment || value instanceof transform.Step) return value.toJSON();
  if (Array.isArray(value)) return value.map(encode);
  if (typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, item]) => [key, encode(item)]));
  }
  return value;
}

const cases = [];
let current;
let suspended = 0;
let source;
const path = [];
globalThis.describe = (title, run) => {
  path.push(title);
  try { run(); } finally { path.pop(); }
};
globalThis.it = (title, run) => {
  current = {source, title: [...path, title].join(" / "), transforms: new Map(), queries: []};
  try {
    run();
    cases.push({
      source,
      title: current.title,
      queries: current.queries,
      transforms: [...current.transforms].map(([tr, trace]) => ({
        ...trace,
        expected: trace.expected ?? tr.doc.toJSON(),
        steps: tr.steps.map(step => step.toJSON()),
        maps: tr.mapping.maps.map(map => map.ranges),
      }))
    });
  } catch (error) {
    throw new Error(`${source}: ${current.title}`, {cause: error});
  } finally {
    current = undefined;
  }
};

function trace(tr) {
  let trace = current.transforms.get(tr);
  if (!trace) {
    trace = {schema: schemaIndex(tr.doc.type.schema), start: tr.before.toJSON(), operations: []};
    current.transforms.set(tr, trace);
  }
  return trace;
}

const methods = [
  "addMark", "addNodeMark", "changedRange", "delete", "deleteRange", "insert", "join", "lift", "maybeStep",
  "removeMark", "removeNodeMark", "replace", "replaceRange", "replaceRangeWith",
  "setBlockType", "setDocAttribute", "setNodeAttribute", "setNodeMarkup", "split", "step", "wrap",
];
for (const method of methods) {
  const original = transform.Transform.prototype[method];
  transform.Transform.prototype[method] = function (...args) {
    if (!current || suspended) return original.apply(this, args);
    const operations = trace(this).operations;
    const operation = {method, args: args.map(encode)};
    if (method === "setBlockType" && typeof args[3] === "function") {
      const attrs = args[3];
      operation.args[3] = null;
      operation.attrsByNode = [];
      args[3] = node => {
        const result = attrs(node);
        operation.attrsByNode.push([node.toJSON(), result]);
        return result;
      };
    }
    operations.push(operation);
    suspended++;
    try {
      const result = original.apply(this, args);
      if (method === "changedRange") operation.result = encode(result);
      if (method === "maybeStep") operation.result = !!result.doc;
      return result;
    } catch (error) {
      operation.error = error.message;
      throw error;
    } finally {
      suspended--;
    }
  };
}

function recordQuery(method, doc, args, expected) {
  current.queries.push({
    method, schema: schemaIndex(doc.type.schema), doc: doc.toJSON(), args, expected
  });
}

globalThis.traceQuery = (method, original, args) => {
  const result = original(...args);
  if (!current || suspended) return result;
  if (method === "canSplit" || method === "dropPoint") recordQuery(method, args[0], args.slice(1).map(encode), result ?? null);
  else {
    const [range, ...rest] = args;
    const expected = method === "findWrapping" && result
      ? result.map(({type, attrs}) => type.create(attrs).toJSON())
      : result ?? null;
    recordQuery(method, range.$from.doc, [encode(range), ...rest.map(encode)], expected);
  }
  return result;
};

const blockRange = ResolvedPos.prototype.blockRange;
ResolvedPos.prototype.blockRange = function (end, predicate) {
  const result = blockRange.call(this, end, predicate);
  if (current && !suspended && !predicate) {
    recordQuery("blockRange", this.doc, [this.pos, end?.pos ?? this.pos], encode(result));
  }
  return result;
};

globalThis.traceTestTransform = (tr, expected, run) => {
  const saved = trace(tr);
  saved.expected = expected.toJSON();
  saved.roundTrip = true;
  saved.positions = Object.entries(expected.tag ?? {}).map(([tag, value]) => [tr.before.tag[tag], value]);
  suspended++;
  try { return run(); } finally { suspended--; }
};

registerHooks({
  resolve(specifier, context, next) {
    if (!context.parentURL?.startsWith(testsUrl)) return next(specifier, context);
    if (specifier === "ist") return {url: istUrl, shortCircuit: true};
    if (specifier === "./trans.js") return next("./trans.ts", context);
    if (specifier === "prosemirror-transform") return {url: tracedExports, shortCircuit: true};
    if (!specifier.startsWith(".")) return next(moduleUrl(specifier), context);
    return next(specifier, context);
  },
  load(url, context, next) {
    if (!url.startsWith(testsUrl) || !url.endsWith(".ts")) return next(url, context);
    let input = readFileSync(fileURLToPath(url), "utf8")
      .replace(/import \{([^}]+)\} from "prosemirror-model"/g, (_, imports) => {
        const values = imports.split(",").map(name => name.trim())
          .filter(name => !["Attrs", "NodeSpec", "MarkSpec"].includes(name));
        return `import {${values.join(", ")}} from "prosemirror-model"`;
      });
    if (url.endsWith("/trans.ts")) {
      input = input.replace(
        "export function testTransform(tr: Transform, expect: Node) {",
        "function verifyTransform(tr: Transform, expect: Node) {",
      ) + '\nexport function testTransform(tr: Transform, expect: Node) {\n' +
        '  return globalThis.traceTestTransform(tr, expect, () => verifyTransform(tr, expect));\n}\n';
    }
    return {format: "module", source: stripTypeScriptTypes(input), shortCircuit: true};
  },
});

for (source of ["test-trans.ts", "test-structure.ts", "test-droppoint.ts"]) {
  await import(testsUrl + source);
}
assert.ok(cases.every(test => test.transforms.length || test.queries.length), "Each assertion needs a Rust case");
writeFileSync(new URL("cases.json", import.meta.url), `{
  "upstream": "ProseMirror/prosemirror-transform@${version}",
  "schemas": [
    ${schemas.map(schema => JSON.stringify(schema)).join(",\n    ")}
  ],
  "cases": [
    ${cases.map(test => JSON.stringify(test)).join(",\n    ")}
  ]
}\n`);
function testName(test) {
  return test.title.toLowerCase().replace(/[^a-z0-9]+/g, "_").replace(/_$/, "");
}
writeFileSync(new URL("coverage.rs", import.meta.url), [
  `// Generated by generate.mjs from upstream prosemirror-transform ${version}.`,
  "upstream_cases! {",
  ...cases.map((test, index) => `    ${testName(test)} => ${index},`),
  "}",
  "",
].join("\n"));
console.log(`Captured ${cases.length} upstream transform and structure tests.`);
