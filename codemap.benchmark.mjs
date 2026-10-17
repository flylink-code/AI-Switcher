import fs from 'node:fs';
import path from 'node:path';
import assert from 'node:assert/strict';
import { performance } from 'node:perf_hooks';
import { inventory } from './codemap.mjs';

// Reproducible navigation scenario: investigate Codex login preservation during provider switching.
const root = import.meta.dirname;
const symbol = 'apply_auth_strategy';
const config = JSON.parse(fs.readFileSync(path.join(root, 'codemap.config.json')));
const sourceFiles = inventory(root, config);
const measure = action => {
  const opened = new Set();
  let bytes = 0;
  const read = file => {
    const data = fs.readFileSync(path.join(root, file));
    opened.add(file);
    bytes += data.length;
    return data.toString('utf8');
  };
  const start = performance.now();
  const found = action(read);
  return { ...found, files: opened.size, bytes, milliseconds: +(performance.now() - start).toFixed(3) };
};
const indexed = measure(read => {
  const index = read('CODE_MAP_INDEX.md');
  assert.ok(index.includes('rust-config'));
  const module = read('docs/codemap/rust-config.md');
  const row = module.split('\n').find(line => line.includes('[src-tauri/src/config/codex.rs]'));
  assert.ok(row);
  let location;
  for (const match of row.matchAll(/\[\d+\]\(([^)]+)\)/g)) {
    const page = read(`docs/codemap/${match[1]}`);
    const declaration = page.split('\n').find(line => line.startsWith(`| ${symbol} |`));
    if (declaration) { location = declaration; break; }
  }
  assert.ok(location);
  const file = 'src-tauri/src/config/codex.rs';
  const line = Number(/#L(\d+)/.exec(location)[1]);
  const source = read(file);
  assert.ok(source.split('\n')[line - 1].includes(symbol));
  return { file, line };
});
const scan = measure(read => {
  let result;
  // The comparison is deliberately bounded to the same backend configuration module.
  for (const file of sourceFiles.filter(file => file.startsWith('src-tauri/src/config/'))) {
    const text = read(file);
    const line = text.split('\n').findIndex(row => row.includes(`fn ${symbol}(`));
    if (line >= 0) result = { file, line: line + 1 };
  }
  assert.ok(result);
  return result;
});
assert.equal(indexed.file, scan.file);
assert.equal(indexed.line, scan.line);
let pageCount = 0;
let maxRows = 0;
let mapBytes = 0;
for (const file of fs.readdirSync(path.join(root, 'docs/codemap'))) {
  const data = fs.readFileSync(path.join(root, 'docs/codemap', file));
  const text = data.toString('utf8');
  const rows = text.split('\n').filter(line => line.startsWith('|')).length - 2;
  assert.ok(rows <= config.pageRows, file);
  maxRows = Math.max(maxRows, rows);
  mapBytes += data.length;
  pageCount++;
  for (const match of text.matchAll(/\]\(([^)]+)\)/g)) {
    const target = path.resolve(root, 'docs/codemap', decodeURIComponent(match[1].split('#')[0]));
    assert.ok(fs.existsSync(target), `${file}: ${match[1]}`);
  }
}
console.log(JSON.stringify({ scenario: 'Codex provider switching login preservation', symbol, indexed, boundedModuleScan: scan, pageCount, maxRows, mapBytes, note: 'Wall clock is a single warm-filesystem observation, not a speed guarantee. Bytes count entire files actually read, including final source; map construction is excluded.' }, null, 2));
