# AI-Switcher

> Local configuration and provider manager for **Claude Code**, **Claude Desktop**, and **Codex**.

[中文](README.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [License: MIT](LICENSE)

Built with **Tauri 2 + Rust + React**. It brings configuration files, OS credentials, and local tooling into one UI while keeping Claude Code, Claude Desktop, and Codex providers and active configs independent.

Works locally by default: API keys go in the OS credential store, writes are backed up first, and sessions only read local JSONL.

| Platform | Package | Notes |
|---|---|---|
| Windows 10/11 | NSIS `.exe` (preferred) / MSI | Full feature set |
| Linux (preview) | AppImage / `.deb` | Best-effort; some Desktop localization features unavailable |

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

### Providers and switching

- Manage third-party APIs, model mappings, import/export, connection tests, and model discovery for Claude Code / Desktop / Codex separately
- One-click switch with backups; restore official login configs
- On Claude targets, sign in with a **ChatGPT subscription** (via local proxy); Codex official accounts use terminal `codex login`
- Codex providers write to `~/.codex/config.toml` and do not use the Claude local proxy

### Local proxy

Anthropic Messages-compatible forwarding, model mapping, credential injection, streaming, status, and logs. Optional automatic failover (off by default).

### MCP / Prompts / Skills

Maintain MCP servers (with Codex sync) and `CLAUDE.md` presets. Install, enable, update, and remove Claude Code or Codex Skills from GitHub or ZIP archives.

### Sessions

Browse, filter, and search local Claude Code and Codex JSONL sessions; export / import / backup / trash. Claude Desktop private history formats are not parsed.

### Localization

Manage Claude Code plugins, editor patch helpers, and Claude Desktop language packs separately. Applying an editor patch always requires confirmation inside the editor.

### Usage, environment, and system

- Usage: merges proxy logs with Codex local events (estimated cost is proxy-log only)
- Environment: config paths, library migration / portable export, WSL·SSH sync preview+push, Claude Code / Codex CLI install detection
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
| `$CODEX_HOME` or `~/.codex/` | Codex config, sessions, Skills |
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

- Product scope: Claude Code + Claude Desktop + Codex only (no Grok / Gemini / …)
- Session “resume” copies a command; it does not launch a terminal
- No auto-merge of remote sync conflicts; no team sharing
- Claude Code and Desktop provider lists and active selections stay independent

---

## References and acknowledgements

Independent project; not affiliated with the repositories below or with Anthropic / OpenAI. Licenses in the table describe **those upstream projects**; AI-Switcher source remains under this repo’s [MIT](LICENSE). When quoting or porting upstream code, follow the corresponding license and copyright notices.

| Project | Area referenced | Upstream |
|---|---|---|
| AI Toolbox | Multi-tool config, sessions, desktop IA | [coulsontl/ai-toolbox](https://github.com/coulsontl/ai-toolbox) MIT |
| cc Proxy | Desktop local proxy and model replacement | [arhsis/cc-proxy](https://github.com/arhsis/cc-proxy) |
| CC Switch | Provider switching, Tauri, sessions, tray | [farion1231/cc-switch](https://github.com/farion1231/cc-switch) MIT |
| Claude Code VS Code Chinese pack | Extension discovery and localization flow | [zstings/claude-code-zh-cn](https://github.com/zstings/claude-code-zh-cn) MIT |
| Claude Code Chinese plugin | CLI localization install / restore | [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) |
| Claude Desktop Chinese patch | Install discovery and language packs | [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn) |
| Code Switch | Local proxy, failover, Codex config | [daodao97/code-swtich](https://github.com/daodao97/code-swtich) Apache-2.0 |
| Codex++ | Codex API writes and session sync | [BigPizzaV3/CodexPlusPlus](https://github.com/BigPizzaV3/CodexPlusPlus) AGPL-3.0 |
