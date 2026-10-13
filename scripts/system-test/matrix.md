# System-test coverage matrix

IDs align with [`docs/smart-gateway/acceptance.md`](../../docs/smart-gateway/acceptance.md) plus the 1.5.4 live-config regressions and the 1.5.5 Responses bridge regression.

| ID | Layer | What it asserts |
| --- | --- | --- |
| `SG-isolate-home` | L2 | `get_paths` / live files stay under `AISW_TEST_HOME` |
| `SG-regress-no-autobind` | L1+L2 | `set_binding_profile` on an unbound target errors; no `gateway_bindings` row |
| `SG-regress-independent-not-stolen` | L1+L2 | Independent Code current + `update_route_mode` does not write `:15828` |
| `SG-regress-auto-current-writes-gateway` | L1+L2 | Bind + Auto current writes gateway URL + discovery; leaving Auto clears it |
| `SG-p0-simulate-default-mode` | L1+L2 | `simulate_gateway_route` (`claude.auto`) uses the default mode, not `gpt-6-astra` |
| `SG-p0-catalog-bind-appends-auto` | L1+L2 | OpenCode bind writes Auto as only managed entry; legacy direct kept in DB but not in live config |
| `SG-p0-agent-proxy-hop` | L1 (+ L2 optional CLI) | Same `correlation_id` keeps innermost hop; live L2 checks `agent_proxy` when a request exists |
| `SG-p0-protocol-responses-bridge` | L1 | Responses→Chat unified assistant (text+tool_calls un-split) to loopback mock; AG Responses→Gemini single model content + mid-session reminder |
| `SG-providers-pool-crud` | L2 | Global upstream pool CRUD, export/import JSON, deletion rejection when referenced by direct (IPC-level verification) |
| `SG-agent-connection-card` | L2 | Agent connection transitions (official/direct/gateway), profile selection rejection in direct/unbound, direct without :15828, deletion safety (IPC-level verification) |
| `SG-gateway-ui-reliability` | L2 DOM + IPC | 真实点击档案备用方式，核对仅修改所选档案、不抢占 direct/不隐式绑定；连接卡显示隔离端口与停启状态；Code/Codex 漂移检测、过期预览拒绝与显式重新应用 |
| `SG-upstream-limits-ui` | L2 DOM + IPC | 无凭据上游的限额弹窗：真实键盘输入、保存重开、Escape 关闭、焦点返回及截图；不发送上游请求 |
| `SG-codex-cli-optional-ui` | L2 DOM | 页面级 stub `get_codex_cli_version` 为未安装：Codex 卡片仍显示连接模式与高级设置，显示「CLI 未安装（可选）」，无主「安装」按钮，维护行为「安装 CLI」且无「升级」 |
| `SG-localization-agentcards-ui` | L2 DOM | 页面级 stub Claude Code / Desktop 为已安装：两张卡的高级设置里各有一行「中文化」，默认只显示状态与「查看详情」，不请求上游版本；展开后才出现宿主版本并请求上游，可再收起；设置页不再有汉化入口 |
| `SG-ui-layout-matrix` | L2 DOM | 顶栏/侧栏 × 深浅色 × 中英文的概览、供应商、网关导航与截图；文档横向溢出检查、拓扑 reduced-motion 计算样式 |
| `SG-A-bind-code` | L2 (via auto-current) | `bind_smart_gateway` for Claude Code |
| optional `claude -p --bare` | L2 `-ClaudeCode` | Real CLI against isolated URL; hop is `agent_proxy` or `smart_gateway` |

## P1 / P2 (not automated here)

- Antigravity 429 Retry-After, usage dashboard currency, MCP/Skill GitHub, ZIP import, 7-item nav visual smoke.

## Commands

- L1: `pnpm system:test:rust` → `cargo test --lib system_test -- --test-threads=1`
- L2: `pnpm system:test` (Windows; not in CI)
