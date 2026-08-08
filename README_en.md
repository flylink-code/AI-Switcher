# AI-Switcher

> Local configuration and provider manager for **Claude Code**, **Claude Desktop**, and **Codex**. **v1.3.0**

[中文](README.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [License: MIT](LICENSE)

Built with **Tauri 2 + Rust + React**. It brings configuration files, OS credentials, and local tooling into one UI while keeping Claude Code, Claude Desktop, and Codex providers and active configs independent.

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
- **No affiliation**: AI-Switcher is an independent community project and is not affiliated with, sponsored by, or endorsed by Anthropic, OpenAI, or the projects listed under Acknowledgements. Claude, Claude Code, Claude Desktop, Codex, and ChatGPT are trademarks of their respective owners
- **Third-party dependencies**: builds link npm / crates packages that remain under their own licenses
- **Inspiration only**: projects in Acknowledgements informed product and implementation ideas; if you port copyrighted code from them, follow those upstream licenses as well (for example AGPL-3.0)
- **Contributions**: Issues and PRs are welcome on GitHub; contributions are accepted under this repository’s MIT license

---

## Install

Download the latest build from [GitHub Releases](https://github.com/flylink-code/AI-Switcher/releases/latest):

- **Windows**: prefer the NSIS installer (per-user, usually no admin). The app binary is `AISwitcher.exe`.
- **Linux**: prefer the `.AppImage` (`chmod +x`, then run).

Install Claude Code, Claude Desktop, or the Codex CLI as needed.

---

## Features

### Workspace shell (1.0)

- **Overview**: usage summary metrics plus a GitHub-style daily activity heatmap (adaptive layout for short and long ranges)
- **Global tool switcher**: header Segmented control for Claude Code / Desktop / Codex; sidebar and pages follow the same workspace context
- **Status strip**: proxy phase/port and active provider in the header, with one-click navigation
- **Grouped sidebar**: Core / Extensions / Data / System

### Providers and switching

- Manage third-party APIs, model mappings, import/export, connection tests, Base URL speed tests, and model discovery for Claude Code / Desktop / Codex separately
- Codex providers can toggle catalog **Web Search** (writes `supports_search_tool` / `web_search_tool_type`)
- Environment page can set top-level `web_search`: `disabled | cached | indexed | live` (separate from catalog toggles; does not write deprecated `features.web_search*`)
- One-click switch with backups; restore official login configs
- On Claude targets, sign in with a **ChatGPT subscription** (via local proxy); Codex official accounts use terminal `codex login`
- Codex writes `~/.codex/config.toml`; OpenAI-compatible upstreams can connect directly, while Anthropic / OAuth still use the local proxy
- Deep Link: `ai-switcher://v1/import?resource=provider|mcp&payload=...` (preview before import)

### Local proxy

Anthropic Messages-compatible forwarding, model mapping, credential injection, streaming, status, and logs. Optional automatic failover (off by default). **Proxy-backed sessions can hot-switch upstreams**; direct (non-proxy) setups may still need a CLI restart.

### Antigravity gateway (1.3)

Built-in local reverse proxy (default `http://127.0.0.1:15830`) that wraps Google / Antigravity (Cloud Code) for Claude Code, Claude Desktop, and Codex:

- **Protocols**: Anthropic `/v1/messages`, OpenAI Chat `/v1/chat/completions`, and OpenAI Responses `/v1/responses` (Codex must bind `openai_responses`)
- **Account pool**: browser OAuth import, multi-account quota scheduling and cooldown rotation; refresh quota to sync the live model catalog
- **One-click bind**: use **Ensure Provider** on the Antigravity page, then switch from each tool’s provider list
- **Reasoning tiers**: Gemini supports `-low` / `-medium` / `-high` suffixes; Claude Desktop unlocks the native effort slider via role routing (`claude-sonnet-5` + `labelOverride`); the gateway keeps session-sticky effort and defaults bare Gemini to high when effort is absent
- **Usage**: gateway requests land in `proxy_request_logs` (`target_app=antigravity`)
- **Note**: personal-use gateway — review account and upstream terms yourself; do not use it as a commercial relay

### MCP / Prompts / Skills / Agents / Plugins

- MCP: unified management with Codex sync; remote HTTP/SSE, OAuth status/clear, and Desktop Connectors / `.mcpb` conflict hints
- MCP Registry: browse the official Registry and install entries that safely convert to Claude config (secret/URL-template entries still need manual setup)
- Prompts: `CLAUDE.md` / Codex `AGENTS.md` presets with rename and one-click activate
- Skills: install, enable, update, and remove Claude Code or Codex Skills from GitHub or ZIP; scan stray skills to register/ignore
- Agents: manage Claude Code user agents under `~/.claude/agents`
- Codex Plugins: enable/disable/uninstall; wraps `codex plugin marketplace list/add/remove` (not a full store browser)

### Projects (Profiles)

Snapshot provider / MCP / Skills / Prompt selections per Claude Code, Desktop, or Codex scope; apply and rename in one click.

### Sessions

Browse, filter, and search local Claude Code and Codex JSONL sessions; export / import / backup / trash. Claude Desktop private history formats are not parsed.

### Localization

Manage Claude Code plugins, editor patch helpers, and Claude Desktop language packs separately. Applying an editor patch always requires confirmation inside the editor. Chinese localization also normalizes invalid bare `spinnerVerbs` arrays.

### Usage, environment, and system

- Usage: merges proxy logs with Codex / Claude Code local session events (including JSONL backfill for Anthropic-compatible direct upstreams); multi-currency estimates (headline picks the largest absolute amount); Opus / Codex Fast tier (`*-fast`) matching
- Environment: config paths, library migration / portable export, WSL·SSH sync, **doctor diagnostics and one-click visibility repair** (does not force-rewrite a direct `ANTHROPIC_BASE_URL`); environment page organized with Tabs
- Tray switching, EN/ZH UI, light / dark / system theme, launch at login

---

## Session Manager

| Source | Path |
|---|---|
| Claude Code | `~/.claude/projects/**/*.jsonl` |
| Codex | `$CODEX_HOME/sessions/**/*.jsonl` (default `~/.codex/sessions/`) |

The list view reads metadata only; message bodies load for details or full-text search. Paths must stay under the session root. Browsing never modifies originals.

For Claude Desktop, the app only detects the data directory and offers the official `claude://claude.ai/new` entry. Known conversation IDs can use Anthropic’s [deep-link format](https://support.claude.com/en/articles/14729294-open-claude-desktop-with-a-link).

---

## Data and configuration

| Path | Purpose |
|---|---|
| `~/.claude/settings.json` | Active Claude Code provider |
| `~/.claude.json` | Claude Code MCP / project config |
| `~/.claude/projects/` | Claude Code sessions |
| `~/.claude/skills/` | Claude Code Skills |
| `%LOCALAPPDATA%\Claude-3p\configLibrary\` | Claude Desktop third-party profiles (Windows) |
| `$CODEX_HOME` or `~/.codex/` | Codex config, sessions, Skills, Plugins |
| `~/.claude/agents/` | Claude Code Agents |
| `~/.claude-switcher/` (relocatable) | App library: database, backups, logs |

The product name is AI-Switcher; the app id and default library path are kept for compatibility. The library can move to another drive (SHA-256 verified, effective after restart). Export / sync omit API keys by default.

---

## Security and privacy

- API keys: Windows Credential Manager / macOS Keychain / Linux Secret Service
- Config: atomic writes with rotating backups
- Sessions: no full-text DB; import/export/trash validate roots and symlinks
- No local content upload except provider tests, model discovery, update checks, user downloads, and confirmed remote archive pushes

---

## Develop from source

Requires Node.js 20+, pnpm 9+ (Corepack is fine), and Rust stable. On Windows also install the VS 2022 Desktop development with C++ workload.

```powershell
pnpm install
pnpm tauri dev
# Without MSVC env vars:
scripts\tauri-msvc.bat dev
```

### Build (Windows)

Scripts run the full Rust test suite first:

```powershell
pnpm build:exe              # release exe → release\AISwitcher.exe
pnpm build:exe:debug        # debug → release\AISwitcher-debug.exe
pnpm build:exe:bundle       # MSI + NSIS
scripts\build-exe.bat release skip-tests   # skip tests
```

| Artifact | Path |
|---|---|
| Release binary | `src-tauri\target\release\AISwitcher.exe` |
| Test copies | `release\AISwitcher.exe` / `AISwitcher-debug.exe` |
| Installers | `src-tauri\target\release\bundle\` |

---

## Project layout

```text
src/                  React + Ant Design + Zustand + i18next
src-tauri/src/        Rust: config, proxy, database, tray, sessions
scripts/              Windows develop / build scripts
```

---

## Current limitations

- Client scope: Claude Code + Claude Desktop + Codex; the Antigravity gateway can attach Gemini / Cloud Code upstreams to those clients
- Codex Plugins are local detect/enable only — not a full official marketplace
- Session “resume” copies a command; it does not launch a terminal
- No auto-merge of remote sync conflicts; no team sharing
- Claude Code and Desktop provider lists and active selections stay independent
- Claude Desktop private session formats are not parsed
- When Antigravity dual-account quotas are exhausted, upstream may still return 429 (rotation helps; it cannot invent quota)

---

## References and acknowledgements

Independent project; not affiliated with the repositories below or with Anthropic / OpenAI. Licenses in the table describe **those upstream projects**; AI-Switcher source remains under this repo’s [MIT](LICENSE). When quoting or porting upstream code, follow the corresponding license and copyright notices.

| Project | Area referenced | Upstream |
|---|---|---|
| AI Toolbox | Multi-tool config, sessions, desktop IA | [coulsontl/ai-toolbox](https://github.com/coulsontl/ai-toolbox) MIT |
| Antigravity-Manager | Antigravity / Cloud Code proxy, account pool, protocol mapping ideas | [lbjlaq/Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) |
| CLIProxyAPI | Multi-protocol gateway and upstream adaptation ideas | [router-for-me/CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) |
| cc Proxy | Desktop local proxy and model replacement | [arhsis/cc-proxy](https://github.com/arhsis/cc-proxy) |
| CC Switch | Provider switching, Tauri, sessions, tray | [farion1231/cc-switch](https://github.com/farion1231/cc-switch) MIT |
| Claude Code VS Code Chinese pack | Extension discovery and localization flow | [zstings/claude-code-zh-cn](https://github.com/zstings/claude-code-zh-cn) MIT |
| Claude Code Chinese plugin | CLI localization install / restore | [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) |
| Claude Desktop Chinese patch | Install discovery and language packs | [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn) |
| Code Switch | Local proxy, failover, Codex config | [daodao97/code-swtich](https://github.com/daodao97/code-swtich) Apache-2.0 |
| Codex++ | Codex API writes and session sync | [BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus) AGPL-3.0 |
