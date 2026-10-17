import fs from 'node:fs';
import path from 'node:path';
import { createHash } from 'node:crypto';
import { execFileSync } from 'node:child_process';
import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';
import ts from 'typescript';
import { Parser, Language } from 'web-tree-sitter';

const require = createRequire(import.meta.url);
const home = path.dirname(fileURLToPath(import.meta.url));
const hash = text => createHash('sha256').update(text).digest('hex');
const normalize = text => text.replace(/\r\n/g, '\n');
const compare = (a, b) => a < b ? -1 : a > b ? 1 : 0;
const grammars = new Map();
const rustKinds = new Set(['function_item', 'function_signature_item', 'struct_item', 'union_item', 'enum_item', 'enum_variant', 'trait_item', 'type_item', 'associated_type', 'const_item', 'static_item', 'mod_item', 'macro_definition', 'field_declaration']);

async function parserFor(language) {
  if (!grammars.has(language)) {
    await Parser.init();
    const parser = new Parser();
    parser.setLanguage(await Language.load(require.resolve(`tree-sitter-wasms/out/tree-sitter-${language}.wasm`)));
    grammars.set(language, parser);
  }
  return grammars.get(language);
}

export async function extract(file, text) {
  const symbols = [];
  const diagnostics = [];
  const add = (name, kind, line, scope = '') => symbols.push({ name, kind, line, scope });
  if (/\.(?:[cm]?js|tsx?)$/.test(file)) {
    const source = ts.createSourceFile(file, text, ts.ScriptTarget.Latest, true);
    const kinds = new Set([ts.SyntaxKind.FunctionDeclaration, ts.SyntaxKind.ClassDeclaration, ts.SyntaxKind.InterfaceDeclaration, ts.SyntaxKind.TypeAliasDeclaration, ts.SyntaxKind.EnumDeclaration, ts.SyntaxKind.EnumMember, ts.SyntaxKind.ModuleDeclaration, ts.SyntaxKind.MethodDeclaration, ts.SyntaxKind.MethodSignature, ts.SyntaxKind.PropertyDeclaration, ts.SyntaxKind.PropertySignature, ts.SyntaxKind.GetAccessor, ts.SyntaxKind.SetAccessor, ts.SyntaxKind.VariableDeclaration, ts.SyntaxKind.TypeParameter, ts.SyntaxKind.BindingElement]);
    const walk = (node, scope = '') => {
      let next = scope;
      const namedExpression = ts.isFunctionExpression(node) || ts.isClassExpression(node);
      const callableProperty = ts.isPropertyAssignment(node) && (ts.isArrowFunction(node.initializer) || ts.isFunctionExpression(node.initializer));
      if ((kinds.has(node.kind) || namedExpression || callableProperty) && node.name) {
        const name = node.name.getText(source);
        add(name, ts.SyntaxKind[node.kind], source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1, scope);
        next = scope ? `${scope}.${name}` : name;
      } else if ((ts.isFunctionDeclaration(node) || ts.isClassDeclaration(node)) && !node.name) {
        add('default', ts.SyntaxKind[node.kind], source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1, scope);
        next = 'default';
      } else if (ts.isConstructorDeclaration(node)) {
        add('constructor', 'Constructor', source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1, scope);
      } else if (ts.isExportAssignment(node)) {
        add('default', 'ExportAssignment', source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1, scope);
      } else if (ts.isExportSpecifier(node)) {
        add(node.name.text, 'ExportSpecifier', source.getLineAndCharacterOfPosition(node.getStart(source)).line + 1, scope);
      }
      ts.forEachChild(node, child => walk(child, next));
    };
    walk(source);
    diagnostics.push(...source.parseDiagnostics.map(d => `line ${source.getLineAndCharacterOfPosition(d.start ?? 0).line + 1}: ${ts.flattenDiagnosticMessageText(d.messageText, ' ')}`));
  } else if (/\.(rs|py|sh)$/.test(file)) {
    const language = file.endsWith('.rs') ? 'rust' : file.endsWith('.py') ? 'python' : 'bash';
    // This grammar snapshot mistakes Rust inner attributes for shebangs. Treat
    // them as outer attributes for declaration extraction, preserving offsets.
    const input = language === 'rust' ? text.replace(/^(\s*)#!(?=\[)/gm, '$1# ') : text;
    const tree = (await parserFor(language)).parse(input);
    const walk = (node, scope = '') => {
      let next = scope;
      const name = node.childForFieldName('name');
      if ((language === 'rust' && rustKinds.has(node.type)) || (language !== 'rust' && ['function_definition', 'class_definition'].includes(node.type))) {
        if (name) {
          add(name.text, node.type, node.startPosition.row + 1, scope);
          next = scope ? `${scope}::${name.text}` : name.text;
        }
      }
      if (node.type === 'impl_item') {
        next = [node.childForFieldName('trait')?.text, node.childForFieldName('type')?.text].filter(Boolean).join(' for ');
        add(next, 'impl', node.startPosition.row + 1, scope);
      }
      if (node.type === 'macro_invocation') {
        add(node.childForFieldName('macro')?.text ?? 'macro', 'macro invocation (unexpanded)', node.startPosition.row + 1, scope);
      }
      if (node.type === 'ERROR' || node.isMissing) diagnostics.push(`line ${node.startPosition.row + 1}: ${node.type}`);
      for (const child of node.namedChildren) walk(child, next);
    };
    walk(tree.rootNode);
    tree.delete();
  } else {
    // PowerShell/BAT have no bundled grammar: expose only declaration/label navigation.
    for (const [index, line] of text.split('\n').entries()) {
      const match = file.endsWith('.ps1') ? /^\s*(?:function|filter|class|enum)\s+([\w:-]+)/i.exec(line) : /^:([\w-]+)\s*$/.exec(line);
      if (match) add(match[1], file.endsWith('.ps1') ? 'lexical declaration' : 'batch label', index + 1);
    }
  }
  return { symbols, diagnostics };
}

export function moduleOf(file) {
  const parts = file.split('/');
  if (parts[0] === 'src-tauri' && parts[1] === 'src') return `rust-${parts.length > 3 ? parts[2] : 'root'}`;
  if (parts[0] === 'src') return `web-${parts.length > 2 ? parts[1] : 'root'}`;
  if (parts[0] === 'scripts') return `scripts-${parts.length > 2 ? parts[1] : 'root'}`;
  return 'tooling';
}

export function eligible(file, config) {
  return !file.split('/').some(part => config.excludeSegments.includes(part)) &&
    (config.roots.includes(file.split('/')[0]) || !file.includes('/')) && config.extensions.includes(path.extname(file));
}

export function inventory(root, config) {
  return [...new Set(execFileSync('git', ['ls-files', '-z', '--cached', '--others', '--exclude-standard'], { cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024 }).split('\0').filter(Boolean))]
    .filter(file => eligible(file, config))
    .filter(file => fs.existsSync(path.join(root, file)) && fs.lstatSync(path.join(root, file)).isFile())
    .sort(compare);
}

const escape = value => String(value).replaceAll('|', '\\|').replaceAll('`', "'").replace(/\s+/g, ' ');
const link = file => file.split('/').map(encodeURIComponent).join('/');

export async function generate(root = home, { check = false, module: selected } = {}) {
  const configText = fs.readFileSync(path.join(root, 'codemap.config.json'), 'utf8');
  const config = JSON.parse(configText);
  if (!Number.isInteger(config.pageRows) || config.pageRows < 1 || config.pageRows > 150) throw new Error('pageRows must be 1..150');
  const version = hash(normalize(fs.readFileSync(fileURLToPath(import.meta.url), 'utf8')) + configText + normalize(fs.readFileSync(path.join(home, 'pnpm-lock.yaml'), 'utf8')));
  const cachePath = path.join(root, '.codemap-cache.json');
  let cache = {};
  if (!check && fs.existsSync(cachePath)) {
    try { const stored = JSON.parse(fs.readFileSync(cachePath, 'utf8')); if (stored.version === version) cache = stored.files; } catch { /* Rebuild invalid local cache. */ }
  }
  const records = {};
  let parsed = 0;
  const files = inventory(root, config);
  if (selected && !files.some(file => moduleOf(file) === selected) && !Object.keys(cache).some(file => moduleOf(file) === selected)) throw new Error(`Unknown module: ${selected}`);
  for (const file of files) {
    const text = normalize(fs.readFileSync(path.join(root, file), 'utf8'));
    const digest = hash(text);
    if (cache[file]?.hash === digest) records[file] = cache[file];
    else {
      if (selected && moduleOf(file) !== selected) throw new Error(`Changed uncached file outside ${selected}: ${file}; run pnpm codemap`);
      records[file] = { hash: digest, ...await extract(file, text) };
      parsed++;
    }
  }
  if (selected && Object.keys(cache).some(file => !records[file] && moduleOf(file) !== selected)) throw new Error('Deletion outside selected module; run pnpm codemap');
  const outputs = new Map();
  const modules = [...new Set(files.map(moduleOf))].sort(compare);
  const index = ['# Code map index', '', '<!-- Generated by codemap.mjs; do not edit. -->', '', 'Read AGENTS.md → choose module → file/symbol page → source. [Method, commands and limits](CODE_MAP.md).', '', '| Module | Files | Symbols | Map |', '| --- | ---: | ---: | --- |'];
  let totalSymbols = 0;
  let diagnostics = 0;
  for (const module of modules) {
    const members = files.filter(file => moduleOf(file) === module);
    const count = members.reduce((sum, file) => sum + records[file].symbols.length, 0);
    totalSymbols += count;
    index.push(`| ${module} | ${members.length} | ${count} | [open](docs/codemap/${module}.md) |`);
    const moduleRows = [];
    for (const file of members) {
      const record = records[file];
      diagnostics += record.diagnostics.length;
      const slug = file.replace(/[^a-zA-Z0-9.-]/g, '_') + '-' + hash(file).slice(0, 8);
      const pages = Math.max(1, Math.ceil(record.symbols.length / config.pageRows));
      const pageLinks = [];
      for (let i = 0; i < pages; i++) {
        const name = `${slug}-${i + 1}.md`;
        pageLinks.push(`[${i + 1}](${name})`);
        const rows = ['# ' + file, '', '<!-- Generated by codemap.mjs; do not edit. -->', '', `[Module](${module}.md) · [Source](../../${link(file)}) · page ${i + 1}/${pages}`, '', `Source SHA-256: ${record.hash}`, '', '| Symbol | Kind | Scope | Line |', '| --- | --- | --- | ---: |'];
        for (const symbol of record.symbols.slice(i * config.pageRows, (i + 1) * config.pageRows)) rows.push(`| ${escape(symbol.name)} | ${escape(symbol.kind)} | ${escape(symbol.scope)} | [${symbol.line}](../../${link(file)}#L${symbol.line}) |`);
        if (!record.symbols.length) rows.push('| (no extracted declarations) | | | |');
        if (record.diagnostics.length) rows.push('', `Parser recovery: ${record.diagnostics.length} diagnostic(s). Inspect source; extraction may be incomplete.`, ...record.diagnostics.slice(0, 8).map(item => `- ${escape(item)}`));
        outputs.set(`docs/codemap/${name}`, rows.join('\n') + '\n');
      }
      moduleRows.push(`| [${file}](../../${link(file)}) | ${record.symbols.length} | ${pageLinks.join(' ')} |`);
    }
    const chunks = Math.ceil(moduleRows.length / config.pageRows);
    for (let i = 0; i < chunks; i++) {
      const name = i ? `${module}-${i + 1}.md` : `${module}.md`;
      outputs.set(`docs/codemap/${name}`, [`# ${module}`, '', '<!-- Generated by codemap.mjs; do not edit. -->', '', '[Root index](../../CODE_MAP_INDEX.md)', '', ...Array.from({ length: chunks }, (_, n) => `[Files ${n + 1}](${n ? `${module}-${n + 1}` : module}.md)`), '', '| File | Symbols | Symbol pages |', '| --- | ---: | --- |', ...moduleRows.slice(i * config.pageRows, (i + 1) * config.pageRows), ''].join('\n'));
    }
  }
  index.push('', `Total: ${files.length} source files, ${totalSymbols} extracted symbols, ${diagnostics} parser diagnostics.`, '', 'Scope includes local declarations; excludes imports, parameters, runtime/macro-generated symbols. PowerShell/BAT use lexical navigation only.');
  outputs.set('CODE_MAP_INDEX.md', index.join('\n') + '\n');
  const directory = path.join(root, 'docs', 'codemap');
  const stale = fs.existsSync(directory) ? fs.readdirSync(directory).filter(name => name.endsWith('.md') && !outputs.has(`docs/codemap/${name}`)) : [];
  const changed = [...outputs].filter(([file, text]) => !fs.existsSync(path.join(root, file)) || normalize(fs.readFileSync(path.join(root, file), 'utf8')) !== text);
  if (!check) {
    fs.mkdirSync(directory, { recursive: true });
    for (const [file, text] of changed) fs.writeFileSync(path.join(root, file), text);
    for (const file of stale) fs.unlinkSync(path.join(directory, file));
    fs.writeFileSync(cachePath, JSON.stringify({ version, files: records }));
  }
  const result = { files: files.length, symbols: totalSymbols, diagnostics, parsed, changed: changed.length, deleted: stale.length, pages: outputs.size };
  if (check && (changed.length || stale.length)) throw new Error(`Stale code map: ${JSON.stringify(result)}. Run pnpm codemap and include generated changes.`);
  return result;
}

export function installHook(root = home) {
  const custom = execFileSync('git', ['config', '--get', 'core.hooksPath'], { cwd: root, encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] });
  return custom;
}

