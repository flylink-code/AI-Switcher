# Layered code map

`AGENTS.md` → `CODE_MAP_INDEX.md` → module file list → file symbol pages → source. The index routes by directory (`rust-gateway`, `web-pages`, etc.); source file paths, symbol names, kinds, lexical scopes and declaration lines are searchable in `docs/codemap`. Pages contain at most 100 symbol/file rows by default. Read only the module/file pages needed; do not load the whole map into agent context.

## Commands

Node 22 or later and the repository's pnpm 9.15 lockfile are required. After `pnpm install --frozen-lockfile`:

```powershell
pnpm codemap
pnpm codemap --module rust-gateway
pnpm codemap:check
pnpm codemap:test
pnpm codemap:benchmark
pnpm codemap:hook
```

The default command discovers Git tracked and nonignored untracked sources, hashes normalized LF content, reuses a local `.codemap-cache.json`, and writes only changed pages. Additions/deletions/renames update the index and remove obsolete generated pages. A module command requires an existing valid cache and refuses changes outside that module; use the default command after configuration/parser changes or first checkout. Even a warm incremental run inventories and hashes every eligible source: incremental means parsing/output, not zero filesystem reads. The cache is disposable and ignored. Check mode ignores the cache, reconstructs all expected pages, checks stale/extra/missing pages without writing, and exits nonzero on drift. Generation has no timestamp or machine path.

CI runs tooling tests and full map verification before frontend lint/build. It detects forgotten regeneration for any indexed code change, including changes that preserve symbols. Local agents are instructed to regenerate after **all file edits**; non-code files do not themselves add symbol rows. No external agent compliance is guaranteed.

The optional pre-commit installer refuses an existing hook or custom `core.hooksPath`; it does not overwrite hooks or change Git config. Existing hook users can add `node codemap.mjs --hook` manually. The hook regenerates automatically, then blocks if generated maps need staging or indexed source has unstaged edits. Review/stage intentionally and retry. It never auto-stages anything, and deliberately rejects partial source staging rather than claiming to map the staged snapshot. CI remains authoritative. No hooks are installed by dependency installation.

## Coverage contract

`codemap.config.json` defines roots/extensions and excluded path segments. Root tooling, `src`, `src-tauri`, and `scripts` are indexed. Vendored examples, dependencies, build outputs, generated bindings/schemas, runtime artifacts and caches are excluded even if tracked. Git-ignored untracked files and symlinks are excluded. Secret/config formats (`.env`, JSON credentials, PEM, etc.) are outside the extension allowlist. Do not put secrets in source code: no static tool can identify all secrets in an allowed source file.

- **TS/TSX/JS:** TypeScript compiler syntax tree: functions, arrow/function-valued variable declarations, variables/binding elements, classes, constructors, interfaces, type aliases/parameters, enums/members, namespaces, methods/signatures, properties/accessors and export aliases/default assignments. Includes nested/local declarations. Imports, ordinary parameters, anonymous callbacks and object literal keys are not standalone declarations. Lexical scope is navigation context, not resolved type identity.
- **Rust:** Tree-sitter: functions/signatures, structs/fields, enums/variants, traits, types/associated types, constants/statics, modules, impl blocks and macro definitions/invocations. Every included `.rs` fragment is parsed independently (including `include!` files); fragments retain actual file coordinates. Impl/trait names give lexical context. Local `let` bindings, generic/lifetime parameters and imports are excluded from the meaningful item-level symbol inventory. Macro token bodies/expansions and build-generated declarations are not expanded; invocation sites remain visible. `cfg` branches are indexed statically regardless of active build target.
- **Python/shell:** bundled Tree-sitter grammars expose named functions/classes. **PowerShell/BAT:** explicitly labeled lexical function/filter/class/enum declarations and batch labels only, potentially matching declarations inside multiline comments/strings. They are navigation aids, not full language parsers. Top-level scripts with no declarations still receive a file page.

For the pinned Rust grammar's shebang ambiguity, line-leading inner attributes `#![...]` are parsed as outer attributes by replacing `!` with a space; source offsets/line numbers remain unchanged. This is navigation-only normalization, not a Rust source edit. `docs/codemap/` is exclusively generator-owned; do not put handwritten documentation there.

