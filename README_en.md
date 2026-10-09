# AI-Switcher

Local configuration and provider manager for **Claude Code**, **Codex**, **OpenCode**, **Pi**, and **Cline**. **v1.5.17-beta.1 (local test build)**

**Beta testing (not a stable release):** A unified provider pool shared by all agents, with smart gateway, direct, and official connection modes. This installer is for local testing; GUI and live upstream acceptance checks remain pending. The latest stable release is still v1.5.16.

**Upgrade note:** This development build migrates the main database to Schema 34, merging legacy providers while retaining their rows. A consistent SQLite backup is created as `app.db.v33.bak` before migration. Older builds cannot open Schema 34: exit the new build and restore the backup or rollback export before downgrading. Back up `~/.claude-switcher` separately as well. 1.4.xx hotfixes stay on `release/1.4.x` without the new schema.

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

- **Providers:** One global upstream pool; API keys and OAuth accounts no longer need configuring per agent. Includes presets, model discovery, connection tests, quotas, and metadata import/export. **Agent connections** offer **Smart gateway (recommended)**, **Direct (one selected upstream)**, or **Official**. Direct Claude Code requires Anthropic; direct Codex requires OpenAI Chat / Responses. Protocol conversion and Codex OAuth use the gateway. OpenCode / Pi / Cline write only the selected managed entry and preserve user-owned configuration.
- **Smart gateway:** Standalone listener at `127.0.0.1:15828` for mode routing, thinking levels, catalog scope, and condition rules. Binding an agent writes an Auto card that points at that port. Usage counts only the innermost hop. Entry: main nav **Gateway**.
- **Antigravity gateway:** `127.0.0.1:15830` exposes Cloud Code as Anthropic Messages / OpenAI Chat / Responses. Browser OAuth account pool with quota-aware scheduling. Personal use; review upstream terms yourself.
- **Kiro gateway:** `127.0.0.1:15831` with an account pool and quota-aware scheduling. Import a Builder ID, Social, or Kiro IDE token, or sign in with a device code or Social PKCE. Usage counts only the Kiro hop. Personal use; review upstream terms yourself.
- **Workspace:** MCP, prompts, skills, agents, plugins, project snapshots — tabs filtered by the current agent.
- **Sessions & usage:** Browse, search, and back up local sessions; estimate cost from proxy logs plus session events.
- **Tools & localization:** Install/update each agent CLI. Claude Code and VS Code/Cursor Chinese packs (compare against GitHub latest; install or remove).

## Paths

| | |
| --- | --- |
| Claude Code | `~/.claude/` |
| Claude Desktop | `%LOCALAPPDATA%\Claude-3p\` (Windows); **hidden from the UI**, backend and existing cards remain |
| Codex | `$CODEX_HOME` or `~/.codex/` |
| OpenCode | `~/.config/opencode/` · `~/.local/share/opencode/` |
| Pi | `~/.pi/agent/` |
| DSH | `~/.dsh/`; **hidden from the UI**, backend and existing cards remain |
| Cline | `~/.cline/` (sidecar `ai-switcher.json`; Auto for gateway mode, the selected upstream for direct mode) |
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

- The product surface is the five agents above; Desktop / DSH retain compatibility backends but remain hidden from the UI.
- Pi / DSH / Cline have no plugins, agents, profiles, or tray switching.
- New direct connections do not start protocol-conversion proxies. Legacy T1 cards that depended on a local proxy retain their existing path; explicitly switching to the smart gateway is recommended.
- No remote conflict merge, no team sharing.
- Linux is Ubuntu 22.04 / Debian 12+ only.

## License & thanks

[MIT](LICENSE). Independent community project — not affiliated with Anthropic, OpenAI, or Google. Claude, Codex, ChatGPT, and related names are trademarks of their owners. Issues and PRs welcome.

Inspiration: [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) · [free-claude-code](https://github.com/Yeachan-Heo/free-claude-code) · [sub2api](https://github.com/sub2api) · [AI Toolbox](https://github.com/coulsontl/ai-toolbox) · [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) · [cc-switch](https://github.com/farion1231/cc-switch) · [code-switch](https://github.com/daodao97/code-swtich) · [Codex++](https://github.com/BigPizzaV3/CodexPlusPlus)

Localization: [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) · [shanjiancaofu/claude-code-vscode-zh-cn](https://github.com/shanjiancaofu/claude-code-vscode-zh-cn) · [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn)