export function setupHook(root = home) {
  let custom = '';
  try { custom = installHook(root).trim(); } catch { /* Unset core.hooksPath is normal. */ }
  if (custom) throw new Error('core.hooksPath is set; integrate node codemap.mjs --hook into your existing hook manually.');
  const hooks = execFileSync('git', ['rev-parse', '--git-path', 'hooks'], { cwd: root, encoding: 'utf8' }).trim();
  const hook = path.resolve(root, hooks, 'pre-commit');
  const content = '#!/bin/sh\n# ai-switcher-codemap\nnode codemap.mjs --hook\n';
  if (fs.existsSync(hook) && fs.readFileSync(hook, 'utf8') !== content) throw new Error('Existing pre-commit hook preserved; integrate node codemap.mjs --hook manually.');
  fs.mkdirSync(path.dirname(hook), { recursive: true });
  fs.writeFileSync(hook, content, { mode: 0o755 });
}

export async function runHook(root = home) {
  const result = await generate(root);
  // Never stage files: partial staging must be resolved explicitly by the author.
  const paths = ['CODE_MAP_INDEX.md', 'docs/codemap'];
  const unstaged = execFileSync('git', ['diff', '--name-only', '--', ...paths], { cwd: root, encoding: 'utf8' });
  const untracked = execFileSync('git', ['ls-files', '--others', '--exclude-standard', '--', ...paths], { cwd: root, encoding: 'utf8' });
  const config = JSON.parse(fs.readFileSync(path.join(root, 'codemap.config.json')));
  const dirty = execFileSync('git', ['diff', '--name-only', '-z'], { cwd: root, encoding: 'utf8' });
  const added = execFileSync('git', ['ls-files', '--others', '--exclude-standard', '-z'], { cwd: root, encoding: 'utf8' });
  const sourceDirty = (dirty + added).split('\0').some(file => eligible(file, config) || ['codemap.config.json', 'pnpm-lock.yaml'].includes(file));
  if (unstaged.trim() || untracked.trim() || sourceDirty) throw new Error('Code map refreshed. Review and stage source/map changes, then retry; partial source staging is not supported by this hook. Nothing was auto-staged.');
  return result;
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  try {
    const args = process.argv.slice(2);
    const valid = new Set(['--check', '--hook', '--install-hook', '--module']);
    for (let i = 0; i < args.length; i++) { if (!valid.has(args[i])) throw new Error(`Unknown argument: ${args[i]}`); if (args[i] === '--module' && !args[++i]) throw new Error('--module needs a name'); }
    const result = args.includes('--install-hook') ? setupHook() : args.includes('--hook') ? await runHook() : await generate(home, { check: args.includes('--check'), module: args.includes('--module') ? args[args.indexOf('--module') + 1] : undefined });
    console.log(JSON.stringify(result ?? { hook: 'installed' }));
  } catch (error) { console.error(error.message); process.exitCode = 1; }
}
