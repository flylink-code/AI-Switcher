# AI-Switcher

> Local configuration and provider manager for **Claude Code**, **Claude Desktop**, **Codex**, **OpenCode**, **Pi CLI**, **DSH**, and **Cline**. **v1.3.24**

**This release:** Cline usage / sessions / workspace parity; Antigravity gateway Wave 1; local `web_search` / `web_fetch`; WSL Direct; WebDAV library backup; About-page release notes and a single-column local-proxy layout.

[中文](README.md) · [Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) · [License: MIT](LICENSE)

Built with **Tauri 2 + Rust + React**. Consolidates scattered configuration files, system credentials, and local tooling into a unified interface, supporting independent provider profiles, unified model catalogs, and local reverse proxy scheduling.

Runs locally by default: API keys are stored in the OS credential store, configuration writes are backed up automatically, and sessions are read directly from local JSONL.

| Platform | Installer | Requirements |
|---|---|---|
| Windows 10/11 | NSIS `.exe` (Recommended) / MSI | Full feature support |
| Linux (Preview) | AppImage / `.deb` | **Ubuntu 22.04 / Debian 12+** (WebKitGTK 4.1) |

---

## Key Features

### 1. Unified Navigation & Workspace
- **Dual Layout Modes**: Toggle between top navigation tabs and a classic vertical sidebar (`cs.layoutMode`).
- **Seven Core Modules**: Overview · Providers · Usage · Accounts & Quotas · Workspace · Sessions · Settings.
- **Agent Toolbox**: Detect and install/update Claude Code, Codex, OpenCode, Pi, and DeepSeek Harness CLI tools (requires local Node.js ≥ 22).

### 2. Provider Management & Unified Model Catalog
- **Multi-Agent Configurations**: Independently manage API endpoints, credentials, model mappings, and Base URL latency diagnostics for Claude Code, Desktop, Codex, OpenCode, Pi, DSH, and Cline. Cline uses the local proxy on port **15827** (`~/.cline/ai-switcher.json`); usage is filtered from proxy logs; sessions scan `~/.cline/data`.
- **Unified Catalog Mode**: Merge multi-provider models through a local proxy so any visible model can be switched seamlessly in the CLI via `/model`.
- **Subagent Smart Routing**: Claude Code (Explore/Haiku) and Codex subagents follow the current default model when left empty; an explicit catalog subagent id is still honored.
- **Thinking / Reasoning Translation**: Seamlessly bridges token budgets and reasoning effort across Anthropic, OpenAI, Gemini, and DeepSeek.
- **Health Diagnostics & Failover**: One-click quarantine for 401/403 errors with transparent failover support on 429/5xx errors.
- **Copy across Agents**: Protocol, Base URL, and Claude role mappings are rewritten for the destination (e.g. Kimi Anthropic → Codex Chat `/v1`). Copies onto Code / Desktop / Codex become current and update live config.

### 3. Antigravity Gateway (Smart Cloud Code Proxy)
Built-in local reverse proxy (default `http://127.0.0.1:15830`) bridging Google Cloud Code to Anthropic Messages and OpenAI Chat/Responses protocols:
- **Smart Account Pool Scheduling**: Browser OAuth import, real-time quota probes, dynamic weighted round-robin, and best account recommendation. Active account is a soft preference—under RPM pressure it yields to healthier accounts.
- **URL-Level 429 Intelligent Fallback**: Accurately recognizes node-level rate limits (`Resource has been exhausted`) and automatically falls back to production endpoints with micro-backoff; remembers daily-host throttling with TTL and skips daily when hot.
- **Graceful Tier Degradation**: Downgrades across Gemini 3.6/3.7 tiers (`3.7-flash-low` → `3.6-flash-low`, then same-base `medium`/`high` as a last resort; at most three tiers per request); per-account token bucket (~30 RPM, burst 8), min-interval backoff, AIMD on 429. Main-session stream retry budget is ~45s (subagent ~15s); a full concurrency slot or deadline returns **429** instead of bypassing the gate. Local network failures are logged as `network` and returned as **504 + Retry-After** (not 502), without cooling accounts. `generate` is bound to the remaining deadline and skips sandbox after earlier host network failures. **Accounts & Quotas** exposes manual concurrency and rate-limit settings; subagent bursts use a separate pool via `x-cs-subagent`.
- **Protocol Adaptations**: OpenAI Chat / Responses replay Gemini 3 `thought_signature` on historical `functionCall` parts (tool-id cache, sentinel fallback) so Codex tool rounds are not rejected with 400; request-body 400/422 is returned as-is instead of being laundered into 429/502. Complete-line UTF-8 decoding to prevent character corruption, LaTeX KaTeX text unwrap, and real token usage passthrough.

