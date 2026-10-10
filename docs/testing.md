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

CDP `Runtime.evaluate` 可能发送 `Origin: null`，Tauri 2 会拒绝该值（`Origin header is not a valid URL`）。`cdp-invoke.mjs` 仅拦截 `ipc.localhost`，将无效 Origin 改为有效页面源；DOM 场景期间保持桥接，以覆盖真实点击触发的 IPC。所有场景启动前均验证应用已渲染，不接受浏览器错误页、空白页或仅有 `__TAURI_INTERNALS__.invoke` 的启动页。

`-SkipBuild` 要求 `.system-test-vite-port` 内的 EXE SHA256 与当前 debug EXE 匹配，不从 EXE 字符串或当前配置猜端口。它只证明构建产物与端口元数据匹配，不证明源码未变化：修改 Rust 或注册 IPC 后必须不带 `-SkipBuild` 重编。

新增 UI 场景：`SG-gateway-ui-reliability` 检查智能网关服务与绑定，并验证连接不被抢占、Code/Codex 漂移检测与显式重新应用；`SG-ui-layout-matrix` 覆盖顶栏/侧栏、深浅色、中英文导航与截图。`SG-upstream-limits-ui` 使用无凭据测试上游验证限额弹窗加载、真实键盘输入、保存重开与 Escape/焦点返回，不发送上游请求。`SG-p0-simulate` 已随路由模拟器删除。截图产出本身不等于视觉验收通过。

事件刷新可单独运行 `node scripts/test-usage-log-refresh.mjs`：转译真实 Hook 源码，覆盖共享订阅、StrictMode、隐藏/恢复可见、尾随刷新和外部在途查询补查。它不替代实际 WebView 事件验收。若测试凭据库报 `Windows error code 8`，应报告场景失败；禁止清除用户凭据、改用明文落盘或关闭生产凭据保护来获得通过结果。

Coverage IDs: [`scripts/system-test/matrix.md`](../scripts/system-test/matrix.md). Smart-gateway acceptance: [`docs/smart-gateway/acceptance.md`](smart-gateway/acceptance.md).

## When to run L2

Change `live_sync` / `gateway_binding` / Claude Code write paths, then run `pnpm system:test` (or the matching scenario). Do not use the user's production `app.db`.
