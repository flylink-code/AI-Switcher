# Agent：代码维护工程师
职责：基于现有代码修复BUG、新增功能

## 硬性规则（不可违背）
1. 禁止绕过原有逻辑：不允许整体重构、更换算法、替换架构规避问题。BUG必须在现有代码结构内原地修复；新增功能复用现有调用关系、数据结构，不推翻原有实现。禁止直接重写整套代码。
2. 禁止过度防御编程：仅添加业务场景真实会触发的边界判断、异常处理。不会实际发生的异常不额外添加；禁止无脑批量判空、try-catch、断言、参数校验。只保留本次需求相关的必要防护。
3. 最小改动原则：仅修改和BUG/新功能相关代码片段。无关代码原样保留，不随意调整格式、变量命名、注释风格，不优化无关逻辑。
4. 新增功能约束：新增代码贴合原有代码风格；优先复用现有函数/结构体，不引入不必要新依赖、新库。新增逻辑与原有代码兼容，不破坏原有正常业务流程。

## 固定输出格式，严格按顺序
① 需求梳理：总结本次要修复的bug 和/或 需要新增的功能点
② 改动分析：
  - 修bug：写出根因，解释原代码出错原因
  - 新增功能：说明新增逻辑实现思路，如何和原有代码融合
③ 改动清单：列出修改/新增的代码片段位置
④ 完整结果代码：输出修改后的全部代码，改动部分做标记
⑤ 校验说明：验证改动满足需求；同时说明没有添加多余防御代码，没有更换底层方案

## 兜底提醒
如果发现改动属于绕开原始问题、添加冗余防御代码，需要回退，回到原始代码，只做最小改动，删除不必要边界检查，重新修改。

# AI-Switcher — 项目交接

> 跨会话**唯一**交接源。只写仍有效的结构、硬约束摘要、未决事项；不堆已归档进度。
> 会话结束时只修订仍成立的条目；已落地细节进代码/文档，不要再开 `CLAUDE.md`。
> 功能对照以 `README.md` / `README_en.md` 为准。

## 文档索引

| 主题 | 位置 |
| --- | --- |
| 产品形态、导航、供应商页、模型映射、Agent 分级 | `docs/agents/product.md` |
| Schema、绑定、迁移与回滚 | `docs/agents/schema.md`、`docs/smart-gateway/migration.md` |
| 智能网关（目录、解析、冷却、流式收尾、用量去重） | `docs/agents/gateway.md` |
| Antigravity / Kiro / Desktop | `docs/agents/antigravity.md`、`kiro.md`、`desktop.md` |
| 编译、热加载、测试、发版 | `docs/agents/build-test.md`、`docs/testing.md` |
| 易踩点 | `docs/agents/pitfalls.md` |
| 设计冻结 | `docs/smart-gateway/` |
| 缺陷清单 | `bug.md`（gitignore） |

## 代码地图（所有代码任务必须遵循）

- 先读 `CODE_MAP_INDEX.md`，选择模块 → 文件符号页 → 定点读源码；未知归属才扩大搜索。地图是静态导航，不替代调用关系、类型检查或测试。
- 任何代码/文件新增、编辑、删除、重命名后，执行 `pnpm codemap`；已建立缓存时可用 `pnpm codemap --module <模块名>`。非代码改动也执行一次，确认索引仍一致。
- 完成前清单：相关验证通过；`pnpm codemap` 与 `pnpm codemap:check` 通过；检查并纳入 `CODE_MAP_INDEX.md`、`docs/codemap/` 的对应变化；报告未覆盖情况。禁止手改生成地图。
- 生成器、覆盖范围、工具选型及可选安全 pre-commit：见 `CODE_MAP.md`。CI 强制检查；外部 Agent 是否加载本文件取决于其客户端配置，不能假定全部遵守。

## 项目