### 4. Extensions & Ecosystem Integration
- **MCP / Prompts / Skills**: Centrally manage MCP servers, prompt templates (`CLAUDE.md` / `AGENTS.md`), and Skill repositories with cross-agent synchronization. Check-all Skill/plugin updates run off the UI thread (per-repo GitHub zips, 90s plugin CLI timeout) so the window stays responsive.
- **Local Session Manager**: Browse, search, backup, and export local session histories across all supported AI coding agents.
- **Localization**: Built-in Claude Code Chinese plugins, desktop language packs, and editor patch tools.

---

## Installation & Getting Started

1. Download the latest installer from [GitHub Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) (NSIS `.exe` recommended for Windows).
2. Go to **Settings → Tools & environment → Agent tools** to verify and install the required agent CLI environments.
3. Configure your API keys in **Providers** or log in with your Google account in **Accounts & Quotas** to get started.

---

## Configuration & Storage Paths

| Client / Component | Configuration & Storage Path |
|---|---|
| Claude Code | `~/.claude/settings.json` · `~/.claude.json` · `~/.claude/projects/` |
| Claude Desktop | `%LOCALAPPDATA%\Claude-3p\configLibrary\` (Windows) |
| Codex CLI | `$CODEX_HOME/` or `~/.codex/` (`config.toml`, `sessions/`) |
| OpenCode | `~/.config/opencode/opencode.json` · `~/.local/share/opencode/` |
| Pi CLI | `~/.pi/agent/models.json` · `auth.json` · `sessions/` |
| DeepSeek Harness | `~/.dsh/settings.yaml` · `.credentials.yaml` · `sessions/` |
| Cline | `~/.cline/ai-switcher.json` · `data/settings/cline_mcp_settings.json` · `data/sessions/` |
| Application Library | `~/.claude-switcher/` (relocatable; stores DB, backups, and logs) |

---

## Development

**Prerequisites**: Node.js 22+, pnpm 9+, Rust stable, VS 2022 C++ desktop workload (Windows).

```powershell
# 1. Install frontend dependencies
pnpm install

# 2. Start dev server with hot reload
.\scripts\dev-hot.ps1

# 3. Build release executable
pnpm build:exe
```

---

## License & Acknowledgements

Released under the **[MIT License](LICENSE)**.

AI-Switcher is an independent community project and is not affiliated with Anthropic, OpenAI, or Google.

**Special Thanks & Upstream Inspiration**:
- [Antigravity-Manager](https://github.com/lbjlaq/Antigravity-Manager) - Reverse proxy scheduling & protocol mapping
- [free-claude-code](https://github.com/Yeachan-Heo/free-claude-code) - Background request short-circuit, stream lifetime, local web_search
- [sub2api](https://github.com/sub2api) - URL-level rate limiting & upstream fallback
- [AI Toolbox](https://github.com/coulsontl/ai-toolbox) · [CLIProxyAPI](https://github.com/router-for-me/CLIProxyAPI) · [cc-switch](https://github.com/farion1231/cc-switch) · [code-switch](https://github.com/daodao97/code-swtich)
- Localization: [taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn) v2.13.0 · [shanjiancaofu/claude-code-vscode-zh-cn](https://github.com/shanjiancaofu/claude-code-vscode-zh-cn) v0.1.2 · [javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn) 1.4.6
