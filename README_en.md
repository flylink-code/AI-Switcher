# AI-Switcher

Local configuration and provider manager for **Claude Code**, **Claude Desktop**, **Codex**, **OpenCode**, **Pi**, **DSH**, and **Cline**. **v1.5.0**

**This release:** The smart gateway is a standalone listener on `127.0.0.1:15828` with per-app bindings, nine route modes, and condition rules. Primary nav is 7 items; local proxy is back under Settings.

[中文](README.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [MIT](LICENSE)

Tauri 2 + Rust + React. One UI for scattered config files, OS credentials, and local agent directories. Runs on this machine by default: keys go in the OS credential store, writes are backed up first, sessions are read from local files.

| Platform | Installer | Notes |
| --- | --- | --- |
| Windows 10/11 | NSIS `.exe` (recommended) / MSI | Full features |
| Linux (preview) | AppImage / `.deb` | Ubuntu 22.04 / Debian 12+ (WebKitGTK 4.1) |

## Get started

1. Download from [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest). Prefer NSIS on Windows (per-user, usually no admin). On Linux use the AppImage (`chmod +x` first).
2. **Settings → Tools & environment → Agent tools** to detect and install CLIs (**Node.js ≥22** required).
3. Add API keys under **Providers**, or sign in to Google / Antigravity under **Accounts & quotas**.

## Features

- **Providers:** Each agent chooses an **external provider connection** or a **gateway profile connection**. The gateway aggregates catalogs, plan/execute/subagent roles, and route logs. Cards copy across agents (protocol and URL rewritten). OpenCode / Pi / DSH still write every provider when external; a gateway connection writes a single loopback entry.
- **Smart gateway:** Standalone listener at `127.0.0.1:15828` for mode routing, thinking levels, catalog scope, and condition rules. Binding an agent writes an Auto card that points at that port. Usage counts only the innermost hop. Entry: main nav **Gateway**.
- **Antigravity gateway:** `127.0.0.1:15830` exposes Cloud Code as Anthropic Messages / OpenAI Chat / Responses. Browser OAuth account pool with quota-aware scheduling. Personal use; review upstream terms yourself.
- **Workspace:** MCP, prompts, skills, agents, plugins, project snapshots — tabs filtered by the current agent.
- **Sessions & usage:** Browse, search, and back up local sessions; estimate cost from proxy logs plus session events. Claude Desktop’s private history is not parsed.
- **Tools & localization:** Install/update each agent CLI. Claude Code, VS Code/Cursor, and Desktop Chinese packs (compare against GitHub latest; install or remove).

## Paths

| | |
| --- | --- |
| Claude Code | `~/.claude/` |
| Claude Desktop | `%LOCALAPPDATA%\Claude-3p\` (Windows) |
| Codex | `$CODEX_HOME` or `~/.codex/` |
| OpenCode | `~/.config/opencode/` · `~/.local/share/opencode/` |
| Pi | `~/.pi/agent/` |
| DSH | `~/.dsh/` |
| Cline | `~/.cline/` (sidecar `ai-switcher.json`, proxy `:15827`) |
| This app | `~/.claude-switcher/` (relocatable; path kept for older installs) |

Exports and sync omit API keys by default.

## Privacy

API keys live in Windows Credential Manager / macOS Keychain / Linux Secret Service. Config writes are atomic with rotating backups. Nothing local is uploaded except connection tests, model discovery, update checks, and remote sync you explicitly confirm.

## Development

Node.js 22+, pnpm 9+ (Corepack), Rust stable; Windows also needs the VS 2022 C++ desktop workload. Dev port **5250**.

```powershell
pnpm install
.\scripts\dev-hot.ps1       # hot reload; uses 5251+ if 5250 is taken
.\scripts\clean-dev.ps1     # stop debug, restore the installed app
pnpm build:exe              # release exe → release\AISwitcher.exe
```

## Limits

- The seven agents above are the product surface; the AG gateway attaches Gemini / Cloud Code to them.
- Pi / DSH / Cline have no plugins, agents, profiles, or tray switching.
- Pi / DSH OpenAI-compatible upstreams go direct; Cline always uses local proxy `:15827`.
- No remote conflict merge, no team sharing.
- Linux is Ubuntu 22.04 / Debian 12+ only.

## License & thanks

[MIT](LICENSE). Independent community project — not affiliated with Anthropic, OpenAI, or Google. Claude, Codex, ChatGPT, and related names are trademarks of their owners. Issues and PRs welcome.

Inspiration: [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) · [free-claude-code](https://github.com/Yeachan-Heo/free-claude-code) · [sub2api](https://github.com/sub2api) · [AI Toolbox](https://github.com/coulsontl/ai-toolbox) · [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) · [cc-switch](https://github.com/farion1231/cc-switch) · [code-switch](https://github.com/daodao97/code-swtich) · [Codex++](https://github.com/BigPizzaV3/CodexPlusPlus)

Localization: [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) · [shanjiancaofu/claude-code-vscode-zh-cn](https://github.com/shanjiancaofu/claude-code-vscode-zh-cn) · [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn)
