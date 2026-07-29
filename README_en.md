# AI-Switcher

> A local configuration, provider, and utility manager for Claude Code, Claude Desktop, and Codex.

[中文](README.md)

AI-Switcher is a desktop application built with Tauri 2, Rust, and React. It brings together configuration files, operating-system credentials, and local tooling in one interface while keeping Claude Code and Claude Desktop providers and active configurations independent.

The application works locally by default. API keys are stored in the operating system credential store, configuration writes are backed up, and the Session Manager only reads local Claude Code session files.

## Features

- **Provider management**: Manage third-party APIs, model mappings, import/export, connection tests, model discovery, and official-login restoration independently for Claude Code, Claude Desktop, and Codex. Codex providers are written directly to `~/.codex/config.toml` and do not use the Claude local proxy.
- **Local proxy**: Anthropic Messages-compatible proxying, model mapping, credential injection, streaming forwarding, runtime status, and request logs. Opt-in automatic failover temporarily opens a provider circuit for 60 seconds after two consecutive transient failures; it is disabled by default.
- **MCP, Prompts, and Skills**: Maintain MCP servers, including Codex synchronization, manage `CLAUDE.md` presets, and install Skills from GitHub or local ZIP files with recorded provenance and manual update checks.
- **Session Manager**: Browse, filter, and search local JSONL sessions for Claude Code and Codex; Codex sessions are discovered under `~/.codex/sessions`.
- **Localization hub**: Manage Claude Code CLI localization, VS Code/Cursor patch helpers, and Claude Desktop language packs separately; applying an editor patch always requires editor confirmation.
- **Usage dashboard**: Requests, tokens, trends, estimated cost, provider/model breakdowns, and log maintenance policies. The yearly heatmap scales to the window width and always shows the full year.
- **System integration**: Provider switching from the system tray, tray labels that follow the selected language, high-contrast light/dark/system themes, and launch at login. Cards, tables, form controls, and overlays use dynamic theme colors in the desktop app.
- **Environment and updates**: Inspect configuration paths, the installed Claude Code version, and application updates.

## Session Manager

Claude Code sessions are scanned read-only:

- Source: `~/.claude/projects/**/*.jsonl`
- The list view extracts only session ID, summary, working directory, and timestamps
- Message contents are read only when details or full-content search are requested
- Every source path is validated against the allowed session root
- Browsing and search never modify original sessions; users may explicitly export one or move it to the AI-Switcher trash for restoration

Claude Desktop does not publish a stable local session-enumeration format. This release only detects its local data directory and provides the official `claude://claude.ai/new` entry point; it does not parse Chromium caches or call private APIs. A known conversation ID can be opened with Anthropic's documented [Claude Desktop deep-link format](https://support.claude.com/en/articles/14729294-open-claude-desktop-with-a-link).

## Installation

