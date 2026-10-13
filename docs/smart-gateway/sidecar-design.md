# 反代网关外置 sidecar（方案 B）设计

状态：**已实现第一版，默认关闭**。默认仍是方案 A（反代网关随主程序同进程运行）；设置 `gateway_runtime_mode = sidecar` 并重启后，AG / Kiro 反代由外置进程承载。实现见 `src-tauri/src/sidecar.rs`。

## 为什么拆

- AG / Kiro 反代的崩溃、内存增长、长连接不应拖垮主程序与智能网关。
- 反代可以独立重启，不需要重启整个桌面端。
- 其他客户端想直接使用反代，不必常驻整套桌面 UI。

## 与最初设计的差异（实现取舍）

| 项 | 最初设计 | 实际实现 | 原因 |
| --- | --- | --- | --- |
| 可执行文件 | 独立 crate / 独立 exe（不做） | **同一个 exe**，`--gateway-sidecar` 启动 | 版本天然一致，不改打包与更新器；`main` 里与 localization worker 同款分流 |
| 版本校验 | 启动时校验 | 仍校验（`--sidecar-version`），不一致退出码 3，主程序不再重启 | 防御异常打包 |
| 账号文件写入 | 主程序只读，经管理接口操作 | **两进程共享账号文件**；每次访问前检查文件 mtime+长度，外部改动则重读（`file_stamp.rs`） | 管理接口要重写全部账号相关 IPC，当前不做 |
| 刷新令牌 / 额度刷新 | 仅 sidecar | sidecar 模式下主程序不跑后台额度刷新，由 sidecar 刷新 | 保持后台写入单一 |
| 一次性 Key | 启动时生成 | 不做，沿用库内 `*_gateway_api_key` | 见“不做” |
| 管理接口 | 状态/账号/登录/刷新模型 | **未做**；主程序用 `/healthz` 探活，设置变更靠重启 sidecar 生效 | 同上 |
| 用量写入 | 只写连接或本地接口 | sidecar 直接写同一个 SQLite（WAL）；主程序每 3 秒看 `proxy_request_logs` 的 `MAX(rowid)`，有新行就发 `usage-log-recorded` | 二选一已定为前者 |

## 进程边界

| 项 | 方案 A（默认） | 方案 B（`sidecar`） |
| --- | --- | --- |
| 进程 | 主程序内的 tokio 任务 | 同一 exe 的子进程，主程序负责启停与健康检查 |
| 监听 | `127.0.0.1:15830`（AG）、`127.0.0.1:15831`（Kiro） | 不变，仍只听 `127.0.0.1` |
| 与智能网关关系 | `up_ag_15830` / `up_kiro_15831` 上游 | 不变 |

端口、上游 id、`provider_kind`、用量 hop 都不变，智能网关、目录、用量统计不需要改。

## 模式切换

- 设置键 `gateway_runtime_mode`：`inproc`（默认）/ `sidecar`。命令 `get_gateway_runtime_mode` / `set_gateway_runtime_mode`；**重启应用后生效**。设置页「通用」里有“反代网关运行方式”下拉，切换后出现“立即重启”。
- 回滚到方案 A：设回 `inproc` 并重启。账号文件格式不变，无需迁移。
- sidecar 模式下主程序里的 AG / Kiro `start_*` / `stop_*` 只改“启用”标志并请求重启 sidecar；`gateway_status().running` 来自探活结果。
- 端口、API Key、出站代理、限速、fast-path 保存后会触发 sidecar 重启以重新读库（进行中的流会断）。

## 生命周期

1. 主程序启动：模式为 sidecar 时启动监督任务；`antigravity_gateway_enabled` / `kiro_gateway_enabled` 至少一个为 1 才拉起子进程。旧版“当前供应商是 AG”的隐式自启，在监督启动时一次性落成 AG 启用标志。
2. 子进程启动：开库、初始化账号库、只按**持久化启用标志**启动对应监听，并跑 AG 额度后台刷新。
3. 探活：主程序每 3 秒请求 `/healthz`，更新 `running`。
4. 崩溃：主程序按 1s→2s→…→30s 退避重启（稳定运行 30 秒后重置）。重启期间连接被拒，不触发跨供应商 failover。
5. 停止：关闭子进程 stdin 即为停止信号（主程序崩溃时管道同样断开，子进程自行退出，不留占端口的孤儿）。5 秒内不退出则强杀。
6. 开机自启只登记主程序。
7. 子进程日志：`~/.claude-switcher/gateway-sidecar.log`（超过 2 MB 截断）。

## 数据与账号

- 账号文件位置不变：AG 账号存储、`~/.claude-switcher/kiro_accounts.json`、`kiro_models.json`。
- 两进程各持内存副本，读写前按文件戳对齐。**仍有极窄的读改写竞态**（两边在同一毫秒级窗口同时改同一文件时后写者覆盖先写者）；常见写者（令牌刷新、冷却、额度）只在 sidecar，用户在 UI 里的增删改在主程序。
- 登录回调端口目前仍由主程序承载（未移入 sidecar）。
- sidecar 模式下 `thought-signatures.db` 只由 sidecar 打开。
- 主库 Schema、ZIP / WebDAV 备份范围不变。

## 未做（后续）

1. sidecar 本地管理接口，使主程序对账号完全只读（根除上面的竞态）。
2. 前端展示 sidecar 运行状态（崩溃、重启次数）。
3. 主程序监督（拉起、重启、退避、探活）的端到端测试；L1 / L2 对两种运行方式跑同一套场景；真机验证 AG / Kiro 实际出网。
4. 稳定一个版本后再决定默认外置；先保留进程内实现一个大版本。

已有自动化：`src-tauri/tests/gateway_sidecar.rs` 用真实 exe 在隔离 HOME 里验证 sidecar 进程本身：版本不符退出码 3、按启用标志监听并通过 `/healthz`、关闭 stdin 后退出且端口释放、父管道断开后自行退出。运行：`cargo test --test gateway_sidecar`（本机若有 dev 实例占着 `target\debug\claude-switcher.exe`，请换 `CARGO_TARGET_DIR`）。

## 不做

- 不拆独立 crate / 独立 exe：同一个 exe 已满足版本一致，而网关代码与主 crate 的 `error` / `database` / `system_proxy` 等耦合很深，拆分只增加风险。
- 不做一次性 Key：Claude Code、Codex 等客户端直连 15830 / 15831，沿用库内 `*_gateway_api_key`；强制一次性 Key 会让它们失效。

- 不监听局域网。
- 不引入 Windows 服务或开机服务。
- 不为 sidecar 新增对外端口。
