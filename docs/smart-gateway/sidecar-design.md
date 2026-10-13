# 反代网关外置 sidecar（方案 B）设计

状态：**管理接口与监督状态已落地，未设置时默认独立进程**。`gateway_runtime_mode` 缺省、空白或无法识别时为 `sidecar`；已保存的 `inproc` 保持进程内。切换后重启生效。实现见 `src-tauri/src/sidecar.rs`。

## 为什么拆

- AG / Kiro 反代的崩溃、内存增长、长连接不应拖垮主程序与智能网关。
- 反代可以独立重启，不需要重启整个桌面端。
- 其他客户端想直接使用反代，不必常驻整套桌面 UI。

## 与最初设计的差异（实现取舍）

| 项 | 最初设计 | 实际实现 | 原因 |
| --- | --- | --- | --- |
| 可执行文件 | 独立 crate / 独立 exe（不做） | **同一个 exe**，`--gateway-sidecar` 启动 | 版本天然一致，不改打包与更新器；`main` 里与 localization worker 同款分流 |
| 版本校验 | 启动时校验 | 仍校验（`--sidecar-version`），不一致退出码 3，主程序不再重启 | 防御异常打包 |
| 账号文件写入 | 主程序只读，经管理接口操作 | **主程序只提交快照**。sidecar 模式下 `persist` / `save_accounts` / `save_live_models` 把整份 JSON 经 stdin 交给 sidecar 落盘并替换其内存；基线戳不一致则拒绝。主程序仍按文件戳重读 sidecar 自己的写入（令牌、额度） | 不新开端口；EOF 仍是停止信号 |
| 刷新令牌 / 额度刷新 | 仅 sidecar | sidecar 模式下主程序不跑后台额度刷新，由 sidecar 刷新 | 保持后台写入单一 |
| 一次性 Key | 启动时生成 | 不做，沿用库内 `*_gateway_api_key` | 见“不做” |
| 管理接口 | 状态/账号/登录/刷新模型 | **账号提交**走 stdin/stdout 的一行 JSON（`op=commit`，库名 `ag` / `kiro` / `kiro-models`）。登录回调仍在主程序。设置变更仍靠重启 sidecar。探活仍是 `/healthz`。监督状态经 `get_gateway_sidecar_status` 给设置页 | 不做一次性 Key，不新开端口 |
| 用量写入 | 只写连接或本地接口 | sidecar 直接写同一个 SQLite（WAL）；主程序每 3 秒看 `proxy_request_logs` 的 `MAX(rowid)`，有新行就发 `usage-log-recorded` | 二选一已定为前者 |

## 进程边界

| 项 | 方案 A（`inproc`） | 方案 B（默认，`sidecar`） |
| --- | --- | --- |
| 进程 | 主程序内的 tokio 任务 | 同一 exe 的子进程，主程序负责启停与健康检查 |
| 监听 | `127.0.0.1:15830`（AG）、`127.0.0.1:15831`（Kiro） | 不变，仍只听 `127.0.0.1` |
| 与智能网关关系 | `up_ag_15830` / `up_kiro_15831` 上游 | 不变 |

端口、上游 id、`provider_kind`、用量 hop 都不变，智能网关、目录、用量统计不需要改。

## 模式切换

- 设置键 `gateway_runtime_mode`：`sidecar`（默认）/ `inproc`。命令 `get_gateway_runtime_mode` / `set_gateway_runtime_mode`；**重启应用后生效**。设置页「通用」里有“反代网关运行方式”下拉，切换后出现“立即重启”。独立进程正在运行时，同一页显示监督状态（`get_gateway_sidecar_status`）。
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
- sidecar 模式下这些文件只有 sidecar 写入。主程序提交时带上自己读到的 mtime+长度；sidecar 在账号锁内核对，不一致就拒绝，主进程下次读取会重载。令牌刷新、冷却、额度仍只在 sidecar 写文件，主程序靠文件戳看到结果。
- 协议是一行 JSON：`{"id","op":"commit","store","body","base":{"exists","mtime_ns","len"}}`，响应 `{"id","ok","error","mtime_ns","len"}`。`body` 是 JSON 字符串，所以快照里的换行不会截断。单条上限 8MB。日志仍走 stderr。stdin 关闭（或父进程消失）照旧退出。
- 登录回调端口目前仍由主程序承载（未移入 sidecar）。
- sidecar 模式下 `thought-signatures.db` 只由 sidecar 打开。
- 主库 Schema、ZIP / WebDAV 备份范围不变。

## 已落地与已决定

1. **管理接口**：见上文。主程序在 `delegated()` 时不再写这三份账号文件。
2. **监督状态**：设置页「通用」在独立进程正在被监督时显示运行 / PID、自动重启次数、上次退出码、退避，以及 Antigravity 与 Kiro 的探活。命令 `get_gateway_sidecar_status`。重启次数只计子进程自己退出，不含设置变更触发的重启。
3. **测试**：`src-tauri/tests/gateway_sidecar.rs` 覆盖版本不符退出码 3、按启用标志监听并通过 `/healthz`、关闭 stdin 后退出且端口释放、父管道断开后自行退出、账号提交与过期基线被拒绝、监督进程拉起后杀掉子进程会再拉起并最终停掉端口。退避计算有单元测试。监听端口、上游 id、用量 hop 不随运行方式改变，因此不为两种模式各跑一遍整套 L2。真机出网：独立进程已在本机跑过一版，没有明显问题；这里没有再发新的 Google / Kiro 请求。
4. **默认**：未设置时为独立进程。已经保存成 `inproc` 的安装保持主程序内。

运行：`cargo test --test gateway_sidecar`（本机若有 dev 实例占着 `target\debug\claude-switcher.exe`，请换 `CARGO_TARGET_DIR`）。

## 不做

- 不拆独立 crate / 独立 exe：同一个 exe 已满足版本一致，而网关代码与主 crate 的 `error` / `database` / `system_proxy` 等耦合很深，拆分只增加风险。
- 不做一次性 Key：Claude Code、Codex 等客户端直连 15830 / 15831，沿用库内 `*_gateway_api_key`；强制一次性 Key 会让它们失效。

- 不监听局域网。
- 不引入 Windows 服务或开机服务。
- 不为 sidecar 新增对外端口。
