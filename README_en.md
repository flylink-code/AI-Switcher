# AI-Switcher

> Local configuration and provider manager for **Claude Code**, **Claude Desktop**, **Codex**, **OpenCode**, and **Pi CLI**. **v1.3.8**

[中文](README.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [License: MIT](LICENSE)

Built with **Tauri 2 + Rust + React**. It brings configuration files, OS credentials, and local tooling into one UI while keeping Claude Code, Claude Desktop, Codex, OpenCode, and Pi providers independent (OpenCode and Pi keep multiple providers side by side; save to sync).

Works locally by default: API keys go in the OS credential store, writes are backed up first, and sessions only read local JSONL.

| Platform | Package | Notes |
|---|---|---|
| Windows 10/11 | NSIS `.exe` (preferred) / MSI | Full feature set |
| Linux (preview) | AppImage / `.deb` | Best-effort; official Claude Desktop Linux config paths are covered; some localization features remain limited |

---

## Open source

AI-Switcher is released under the **[MIT License](LICENSE)**. Source: [flylink-code/AI-Switcher](https://github.com/flylink-code/AI-Switcher).

- **You may** use, modify, distribute, and commercialize the software; derivatives may use another license if you keep the MIT copyright and permission notice
- **You must** include the copyright and permission notice from `LICENSE` in all copies or substantial portions
- **No affiliation**: AI-Switcher is an independent community project and is not affiliated with, sponsored by, or endorsed by Anthropic, OpenAI, or the projects listed under Acknowledgements. Claude, Claude Code, Claude Desktop, Codex, ChatGPT, and Pi are trademarks of their respective owners
- **Third-party dependencies**: builds link npm / crates packages that remain under their own licenses
- **Inspiration only**: projects in Acknowledgements informed product and implementation ideas; if you port copyrighted code from them, follow those upstream licenses as well (for example AGPL-3.0)
- **Contributions**: Issues and PRs are welcome on GitHub; contributions are accepted under this repository’s MIT license

---

## Install

Download the latest build from [GitHub Releases](https://github.com/flylink-code/AI-Switcher/releases/latest):

- **Windows**: prefer the NSIS installer (per-user, usually no admin). The app binary is `AISwitcher.exe`.
- **Linux**: prefer the `.AppImage` (`chmod +x`, then run).

Install Claude Code, Claude Desktop, the Codex CLI, OpenCode (CLI / Desktop), or Pi as needed. Installing/updating agent CLIs requires **Node.js ≥22** on the machine (detect/install via **Settings → Tools & environment → Agent tools**).

---

## Features

### Navigation & layout (1.3.8)

- **Two layouts**: switch **sidebar** / **top** navigation in the title bar (browser-like default vs vertical tabs); preference stored as `cs.layoutMode`
- **Top nav (seven items)**: Overview · Providers · Usage · Accounts & quota · Workspace · **Sessions** · Settings
- **Overview**: status strip → last-24h usage hero → attention / recent activity → year heatmap (Usage Intelligence)
- **Settings children** (Tools & environment): local proxy, environment, **Agent tools**, localization, about (with **← Settings** back header; Sessions is now a top-level page)
- **Visible agents**: Settings checkboxes control which tools appear in global and per-page switchers
- **Workspace tabs follow the Agent**: pick an Agent first; only its supported MCP / Prompts / Skills / Agents / Plugins / Projects tabs are shown
- **Agent switchers**: page-local on Providers / Proxy / Workspace (Claude Code / Desktop / Codex / OpenCode / Pi)

### Providers and switching

- Manage third-party APIs, model mappings, import/export, connection tests, Base URL speed tests, and model discovery for Claude Code / Desktop / Codex / OpenCode / Pi separately
- Provider cards can **copy to another Agent**; Providers page can **import from another Agent** (fields mapped to the target protocol)
- Codex providers can toggle catalog **Web Search** (writes `supports_search_tool` / `web_search_tool_type`)
- Environment page can set top-level `web_search`: `disabled | cached | indexed | live` (separate from catalog toggles; does not write deprecated `features.web_search*`)
- One-click switch with backups; restore official login configs
- On Claude targets, sign in with a **ChatGPT subscription** (via local proxy); Codex official accounts use terminal `codex login`
- Codex writes `~/.codex/config.toml`; OpenAI-compatible upstreams can connect directly, while Anthropic / OAuth still use the local proxy
- Deep Link: `ai-switcher://v1/import?resource=provider|mcp&payload=...` (preview before import)

### Local proxy

Anthropic Messages-compatible forwarding, model mapping, credential injection, streaming, status, and logs. Optional automatic failover (off by default). **Proxy-backed sessions can hot-switch upstreams**; direct (non-proxy) setups may still need a CLI restart.

Entry point: **Settings → Tools & environment → Local proxy** (port / force restart / failover). Day-to-day provider switches still start/stop the proxy from the Providers page. Multi-turn `openai_responses` assistant history uses `output_text` (avoids 502 from the second turn).

### Antigravity gateway

Built-in local reverse proxy (default `http://127.0.0.1:15830`) that wraps Google / Antigravity (Cloud Code) for Claude Code, Claude Desktop, Codex, and Pi:

- **Protocols**: Anthropic `/v1/messages`, OpenAI Chat `/v1/chat/completions`, and OpenAI Responses `/v1/responses` (Codex must bind `openai_responses`)
- **Account pool**: browser OAuth import, multi-account quota scheduling and cooldown rotation; refresh quota to sync the live model catalog; background auto-refresh
- **Model catalog**: prefers current Gemini tiers (including `-low` / `-medium` / `-high`); hides retired legacy IDs
- **One-click bind**: ensure providers on the Accounts & quota page, then switch from each tool’s provider list
- **Usage**: gateway requests land in `proxy_request_logs` (`target_app=antigravity`)
- **Note**: personal-use gateway — review account and upstream terms yourself; do not use it as a commercial relay

### OpenCode

Reads/writes `~/.config/opencode/opencode.json` (shared by CLI and Desktop). Multiple providers coexist; save to sync — no switch step:

- **Provider sync**: save/delete/import writes `aisw-<id>` entries; pick models inside OpenCode
- **Import from local config**: **Update from local config** on Workbench/Providers syncs from `opencode.json(c)` (skips managed entries and Desktop built-in connectors)
- **Sessions & usage**: scans `opencode.db`; detect/install/update OpenCode CLI under **Settings → Agent tools** (Node.js ≥22)

### Pi

Reads/writes `~/.pi/agent/models.json` and `auth.json`. Same model as OpenCode: multiple providers coexist; save to sync — no switch step:

- **Provider sync**: save/delete/import writes every enabled Pi provider; pick models inside Pi
- **Direct upstream**: OpenAI-compatible APIs (including Responses) use the provider Base URL and skip the local proxy (Pi’s built-in proxy only mounts `/v1/messages`; Responses would 404). Anthropic can use the Antigravity gateway
- **Workspace**: Prompts (`~/.pi/agent/AGENTS.md`), Skills, MCP (`~/.pi/agent/mcp.json`, needs pi-mcp-adapter / extension). No Plugins, Agents, or Profiles
- **Sessions & usage**: scans `message.usage` in `~/.pi/agent/sessions/**/*.jsonl`; Usage refresh syncs (deduped against Antigravity gateway rows)
- **Agent tools**: detect/install/update the Pi CLI (Node.js ≥22)

### Agent tools (1.3.7)

Unified detect/install under **Settings → Tools & environment → Agent tools**:

- **Node.js environment** (local ≥22; optional fnm + mirror install)
- Claude Code / Codex / OpenCode / **Pi** CLI install & update (npm global; works with npm 11 and Windows `%APPDATA%\npm`)

The About page keeps only app version, update check, update mirror, and onboarding tip restore.

### MCP / Prompts / Skills / Agents / Plugins

- MCP: unified management with Codex / Pi sync; remote HTTP/SSE, OAuth status/clear, and Desktop Connectors / `.mcpb` conflict hints
- MCP Registry: browse the official Registry and install entries that safely convert to Claude config (secret/URL-template entries still need manual setup)
- Prompts: `CLAUDE.md` / Codex `AGENTS.md` / Pi `~/.pi/agent/AGENTS.md` presets with rename and one-click activate
- Skills: install, enable, update, and remove Claude Code, Codex, or Pi Skills from GitHub or ZIP; add/remove skill repositories and pick skills to install onto the current Agent; scan stray skills to register/ignore
- Agents: manage Claude Code user agents under `~/.claude/agents`
- **Plugins**: single Workspace **Plugins** tab with in-page Claude Code / Codex switch; marketplaces, catalog install, enable/disable, uninstall, check/update

### Projects (Profiles)

Snapshot provider / MCP / Skills / Prompt selections per Claude Code, Desktop, or Codex scope; apply and rename in one click.

### Sessions

Browse, filter, and search local Claude Code, Codex, OpenCode, and Pi sessions; export / import / backup / trash (OpenCode does not support archive/export/trash yet). Claude Desktop private history formats are not parsed.

### Localization

Manage Claude Code plugins, editor patch helpers, and Claude Desktop language packs separately. Applying an editor patch always requires confirmation inside the editor. Chinese localization also normalizes invalid bare `spinnerVerbs` arrays.

### Usage, environment, and system

- Usage: merges proxy logs with Codex / Claude Code / OpenCode / Pi local session events (including JSONL backfill for Anthropic-compatible direct upstreams and Pi sessions); multi-currency estimates; Opus / Codex Fast tier (`*-fast`) matching; in-page source filters
- Environment: config paths, library migration / portable export, WSL·SSH sync, **doctor diagnostics and one-click visibility repair** (does not force-rewrite a direct `ANTHROPIC_BASE_URL`)
- Tray switching, EN/ZH UI, light / dark / system theme, launch at login

---

## Session Manager

| Source | Path |
|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` |
| Codex | `$CODEX_HOME/sessions/**/*.jsonl` (default `~/.codex/sessions/`) |
| OpenCode | `~/.local/share/opencode/opencode.db` (and legacy JSON storage) |
| Pi | `~/.pi/agent/sessions/**/*.jsonl` |

The list reads metadata only; message bodies load when you open details or run full-text search. Paths stay under the session roots. Browsing does not modify originals.

Claude Desktop only detects the data directory and offers the official entry `claude://claude.ai/new`; known session IDs can use [official deep links](https://support.claude.com/en/articles/14729294-open-claude-desktop-with-a-link).

---

## Data and config

| Path | Purpose |
|---|---|
| `~/.claude/settings.json` | Claude Code active provider |
| `~/.claude.json` | Claude Code MCP / project config |
| `~/.claude/projects/` | Claude Code sessions |
| `~/.claude/skills/` | Claude Code Skills |
| `%LOCALAPPDATA%\Claude-3p\configLibrary\` | Claude Desktop third-party config (Windows) |
| `$CODEX_HOME` or `~/.codex/` | Codex config, sessions, Skills, Plugins |
| `~/.config/opencode/opencode.json` | OpenCode providers (CLI + Desktop; `opencode.jsonc` also supported) |
| `~/.local/share/opencode/` | OpenCode session database |
| `~/.pi/agent/models.json` / `auth.json` | Pi providers and credentials |
| `~/.pi/agent/sessions/` | Pi session JSONL |
| `~/.pi/agent/AGENTS.md` / `skills/` / `mcp.json` | Pi Prompts / Skills / MCP |
| `~/.claude/agents/` | Claude Code Agents |
| `~/.claude-switcher/` (relocatable) | App library: database, backups, logs |

The product name is AI-Switcher; the original app id and default library path remain for compatibility. The library can move to another drive (SHA-256 verified; restart to apply). Export / sync omit API keys by default.

---

## Security and privacy

- API keys: Windows Credential Manager / macOS Keychain / Linux Secret Service
- Config: atomic writes + rotating backups
- Sessions: no full-text index; import/export/trash validate roots and symlinks
- Aside from connection tests, model discovery, update checks, and user-confirmed remote archive pushes, local content is not uploaded

---

## Develop from source

Requires: Node.js 22+, pnpm 9+ (Corepack OK), Rust stable. On Windows also need VS 2022 C++ desktop workload.

```powershell
pnpm install
pnpm tauri dev
# Without MSVC env vars:
scripts\tauri-msvc.bat dev
```

Dev server port is **5250** (must match `tauri.conf.json` `devUrl`). More reliable hot reload: run `pnpm dev`, then launch `src-tauri\target\debug\claude-switcher.exe` built with `cfg(dev)`.

### Build (Windows)

Scripts run the full Rust test suite by default:

```powershell
pnpm build:exe              # release exe → release\AISwitcher.exe
pnpm build:exe:debug        # debug → release\AISwitcher-debug.exe
pnpm build:exe:bundle       # MSI + NSIS
scripts\build-exe.bat release skip-tests   # skip tests
```

| Artifact | Path |
|---|---|
| Release binary | `src-tauri\target\release\AISwitcher.exe` |
| Convenience copy | `release\AISwitcher.exe` / `AISwitcher-debug.exe` |
| Installers | `src-tauri\target\release\bundle\` |

---

## Project layout

```text
src/                      React + Ant Design + Zustand + i18next
  pages/                  Feature pages (Workbench / Providers / Usage / …)
  components/layout/      Sidebar shell (AppShell / SideNav / …)
  components/v2/shell/    Top-nav shell (DesktopShell / TopNavigation)
  lib/pageRegistry.ts     Page keys + lazy loaders
src-tauri/src/            Rust: config, proxy, Antigravity, DB, tray, sessions
  antigravity/            AG gateway (default :15830)
  proxy/                  Local Anthropic-compatible proxy
  config/                 Claude / Codex / OpenCode config I/O
  coding/pi/              Pi models.json / auth / MCP / session usage
  database/               SQLite (user_version)
scripts/                  Windows dev / build scripts
```

---

## Current boundaries

- Client scope: Claude Code + Claude Desktop + Codex + OpenCode + Pi; the Antigravity gateway can attach Gemini / Cloud Code upstreams to those clients
- Pi has no Plugins / Agents / Profiles / tray switching; Pi OpenAI-compatible upstreams connect directly and skip the local proxy
- Plugins page manages installed marketplaces/plugins locally — not a full replacement for the official CLI store browser
- Session “resume” only copies a command; it does not open a terminal
- No automatic remote conflict merge; no team sharing
- Claude Code and Desktop provider lists / active state stay independent
- Claude Desktop private history formats are not parsed
- When Antigravity accounts are exhausted, upstreams may still 429 (the gateway rotates; it cannot invent quota)

---

## Acknowledgements

Independent project — no affiliation with the repos below or with Anthropic / OpenAI. Licenses in the table refer to the **upstream repos**; AI-Switcher source remains [MIT](LICENSE). If you port upstream code, follow those licenses and copyright notices as well.

| Project | Inspiration | Upstream |
|---|---|---|
| AI Toolbox | Multi-tool config, sessions, desktop IA | [coulsontl/ai-toolbox](https://github.com/coulsontl/ai-toolbox) MIT |
| Antigravity-Manager | Antigravity / Cloud Code reverse proxy, account pool, protocol mapping | [lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) |
| CLIProxyAPI | Multi-protocol gateway and upstream adaptation | [router-for-me/CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) |
| cc Proxy | Desktop local proxy and model rewrite | [arhsis/cc-proxy](https://github.com/arhsis/cc-proxy) |
| CC Switch | Provider switching, Tauri, sessions, tray | [farion1231/cc-switch](https://github.com/farion1231/cc-switch) MIT |
| Claude Code VS Code ZH pack | Extension localization flow | [zstings/claude-code-zh-cn](https://github.com/zstings/claude-code-zh-cn) MIT |
| Claude Code ZH plugin | CLI localization install/restore | [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) |
| Claude Desktop ZH patch | Install discovery and language packs | [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn) |
| Code Switch | Local proxy, failover, Codex config | [daodao97/code-swtich](https://github.com/daodao97/code-swtich) Apache-2.0 |
| Codex++ | Codex API writes and history sync | [BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus) AGPL-3.0 |
