import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { execFileSync, spawnSync } from 'node:child_process';
import { test } from 'node:test';
import { extract, generate, inventory, setupHook, runHook } from './codemap.mjs';

const home = import.meta.dirname;
const root = path.join(home, '.codemap-fixture');
const git = (...args) => execFileSync('git', args, { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'pipe'] });
const write = (file, text) => { fs.mkdirSync(path.dirname(path.join(root, file)), { recursive: true }); fs.writeFileSync(path.join(root, file), text); };

 test('TS/TSX arrow exports, methods, interfaces, aliases and nested bindings', async () => {
  const result = await extract('sample.tsx', 'export const View = () => <div/>; export class Box { value = 1; constructor() {} get size() { return 1; } run() {} } interface I { run(): void; field: string } type Alias = I; export { Box as Renamed }; const { a, b: renamed } = value;');
  assert.deepEqual(result.diagnostics, []);
  for (const name of ['View', 'Box', 'value', 'constructor', 'size', 'run', 'I', 'field', 'Alias', 'Renamed', 'a', 'renamed']) assert.ok(result.symbols.some(s => s.name === name), name);
  assert.ok(result.symbols.some(s => s.name === 'run' && s.scope === 'Box'));
});

test('Rust impl/trait fields, associated items, macros and independent include fragments', async () => {
  const result = await extract('sample.rs', 'pub struct Box { pub value: u32 }\npub enum E { First, Second(u32) }\ntrait T { type Item; fn run(&self); }\nimpl T for Box { type Item = u32; fn run(&self) {} }\nmacro_rules! make { () => { fn generated() {} }; }\nmake!(); include!("fragment.rs");');
  assert.deepEqual(result.diagnostics, []);
  for (const name of ['Box', 'value', 'E', 'First', 'Second', 'T', 'Item', 'run', 'make', 'include']) assert.ok(result.symbols.some(s => s.name === name), name);
  assert.ok(result.symbols.some(s => s.name === 'run' && s.scope === 'T for Box'));
  assert.ok(!result.symbols.some(s => s.name === 'generated'));
  const attributes = await extract('lib.rs', '//! Library\n#![cfg_attr(\n all(not(debug_assertions), target_os = "windows"),\n windows_subsystem = "windows"\n)]\nfn entry() {}');
  assert.deepEqual(attributes.diagnostics, []);
  assert.ok(attributes.symbols.some(s => s.name === 'entry' && s.line === 6));
  const fragment = await extract('fragment.rs', '#[derive(Clone)]\nstruct Fragment;\nfn private_helper() {}');
  assert.ok(fragment.symbols.some(s => s.name === 'Fragment' && s.line === 2));
  assert.ok(fragment.symbols.some(s => s.name === 'private_helper'));
});

test('Python, shell, PowerShell and BAT file navigation', async () => {
  for (const [file, text, name] of [['a.py', 'class Thing:\n    def run(self):\n        pass\n', 'run'], ['a.sh', 'hello() { echo hi; }', 'hello'], ['a.ps1', 'function Invoke-Thing { }', 'Invoke-Thing'], ['a.bat', ':build\necho hi', 'build']]) {
    assert.ok((await extract(file, text)).symbols.some(s => s.name === name));
  }
});

