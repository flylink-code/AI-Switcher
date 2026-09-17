# System-test coverage matrix

IDs align with [`docs/smart-gateway/acceptance.md`](../../docs/smart-gateway/acceptance.md) plus the 1.5.4 live-config regressions and the 1.5.5 Responses bridge regression.

| ID | Layer | What it asserts |
| --- | --- | --- |
| `SG-isolate-home` | L2 | `get_paths` / live files stay under `AISW_TEST_HOME` |
| `SG-regress-no-autobind` | L1+L2 | `set_binding_profile` on an unbound target errors; no `gateway_bindings` row |
| `SG-regress-independent-not-stolen` | L1+L2 | Independent Code current + `update_route_mode` does not write `:15828` |
| `SG-regress-auto-current-writes-gateway` | L1+L2 | Bind + Auto current writes gateway URL + discovery; leaving Auto clears it |
| `SG-p0-simulate-default-mode` | L1+L2 | `simulate_gateway_route` (`claude.auto`) uses the default mode, not `gpt-6-astra` |
| `SG-p0-catalog-bind-appends-auto` | L1+L2 | OpenCode bind appends Auto and keeps the independent provider |
| `SG-p0-agent-proxy-hop` | L1 (+ L2 optional CLI) | Same `correlation_id` keeps innermost hop; live L2 checks `agent_proxy` when a request exists |
| `SG-p0-protocol-responses-bridge` | L1 | Responses→Chat unified assistant (text+tool_calls un-split) to loopback mock; AG Responses→Gemini single model content + mid-session reminder |
| `SG-A-bind-code` | L2 (via auto-current) | `bind_smart_gateway` for Claude Code |
| optional `claude -p --bare` | L2 `-ClaudeCode` | Real CLI against isolated URL; hop is `agent_proxy` or `smart_gateway` |

## P1 / P2 (not automated here)

- Antigravity 429 Retry-After, usage dashboard currency, MCP/Skill GitHub, ZIP import, 7-item nav visual smoke.

## Commands

- L1: `pnpm system:test:rust` → `cargo test --lib system_test -- --test-threads=1`
- L2: `pnpm system:test` (Windows; not in CI)