Parser recovery diagnostics are displayed per file and counted in the root index; check mode checks reproducibility, not semantic validity. Read source when recovery is reported. This inventory does not claim a complete semantic call graph or every runtime symbol. Use editor references/type checking/rust-analyzer for semantic questions; no LSP server needs to be running to generate the map.

## Tool choice (official references)

| Tool | Strength | Role here |
| --- | --- | --- |
| [Universal Ctags](https://docs.ctags.io/en/latest/man/ctags.1.html) | Mature multi-language definition tags, kinds, fields, JSON output | Viable alternative; not installed on this machine. Requires a pinned executable and language/kind configuration across Windows/Linux. No silent regex fallback for Rust/TS. |
| [Tree-sitter](https://tree-sitter.github.io/tree-sitter/) | Concrete syntax trees, error recovery, independent grammar packages | Pinned `web-tree-sitter` + `tree-sitter-wasms` avoids native compiler/install prerequisites. Rust fragments parse without a Cargo build. Grammar snapshot is older than current Rust; diagnostics expose recovery rather than hiding it. |
| [TypeScript compiler API](https://github.com/microsoft/TypeScript/wiki/Using-the-Compiler-API) | TS/TSX/JS syntax including arrow variables and class members | Reuses the project's locked TypeScript compiler for accurate syntax extraction. |
| [rust-analyzer](https://rust-analyzer.github.io/book/) | Cargo-aware types, references, macro expansion and semantic navigation | Supplement manually for bug tracing; workspace/build configuration makes it unsuitable as a mandatory lightweight deterministic generator here. |

Ctags is a tag generator; Tree-sitter is a parser; rust-analyzer is a language server. Their output/operational costs are not interchangeable. clangd targets C/C++, which are not this project's principal source languages. Parser versions are frozen by `pnpm-lock.yaml`; updating it invalidates the cache.

## Completion checklist

1. Locate the relevant module and declaration, then inspect source and semantic usages as necessary.
2. Make changes and run the smallest relevant validation.
3. Run `pnpm codemap` and `pnpm codemap:check` after final edits.
4. Review source/map diffs, parser diagnostics and deletions; include generated pages with source changes.
5. Report actual verification and limitations. Do not hand-edit generated maps.

## Verification record (2026-10-16)

Validated locally on Windows with Node **24.15.0** / pnpm 9.15.0; CI is configured for Node 22 but was not executed remotely in this task. Focused ESLint and VS Code Problems reported no errors. Four Node test groups cover TS/TSX declarations, Rust impl/trait/macros/include fragments/inner attributes, secondary script navigation, and Git lifecycle integration. The integration group creates an isolated `.codemap-fixture` repository inside the project, exercises actual pre-commit execution and fixture commits, and removes it in `finally`; it never commits the application repository. It verifies edit/add/delete/rename, module-only parsing, untouched page mtimes, stale/extra maps, exclusions, links, paging, partial-staging rejection and existing-hook preservation.

`pnpm codemap:check` reconstructed the complete map with zero drift and zero parser diagnostics. The initial completed inventory contains 412 source files and roughly 25,000 declarations/invocation records, 558 detail/module pages (plus root index), about 3.22 MB of generated Markdown. Maximum observed page size was 17,126 bytes and 100 data rows. The benchmark validates every generated relative link; fixtures validate excluded files never enter the inventory. Counts evolve with code; the generated index is authoritative.

Reproducible bug-location simulation: investigate login preservation when switching a Codex provider. Starting at the root index, choose `rust-config`, then `codex.rs`, then `apply_auth_strategy` at line 253. Source inspection confirms the function checks existing credential login material and sets/removes `experimental_bearer_token`. This is a navigation exercise, not a claim to have found or fixed an application defect.

| Actual read strategy | Files read | Bytes read | Single observed wall time |
| --- | ---: | ---: | ---: |
| Index → module → symbol page → complete source file | 4 | 84,938 | 1.114 ms |
| Scan all Rust sources in the same `config` module | 11 | 316,161 | 2.639 ms |

Read bytes decreased by approximately 73% in this bounded scenario. Both paths locate the same declaration; no repository-wide scan is used to inflate the comparison. Counts include entire source files actually read, not hypothetical line slices. Timings are single warm-filesystem observations, exclude map construction, and are **not a speed guarantee**. A known exact filename/symbol can already be found cheaply using targeted search; maps primarily improve routing/context selection. Re-run `pnpm codemap:benchmark` for current counts/timings and page/link validation.
