# Testing

Layered checks. L0/L1 run in CI. L2 is a Windows-only release regression harness and **must not** write the real `~/.claude` / `~/.claude-switcher`.

```
L0  cargo test --lib          unit + DAO (skip L1 module)
L1  cargo test --lib system_test -- --test-threads=1
L2  pnpm system:test          debug exe + CDP IPC (not in CI)
```

## Isolation

- `AISW_TEST_HOME` rewrites `get_home_dir()` so `.claude-switcher` and `.claude` nest under that folder.
- Production ignores it unless `AISW_ALLOW_TEST_HOME=1`.
- L2 also sets `CODEX_HOME`, `OPENCODE_CONFIG`, `DSH_HOME`, `PI_CODING_AGENT_DIR`, `AISW_SMART_GATEWAY_PORT`, `AISW_PROXY_PORT_BASE` so listeners stay off 15821–15828.
- L2 使用独立 `WEBVIEW2_USER_DATA_FOLDER`；仅在测试隔离开关启用且 `AISW_TEST_HOME` 为绝对路径时跳过单实例锁，不再关闭安装版。DOM 场景不接受空白页或浏览器错误页作为成功。
- Isolated launches skip HKCU autostart migration and Deep Link registration.
- 隔离模式的 provider keyring 使用 `com.claude-switcher.provider.test.<HOME 哈希>`，无 HOME 的单元测试使用进程随机 namespace；不读写正式 `sgw_*`。测试 Key 仍在 OS 凭据库，不落入 SQLite 或日志。
- Desktop AppData 与 Pi 默认目录跟随隔离 HOME；禁止测试回退到宿主 APPDATA 或 WSL 探测。隔离模式忽略 `data-root.json` 指针。
- 全新库不自动创建 T2 绑定；旧库升级仍保留 Schema 34 既定迁移行为，两条路径分别回归。
- Windows 自启单元测试使用 `cfg(test)` 随机注册表子树，并在结束时清理，不访问正式 HKCU Run / StartupApproved。
- Windows `--lib` test EXEs embed `src-tauri/windows-common-controls.manifest` (comctl32 v6). Without it, tao's `TaskDialogIndirect` import fails at process start (`STATUS_ENTRYPOINT_NOT_FOUND`).
- L1 protocol scenarios (`sg_p0_protocol_responses_roundtrip`) run against ephemeral local loopback mock servers (`127.0.0.1:0`) with isolated HOME and zero real upstream network access, strictly validating Responses ↔ Chat Completions / Gemini turn unification (commentary + multiple tool calls) and mid-session reminders.

## Commands

| Command | What |
| --- | --- |
| `pnpm system:test:rust` | L1 Rust scenarios |
| `pnpm system:test` | L2: isolated HOME + WebView → debug exe → CDP IPC/DOM → `clean:dev`（保留安装版） |
| `pnpm system:test -- -Scenario SG-regress-no-autobind` | one L2 scenario |
| `.\scripts\system-test\run.ps1 -SkipBuild` | reuse the current debug exe (pnpm's extra `--` is ignored) |
| `pnpm system:test -- -ClaudeCode` | optional `claude -p --bare` hop check |
| `pnpm system:test -- -KeepHome` | keep the temp home on success |

On L2 failure the temp home is copied to `scripts/system-test/artifacts/`.

CDP `Runtime.evaluate` often sends `Origin: null`, which Tauri 2 rejects (`Origin header is not a valid URL`). `cdp-invoke.mjs` intercepts Fetch and rewrites Origin to the page origin or `https://tauri.localhost`. IPC-only scenarios may run against a browser-error document as long as `__TAURI_INTERNALS__.invoke` exists; DOM scenarios must verify a rendered application page and cannot treat `chrome-error://` or a blank document as success.

Coverage IDs: [`scripts/system-test/matrix.md`](../scripts/system-test/matrix.md). Smart-gateway acceptance: [`docs/smart-gateway/acceptance.md`](smart-gateway/acceptance.md).

## When to run L2

Change `live_sync` / `gateway_binding` / Claude Code write paths, then run `pnpm system:test` (or the matching scenario). Do not use the user's production `app.db`.
