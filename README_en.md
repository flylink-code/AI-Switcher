# AI-Switcher

Local AI configuration, unified provider pool, and smart gateway workbench for **Claude Code**, **Claude Desktop**, **Codex**, **VS Code Copilot**, **OpenCode**, and **Pi**. **v1.6.6**

## Project Outlook

After releasing **v1.6.6**, the author is moving primarily to [Magpie](https://github.com/yetone/magpie), so AI-Switcher will not receive new features. **v1.6.6 is the final feature release and this repository is archived (read-only)**; the code, discussions, and existing release installers stay available for use, reference, and self-maintenance. To make changes, fork the repository and publish your own builds; continued updates are not guaranteed. Thank you for your usage, feedback, and support.

## What's New in v1.6.6

- Added a tray Quick Panel: left-click the tray icon to switch Claude Code / Codex between Smart Gateway and Official modes and view today's usage and gateway status. Includes focus-lost auto-hide, one-click main-window access, and tray double-click activation.
- Added per-model OpenAI Priority fast mode with individual and batch controls for upstream models. Explicit client tiers take precedence; availability and pricing depend on the provider.

Every agent shares a single global upstream pool (`upstreams`) with unified connection modes: **Smart Gateway (recommended)**, **Direct**, or **Official**. Built-in Antigravity and Kiro reverse-proxy gateways run in an isolated Sidecar child process (`--gateway-sidecar`) by default with automatic supervision.

**Upgrade & Refactoring Notes:**
- **Database Migration:** The main database migrates to **Schema 37**. Upgrading from 1.5.16 (Schema 33) automatically creates `app.db.v33.bak` first. Older versions cannot open the new schema: quit the new build and restore the backup before downgrading. Backing up `~/.claude-switcher` separately is also recommended. 1.4.xx hotfixes stay on `release/1.4.x`.
- **Navigation & Architecture Refactoring:** Primary navigation is streamlined into 7 tabs (**Overview / Agent / Providers / Gateways / Usage / Workspace / Settings**) with switchable top-bar (default) and sidebar layouts, unifying the global upstream pool, smart gateway catalog, VS Code Copilot managed models, and sidecar reverse proxies.

[中文](README.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [MIT](LICENSE)

Built with Tauri 2 + Rust + React. Consolidates scattered agent config files, OS credentials, OAuth account pools, local sessions, and token billing into a single desktop interface. Runs locally by default: API keys live in the OS credential store, config writes are backed up atomically, and session data is read from local files.

| Platform | Installer | Notes |
| --- | --- | --- |
| Windows 10/11 | NSIS `.exe` (recommended) / MSI | Full features (including Claude Desktop management & localization) |
| Linux (preview) | AppImage / `.deb` | Ubuntu 22.04 / Debian 12+ (WebKitGTK 4.1) |

## Quick Start

1. Download the installer from [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest). Prefer the NSIS `.exe` on Windows (per-user install, usually no admin required); on Linux use the AppImage (`chmod +x` first).
2. Open the **Agent** page to detect, install, or upgrade agent CLIs (**Node.js ≥ 22** required).
3. Add API key upstreams under **Providers** (presets, model discovery, and connectivity checks included), or sign in to **Antigravity / Kiro / ChatGPT (Codex OAuth)** account pools under **Gateways**.
4. Return to the **Agent** page and switch each agent to **Smart Gateway** or a selected **Direct** upstream.

## Core Features

- **Agent Hub (`Agent` page):**
  - Manage connection status and CLI versions for **Claude Code**, **Claude Desktop**, **Codex**, **VS Code Copilot**, **OpenCode**, and **Pi** (with one-click in-card CLI upgrades).
  - **Connection Modes:** Choose **Smart Gateway (recommended)**, **Direct (single selected upstream)**, or **Official**. Direct Claude Code requires Anthropic; direct Codex requires OpenAI Chat / Responses; cross-protocol routing and Codex OAuth run through the Smart Gateway.
  - **VS Code Copilot Integration:** Writes smart gateway models into VS Code's native `chatLanguageModels.json` (managing only the `AI-Switcher` entry while preserving official models and user-defined providers), with full multi-tier reasoning effort (`low` / `medium` / `high`) support for Gemini, Claude, and GPT models.
  - **Advanced Settings & Localization:** Configure direct role-model mappings for Claude Code / Desktop, web search and output effort for Codex, and one-click Chinese localization packs for Claude Code, VS Code / Cursor extensions, and Claude Desktop.
- **Global Providers & Smart Gateway (`Providers` page · `127.0.0.1:15828`):**
  - **Model Context Metadata:** Model refresh preserves upstream input/output limits and supports sub2api's Codex manifest. Windows remain provider-specific, manual overrides are preserved, and Codex default and maximum windows stay separate. After upgrading, refresh upstream models and restart Code/Codex to load the new catalog; missing metadata retains the existing fallback.
  - **Single Upstream Pool:** Configure API keys and OAuth pools once and share across all agents. Supports drag/up-down priority ordering (`sort_index`), per-model toggles, quota checks, and key-free metadata import/export.
  - **Automatic Disambiguation:** Models from a single upstream keep clean bare IDs; when multiple upstreams provide the same model name, catalog IDs are automatically prefixed by provider (`<provider>.<model>`) for exact routing.
  - **Cross-Protocol Bridging & Resilience:** Bidirectional Anthropic Messages ↔ OpenAI Chat / Responses conversion, immediate SSE handshake on `/v1/responses`, long-context thinking stream keep-alive, and `localhost:15828` loopback normalization so Codex bypasses Windows system proxies cleanly.
- **Reverse-Proxy Gateways & Account Pools (`Gateways` page):**
  - **Sidecar Process Supervision:** Antigravity and Kiro reverse proxies run in an isolated child process (`--gateway-sidecar`) by default; runtime mode and live health can be managed in **Settings → General**.
  - **Antigravity Gateway (`127.0.0.1:15830`):** Browser OAuth account pool with quota-aware scheduling, exposing Google Cloud Code as standard Anthropic Messages / OpenAI Chat / Responses endpoints.
  - **Kiro Gateway (`127.0.0.1:15831`):** Import Builder ID, Social PKCE, or Kiro IDE tokens (or sign in via device code), featuring full AWS EventStream frame decoding, connect-timeout retry, and automatic account rotation.
  - **ChatGPT Account Pool (Codex OAuth):** Multi-account sign-in and default-account switching, exposed as a built-in upstream for the Smart Gateway.
- **Workspace (`Workspace` page):**
  - Select an agent at the top to filter supported resource tabs: **Sessions** (first tab), **Project Profiles**, **MCP Servers**, **Prompts**, **Skills**, **Subagents**, and **Plugins**.
- **Usage & Observability (`Usage` & `Overview` pages):**
  - **Multi-Hop Deduplication:** Correlated request chains (`correlation_id`) count only the innermost billing hop (`kiro` / `antigravity` over `smart_gateway`), and `count_tokens` probes are excluded from billing.
  - **Unified Provider Breakdown:** Built-in reverse proxies and gateway aliases automatically fold into canonical provider names (`Antigravity`, `Kiro`, `ChatGPT`), combining live proxy logs with incremental local session sync (Claude Code / Codex / OpenCode / Pi), multi-currency USD conversion, and Excel pricing import/export.

## Local Ports & Paths

### Local Listeners (`127.0.0.1` / `localhost` only)

| Port | Service | Description |
| --- | --- | --- |
| **15828** | Smart Gateway | Aggregated model catalog & cross-protocol router; Claude Code, Codex, and VS Code Copilot connect directly here |
| **15830** | Antigravity Proxy | Google Cloud Code account-pool reverse proxy (runs in Sidecar process by default) |
| **15831** | Kiro Proxy | Kiro account-pool reverse proxy (runs in Sidecar process by default) |
| **15822** | Claude Desktop Proxy | Provides `/v1/models` alias mapping and local inference probe responses for Claude Desktop |
| 15821 / 15823 / 15826 / 15827 | Legacy Local Proxies | Compatibility ports for Code / Codex / DSH / Cline |

### Configuration Paths

| Target | Path & Notes |
| --- | --- |
| Claude Code | `~/.claude/` (`settings.json`, etc.) |
| Claude Desktop | `%LOCALAPPDATA%\Claude-3p\` (Windows) |
| Codex | `$CODEX_HOME` or `~/.codex/` (`config.toml`, `auth.json`, `ai-switcher-model-catalog.json`) |
| VS Code Copilot | `%APPDATA%\Code\User\chatLanguageModels.json` (manages only the `AI-Switcher` entry) |
| OpenCode | `~/.config/opencode/` · `~/.local/share/opencode/` |
| Pi | `~/.pi/agent/` |
| Cline / DSH | `~/.cline/` (`ai-switcher.json`) · `~/.dsh/`; **T3 compatibility retained, hidden from UI** |
| App Data | `~/.claude-switcher/` (relocatable; exports and remote sync omit API keys by default) |

## Privacy & Security

API keys are stored in the OS credential store (Windows Credential Manager / macOS Keychain / Linux Secret Service). Config writes are atomic with rotating backups. No local data leaves your machine except for connection tests, model discovery, update checks, and remote sync actions you explicitly trigger.

## Development & Build

Requires Node.js 22+, pnpm 9+ (Corepack), and Rust stable; Windows also requires the VS 2022 C++ desktop workload. Default dev server port: **5250**.

```powershell
pnpm install
pnpm dev:hot                # hot reload dev; switch temporarily to 5251 if 5250 is occupied
pnpm clean:dev              # stop debug processes and restore installed release state
pnpm build:exe              # build release binary → release\AISwitcher.exe
pnpm build:exe:bundle       # build NSIS (.exe) and MSI installer bundles
```

## Scope & Limits

- The UI product surface focuses on **Claude Code, Claude Desktop, Codex, VS Code Copilot, OpenCode, and Pi**; Cline and DSH retain backend, schema, and config compatibility but are hidden from the UI.
- Antigravity / Kiro / ChatGPT OAuth account pools are for personal use only; evaluate and comply with upstream terms yourself.
- OpenCode and Pi write only the selected managed entry and do not manage plugins, subagents, or project profiles.
- No multi-device conflict merging, no LAN listener, and no shared team server.
- Linux preview supports Ubuntu 22.04 / Debian 12+ only.

## License & Acknowledgements

[MIT](LICENSE). Independent open-source community project — not affiliated with Anthropic, OpenAI, Google, GitHub, or Amazon. Claude, Codex, ChatGPT, Copilot, Gemini, and related names are trademarks of their respective owners. Issues and PRs are welcome.

Inspiration: [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) · [free-claude-code](https://github.com/Yeachan-Heo/free-claude-code) · [sub2api](https://github.com/sub2api) · [AI Toolbox](https://github.com/coulsontl/ai-toolbox) · [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) · [cc-switch](https://github.com/farion1231/cc-switch) · [code-switch](https://github.com/daodao97/code-swtich) · [Codex++](https://github.com/BigPizzaV3/CodexPlusPlus)

Localization: [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) · [shanjiancaofu/claude-code-vscode-zh-cn](https://github.com/shanjiancaofu/claude-code-vscode-zh-cn) · [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn)