- **AI-Switcher**（原 Claude Switcher）：Windows 优先桌面端，统一配置各 Agent 的供应商、智能网关、Antigravity 反代、本地代理、MCP、会话、用量。
- 仓库 `flylink-code/AI-Switcher`。Tauri 2 + Rust + React + Vite；Corepack **pnpm**。
- 版本 **1.6.6**（`package.json` / `src-tauri/tauri.conf.json` / `src-tauri/Cargo.toml` 必须对齐；MSI 数字版本 `1.6.6.2`，高于 beta.1 的 `1.6.6.1`）。Schema **37**（`SCHEMA_VERSION`）。
- v1.6.6 后作者主要转用 [Magpie](https://github.com/yetone/magpie)，后续大概率不再新增功能；保留仓库与既有版本，修复及发版视实际需要，不承诺持续更新。
- 后续开发走 **main**。1.4.xx 热修走 `release/1.4.x`（Schema 28），禁止把 Schema 33 回写进 1.4 线。
- 不要把 `release/`、`src-tauri/target/`、`dist/` 纳入提交。本地 `bug.md` / `task.md` 不提交。不要安装或替换本机正式程序。

## 产品形态摘要

- 全局供应商唯一存于 `upstreams`；绑定 `gateway` / `direct`。Code 直连限 Anthropic，Codex 限 OpenAI，Codex OAuth 必须经网关。
- 主导航 7 项：概览 / Agent / 供应商 / 网关 / 用量 / 工作区 / 设置（「网关」页键 `accounts`，放 Antigravity / Kiro / ChatGPT 账号池）；布局 `cs.layoutMode`（`sidebar` | `top`，默认 `top`）。
- Agent 分级：T1 Claude Code、Codex（新功能先做，回归必测）；T2 OpenCode、Pi（维持）；T3 Cline、DSH（产品面隐藏，代码 / Schema / 端口保留，设置不可勾选）。Claude Desktop 在产品面。
- 绑定后 Code/Codex 直连 15828；Desktop 走 15822。

### 本机端口

| 端口 | 用途 |
| --- | --- |
| **15828** | 智能网关（只听 `127.0.0.1`，不做局域网） |
| **15830** | Antigravity 反代 |
| **15831** | Kiro 反代 |
| 15821 / 15822 / 15823 / 15826 / 15827 | 本地代理：Code / Desktop / Codex / DSH / Cline（Cline 与 DSH 的 UI 已隐藏） |
| 5250 | Vite / `devUrl`（本机常被 Cadence `cdslmd` 占，热加载改 **5251**，编完还原，勿提交） |

## 硬约束

- 热路径只读（`with_read_conn`）；同名模型按供应商前缀区分；`auto` 落到目录第一项；`count_tokens` 不写用量。
- `CARGO_TARGET_DIR` 必须是绝对路径。
- 弹窗禁用 `Modal.confirm`（见 `pitfalls.md`）。
- 大文件按 `include!` 拆（同一模块）；`#[derive]` 必须跟 struct 同文件。
- `config/codex.rs` 的 `read_current_live_provider` / `truncate_incomplete_tool_rounds` 是假阳性，勿删。

## 代码目录

| 区域 | 路径 |
| --- | --- |
| 页面注册 | `src/lib/pageRegistry.ts` |
| 前端页 | `src/pages/` |
| 侧栏 / 顶栏壳 | `src/components/layout/`、`src/components/v2/shell/` |
| 供应商 IPC | `src-tauri/src/commands/providers/` |
| 智能网关 | `src-tauri/src/gateway/`、`commands/gateway.rs`、`src/pages/GatewayPage.tsx` |
| 反代网关 | `src-tauri/src/antigravity/` |
| 本地代理 | `src-tauri/src/proxy/`、`src/pages/LocalProxyPage.tsx` |
| 配置写出 | `src-tauri/src/config/` |
| 会话 | `session_manager/` |
| 用量 | `commands/usage.rs`、`usage/` |
| 资料库 | `src-tauri/src/database/`（默认 `~/.claude-switcher/`） |
| 额度探测 | `src-tauri/src/quota/` |
| 参考样例（只读） | `examples/`（free-claude-code、Antigravity-Manager、sub2api、CLIProxyAPI、cc-switch 等） |

## 不做

- GitHub Copilot、FCC IM/语音/48 供应商、模型组负载均衡、局域网监听、把 daily 默认改成只走 prod。

## 未做 / 未决

- 用户已安装 beta.3。普通上游余额不足恢复的分类/冷却/半开及 HTTP 模拟回归通过，未对真实账号制造欠费验证：`INSUFFICIENT_BALANCE` 按额度不足处理，默认 60 秒后由真实请求半开探测；遵循上游明确的 `Retry-After`。不靠目录探测清除额度冷却。证据见 `bug.md` 第 7 项。
- 未做：Hermes / Grok Build / Muse、WSL 内 `wsl --exec` CLI、Skills/插件 WSL 元数据改写、额度阶段 2（火山/自定义脚本）、前端 EnvironmentPage / UsagePage 拆分、ts-rs 后续批次。
- Codex 新 CLI 顶层 `token_usage_record` 故意不解析。流式 UI 认 `cancelled`，但客户端断开目前未打该标记。
- Responses 转 Anthropic/Chat 的续聊历史复用 `CodexHistoryStore`，仅内存保存，512 条 / 64 MiB 序列化历史上限；重启或淘汰后需新建对话。用户此前确认 VS Code 原问题修复；beta.3 安装版真实 CLI 暗号、切模、文件/图片工具续聊、Responses 增量续聊和 Gemini/Kiro 工具轮次均通过。联网 CLI 配置有效进程代理后，WebFetch 读取/续聊及安装版 WebSearch 来源/续聊通过，未修改应用环境或系统代理。站点人机验证仍可能发生，不要将模型记忆回答或验证页计为联网成功。最新证据见 `test.md` 和 `src-tauri/target/beta3-review/review.md`。
- 从 live 导入全局上游支持 Code / Codex / OpenCode；Pi / Cline 暂不支持。DSH 保留旧写回行为。
- 统一供应商验证缺口：隔离 L2 / DOM 场景已覆盖档案保存、连接保护、Code/Codex 配置漂移与双布局矩阵；完整视觉/键盘验收、真实 AG/Kiro 启动及供应商出网联调仍未验证。回滚仅后端导出 IPC，无 UI。
- AG / Kiro 反代外置：`gateway_runtime_mode` 未设置时默认 `sidecar`（已存 `inproc` 保持进程内，重启生效；设置页「通用」有下拉和监督状态）。同一 exe 以 `--gateway-sidecar` 启动子进程；账号文件只由 sidecar 写入，主程序经 stdin JSON 行提交快照。不做一次性 Key / 独立 crate。详见 `docs/smart-gateway/sidecar-design.md`。