test('incremental lifecycle, drift, exclusions, paging, links, and real isolated pre-commit execution', async () => {
  assert.ok(!fs.existsSync(root), 'fixture path must not overwrite existing files');
  fs.mkdirSync(root);
  try {
    git('init', '--quiet');
    git('config', 'user.name', 'Code Map Test');
    git('config', 'user.email', 'codemap@example.invalid');
    git('config', 'commit.gpgsign', 'false');
    git('config', 'core.autocrlf', 'false');
    const config = JSON.parse(fs.readFileSync(path.join(home, 'codemap.config.json')));
    config.pageRows = 2;
    write('codemap.config.json', JSON.stringify(config));
    write('.gitignore', '.codemap-cache.json\nnode_modules/\n.env\n');
    write('codemap.mjs', "import {runHook} from '../codemap.mjs'; await runHook(process.cwd());\n");
    write('src/a.ts', 'export const A = 1; export const B = 2; export const C = 3;\n');
    write('src-tauri/src/gateway/mod.rs', 'include!("part.rs");\n');
    write('src-tauri/src/gateway/part.rs', 'fn locate_bug() {}\n');
    for (const file of ['examples/skip.ts', 'src/types/generated/skip.ts', 'src-tauri/target/skip.rs', 'node_modules/skip.ts']) write(file, 'SHOULD_NOT_APPEAR');
    write('.env', 'SECRET=not-real');
    const first = await generate(root);
    assert.equal(first.files, 4);
    assert.equal(first.parsed, 4);
    assert.equal((await generate(root)).changed, 0);
    assert.equal((await generate(root)).parsed, 0);
    await generate(root, { check: true });
    const rustPage = fs.readdirSync(path.join(root, 'docs/codemap')).find(f => f.startsWith('src-tauri_src_gateway_part.rs'));
    const rustTime = fs.statSync(path.join(root, 'docs/codemap', rustPage)).mtimeMs;
    write('src/a.ts', 'export const A = 2; export const B = 2; export const C = 3;\n');
    await assert.rejects(generate(root, { check: true }), /Stale/);
    const update = await generate(root, { module: 'web-root' });
    assert.equal(update.parsed, 1);
    assert.equal(fs.statSync(path.join(root, 'docs/codemap', rustPage)).mtimeMs, rustTime);
    write('src/new.ts', 'export function Added() {}\n');
    assert.equal((await generate(root, { module: 'web-root' })).parsed, 1);
    fs.renameSync(path.join(root, 'src/new.ts'), path.join(root, 'src/renamed.ts'));
    const renamed = await generate(root, { module: 'web-root' });
    assert.equal(renamed.parsed, 1);
    assert.equal(renamed.deleted, 1);
    fs.unlinkSync(path.join(root, 'src/renamed.ts'));
    assert.equal((await generate(root, { module: 'web-root' })).deleted, 1);
    await assert.rejects(generate(root, { module: 'missing' }), /Unknown module/);
    for (const file of fs.readdirSync(path.join(root, 'docs/codemap'))) {
      const text = fs.readFileSync(path.join(root, 'docs/codemap', file), 'utf8');
      assert.ok(text.split('\n').filter(line => line.startsWith('|')).length <= 4, file);
      assert.ok(!text.includes('SHOULD_NOT_APPEAR'));
      for (const match of text.matchAll(/\]\(([^)]+)\)/g)) assert.ok(fs.existsSync(path.resolve(root, 'docs/codemap', decodeURIComponent(match[1].split('#')[0]))), match[1]);
    }
    write('docs/codemap/obsolete.md', 'old');
    await assert.rejects(generate(root, { check: true }), /Stale/);
    assert.equal((await generate(root)).deleted, 1);
    assert.deepEqual(inventory(root, config).filter(file => file.includes('skip')), []);
    setupHook(root);
    setupHook(root);
    git('add', '.');
    git('commit', '--quiet', '-m', 'fixture baseline');
    const head = git('rev-parse', 'HEAD');
    write('src/a.ts', 'export const Changed = 9;\n');
    git('add', 'src/a.ts');
    const failed = spawnSync('git', ['commit', '--quiet', '-m', 'must block'], { cwd: root, encoding: 'utf8' });
    assert.notEqual(failed.status, 0);
    assert.match(failed.stderr, /Code map refreshed/);
    assert.equal(git('rev-parse', 'HEAD'), head);
    assert.ok(git('diff', '--name-only').includes('docs/codemap/'));
    assert.equal(git('diff', '--cached', '--name-only').trim(), 'src/a.ts');
    git('add', 'CODE_MAP_INDEX.md', 'docs/codemap');
    await runHook(root);
    // Even with maps staged, partial staging and unstaged deletion must block.
    write('src/a.ts', 'export const Partial = 10;\n');
    await assert.rejects(runHook(root), /Nothing was auto-staged/);
    await generate(root);
    git('add', 'src/a.ts', 'CODE_MAP_INDEX.md', 'docs/codemap');
    fs.unlinkSync(path.join(root, 'src/a.ts'));
    await assert.rejects(runHook(root), /Nothing was auto-staged/);
    write('src/a.ts', 'export const Partial = 10;\n');
    await generate(root);
    git('add', 'src/a.ts', 'CODE_MAP_INDEX.md', 'docs/codemap');
    git('commit', '--quiet', '-m', 'fixture regenerated');
    await generate(root, { check: true });
    write('.git/hooks/pre-commit', '#!/bin/sh\necho existing\n');
    assert.throws(() => setupHook(root), /Existing pre-commit/);
    assert.match(fs.readFileSync(path.join(root, '.git/hooks/pre-commit'), 'utf8'), /existing/);
    git('config', 'core.hooksPath', 'other-hooks');
    assert.throws(() => setupHook(root), /core.hooksPath/);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});