Download the NSIS installer from [GitHub Releases](https://github.com/flylink-code/AI-Switcher/releases/latest). The installed executable is named `AISwitcher.exe`.

Requirements:

- Windows 10/11
- Claude Code or Claude Desktop as needed
- Source development requires Node.js 20+, pnpm 9+, Rust stable (MSVC), and the Visual Studio 2022 Desktop development with C++ workload

## Run from source

```powershell
pnpm install
pnpm tauri dev
```

If the current terminal does not have the MSVC environment:

```powershell
scripts\tauri-msvc.bat dev
```

## Build

Build scripts run the complete Rust test suite before compiling the application. Build a release executable without installers:

```powershell
scripts\build-exe.bat
# or
pnpm build:exe
```

Build a debug executable:

```powershell
scripts\build-exe.bat debug
# or
pnpm build:exe:debug
```

Build MSI and NSIS installers:

```powershell
scripts\build-exe.bat bundle
# or
pnpm build:exe:bundle
```

For a faster local build that skips tests:

```powershell
scripts\build-exe.bat release skip-tests
# or use the PowerShell entry point directly
.\scripts\build-exe.ps1 -SkipTests
```

The scripts automatically detect Visual Studio 2022 Community, Professional, Enterprise, or Build Tools and fall back to `corepack pnpm` when a global `pnpm` shim is unavailable.

Main outputs:

| Artifact | Path |
|---|---|
| Tauri release executable | `src-tauri\target\release\AISwitcher.exe` |
| Release test copy | `release\AISwitcher.exe` |
| Debug test copy | `release\AISwitcher-debug.exe` |
| Installers | `src-tauri\target\release\bundle\` |

## Data and configuration

| Path | Purpose |
|---|---|
| `~/.claude/settings.json` | Active Claude Code provider configuration |
| `~/.claude.json` | Claude Code MCP and project configuration |
| `~/.claude/projects/` | Local Claude Code sessions, read-only |
| `%LOCALAPPDATA%\Claude-3p\configLibrary\` | Claude Desktop third-party gateway profiles |
| `~/.claude/skills/` | Claude Code Skills |
| `~/.claude-switcher/` (default) or a directory selected on Environment | AI-Switcher-managed data library: database, backups, downloaded resources, and logs |

The product is now AI-Switcher while the application identifier, signing key, and default data location are retained for compatibility. The data library can be copied to another drive; every copied file is SHA-256 verified, the old copy is retained, and the new location is used after restart. Claude live configuration remains at its official location.

Environment can export a versioned portable-library ZIP containing a sanitized database snapshot, Skills, Skill provenance, and session archives with a SHA-256 manifest. API keys, OS credentials, Claude sign-in state, passwords, and private keys are excluded from both export and WSL/SSH pushes. Sync always shows a preview first; confirmation writes only an archive to the target's `incoming/` directory and never overwrites its live configuration.

## Security and privacy

- API keys are stored through Windows Credential Manager, macOS Keychain, or Linux Secret Service.
- Configuration files use atomic writes with rotating pre-write backups.
- The Session Manager reads local JSONL files only, creates no full-text database, and rejects paths outside the session root.
- Sessions may contain source code, credentials, or other sensitive data. Review content before copying or sharing it.
- Except for provider tests, model discovery, update checks, user-requested downloads, and a user-confirmed WSL/SSH archive push, the application does not upload local content.

## Project layout

```text
src/                         React, Ant Design, Zustand, and i18next frontend
src/pages/SessionsPage.tsx   Local session list and details
src-tauri/src/               Rust backend, configuration, proxy, database, and tray
src-tauri/src/session_manager.rs
                             Read-only session adapter and path validation
scripts/                     Windows development and build scripts
```

## References and acknowledgements

This project drew product and implementation ideas from the following open-source projects. AI-Switcher is independent and is not affiliated with these projects or with Anthropic.

| Project | Area referenced | Upstream and license |
|---|---|---|
| AI Toolbox | Multi-tool configuration, sessions, and desktop information architecture | [coulsontl/ai-toolbox](https://github.com/coulsontl/ai-toolbox), MIT |
| cc Proxy | Claude Desktop local proxying and model replacement | [arhsis/cc-proxy](https://github.com/arhsis/cc-proxy), subject to its repository license terms |
| CC Switch | Provider switching, Tauri architecture, session parsing, and tray interactions | [farion1231/cc-switch](https://github.com/farion1231/cc-switch), MIT |
| Claude Code for VS Code Chinese Pack | VS Code extension discovery, localization rules, backup, and restoration | Local reference: `examples/claude-code-vscode-zh-cn`; [zstings/claude-code-zh-cn](https://github.com/zstings/claude-code-zh-cn), MIT |
| Claude Code Chinese Localization Plugin | Claude Code CLI localization installation, update, and restoration | Local reference: `examples/claude-code-zh-cn`; [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn), subject to its repository license terms |
| Claude Desktop Chinese Patch | Desktop discovery, language-pack validation, and recovery | Localization repository: [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn), subject to its repository terms |
| Code Switch | Local proxying, failover, and Claude Code/Codex configuration | [daodao97/code-swtich](https://github.com/daodao97/code-swtich), Apache-2.0 |

When quoting, porting, or redistributing code from these projects, follow the corresponding upstream license and copyright notices.

## Current limitations

- Session resume copies a command; it does not launch a terminal or execute commands. Moving to trash first creates a verified archive.
- Remote archives are not auto-imported, remote conflicts are not merged, and team sharing is not included.
- Claude Desktop history is not parsed through private formats while no stable official interface exists.
- Claude Code and Claude Desktop provider lists, active selections, and live configurations remain independent.
