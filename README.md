# AI-Switcher

> 面向 Claude Code、Claude Desktop 与 Codex 的本地配置、供应商和辅助工具管理器。

[English](README_en.md)

AI-Switcher 是一款基于 Tauri 2、Rust 与 React 构建的桌面应用。它把分散在配置文件、系统凭据库和本地目录中的常用能力整合到一个界面中，并让 Claude Code 与 Claude Desktop 的供应商和当前配置保持相互独立。

项目默认在本机工作。API 密钥保存到操作系统凭据库，配置写入前会备份，会话管理器只读取本地 Claude Code 会话文件。

## 主要功能

- **供应商管理**：分别管理 Claude Code、Claude Desktop 与 Codex 的第三方 API、模型映射、导入导出、连接测试、模型发现和官方登录恢复。Codex 使用 `~/.codex/config.toml` 的直连模型提供方，不经过 Claude 本地代理。
- **本地代理**：提供 Anthropic Messages 兼容代理、模型映射、密钥注入、流式转发、运行状态和请求日志；可显式启用自动故障切换，连续两次临时失败会在本次代理运行期间熔断 60 秒。该开关默认关闭。
- **MCP、Prompts 与 Skills**：统一维护 MCP 服务（可同步到 Codex），管理 `CLAUDE.md` 预设；Skills 会记录安装来源、版本摘要并支持手动检查更新。
- **会话管理**：浏览、筛选和搜索 Claude Code 与 Codex 的本地 JSONL 会话；Codex 会话位于 `~/.codex/sessions`。
- **中文化中心**：分别管理 Claude Code CLI、VS Code/Cursor 扩展补丁助手及 Claude Desktop 语言包；补丁应用始终需要在编辑器中确认。
- **用量统计**：按供应商和模型统计请求、Token、趋势与估算成本；年度热力图会随窗口宽度缩放，完整显示全年数据。
- **系统集成**：系统托盘快捷切换、跟随界面语言的中英文菜单、高对比度浅色/深色/跟随系统主题和开机自启。桌面端会为卡片、表格、表单控件与弹层应用动态主题配色。
- **环境与更新**：查看配置路径、Claude Code 版本和应用更新状态。

## 会话管理说明

Claude Code 会话以只读方式扫描：

- 数据源：`~/.claude/projects/**/*.jsonl`
- 列表阶段只提取会话 ID、摘要、工作目录和时间等元数据
- 打开详情或执行全文搜索时才读取消息内容
- 所有文件路径都会校验在允许的会话根目录内
- 浏览和搜索不修改原始会话；用户可明确选择导出，或移入 AI-Switcher 回收站后恢复

Claude Desktop 没有公开稳定的本地会话枚举格式。当前版本仅检测其本地数据目录并提供 `claude://claude.ai/new` 官方入口，不读取 Chromium 缓存或调用私有接口。已知会话 ID 可使用 Anthropic 公布的 [Claude Desktop 深链格式](https://support.claude.com/en/articles/14729294-open-claude-desktop-with-a-link) 打开。

## 安装

从 [GitHub Releases](https://github.com/flylink-code/AI-Switcher/releases/latest) 下载 NSIS 安装程序。安装后的主程序文件名为 `AISwitcher.exe`。

运行环境：

- Windows 10/11
- Claude Code 或 Claude Desktop 按需安装
- 从源码开发时需要 Node.js 20+、pnpm 9+、Rust stable（MSVC）以及 Visual Studio 2022 C++ 桌面开发组件

## 从源码运行

```powershell
pnpm install
pnpm tauri dev
```

如果当前终端没有 MSVC 环境变量：

```powershell
scripts\tauri-msvc.bat dev
```

## 构建

构建脚本默认先运行完整 Rust 测试，再编译应用。构建正式版 exe：

```powershell
scripts\build-exe.bat
# 或
pnpm build:exe
```

快速构建调试版：

```powershell
scripts\build-exe.bat debug
# 或
pnpm build:exe:debug
```

完整构建 MSI 与 NSIS 安装包：

```powershell
scripts\build-exe.bat bundle
# 或
pnpm build:exe:bundle
```

需要跳过测试进行快速本地构建时：

```powershell
scripts\build-exe.bat release skip-tests
# 或直接使用 PowerShell 入口
.\scripts\build-exe.ps1 -SkipTests
```

脚本会自动发现已安装的 Visual Studio 2022 Community、Professional、Enterprise 或 Build Tools，并在没有全局 `pnpm` shim 时回退到 `corepack pnpm`。

主要产物：

| 产物 | 路径 |
|---|---|
| Tauri 正式版 | `src-tauri\target\release\AISwitcher.exe` |
| 正式版测试副本 | `release\AISwitcher.exe` |
| 调试版测试副本 | `release\AISwitcher-debug.exe` |
| 安装包 | `src-tauri\target\release\bundle\` |

## 数据与配置

| 路径 | 用途 |
|---|---|
| `~/.claude/settings.json` | Claude Code 当前供应商配置 |
| `~/.claude.json` | Claude Code MCP 与项目配置 |
| `~/.claude/projects/` | Claude Code 本地会话，只读 |
| `%LOCALAPPDATA%\Claude-3p\configLibrary\` | Claude Desktop 第三方网关配置 |
| `~/.claude/skills/` | Claude Code Skills |
| `~/.claude-switcher/`（默认）或“环境”页选择的目录 | AI-Switcher 自有资料库：数据库、备份、下载资源与日志 |

应用已更名为 AI-Switcher，但保留原应用标识、签名密钥和默认资料库位置，以兼容既有用户。资料库可迁移至其他盘；迁移会逐文件校验 SHA-256、保留旧副本，并在重启后生效。Claude 的活动配置仍保留在官方读取目录。

环境页可导出版本化资料库 ZIP：它包含脱敏数据库快照、Skills、Skill 来源记录和会话归档，并附逐文件 SHA-256 清单。导出与 WSL/SSH 推送均不包含 API Key、系统凭据、Claude 登录状态、密码或私钥。同步先显示预览；确认后只将归档写入远端 `incoming/`，不自动覆盖远端活动配置。

## 安全与隐私

- API 密钥通过 Windows Credential Manager、macOS Keychain 或 Linux Secret Service 保存。
- 配置文件使用原子写入，并在修改前创建轮换备份。
- 会话管理器只读本地 JSONL，不建立全文数据库，不访问任意用户路径。
- 会话可能包含源代码、密钥或其他敏感信息，复制命令和内容前请自行确认。
- 除供应商连接测试、模型发现、更新检查、用户主动下载与用户确认的 WSL/SSH 归档推送外，应用不会上传本地内容。

## 项目结构

```text
src/                         React、Ant Design、Zustand 与 i18next 前端
src/pages/SessionsPage.tsx   本地会话列表与详情
src-tauri/src/               Rust 后端、配置、代理、数据库与托盘
src-tauri/src/session_manager.rs
                             只读会话适配与路径校验
scripts/                     Windows 开发和构建脚本
```

## 参考与致谢

本项目在产品设计和实现思路上参考了以下开源工程。AI-Switcher 是独立项目，与这些项目及 Anthropic 均无隶属或官方关系。

| 项目 | 参考方向 | 上游与许可证 |
|---|---|---|
| AI Toolbox | 多工具配置管理、会话与桌面信息架构 | [coulsontl/ai-toolbox](https://github.com/coulsontl/ai-toolbox)，MIT |
| cc Proxy | Claude Desktop 本地代理和模型替换思路 | [arhsis/cc-proxy](https://github.com/arhsis/cc-proxy)，以其仓库许可证声明为准 |
| CC Switch | Provider 切换、Tauri 架构、会话解析与托盘交互 | [farion1231/cc-switch](https://github.com/farion1231/cc-switch)，MIT |
| Claude Code for VS Code 中文包 | VS Code 扩展定位、汉化规则、备份与恢复流程 | 本地参考：`examples/claude-code-vscode-zh-cn`；[zstings/claude-code-zh-cn](https://github.com/zstings/claude-code-zh-cn)，MIT |
| Claude Code 中文本地化插件 | Claude Code CLI 的中文化安装、更新与恢复流程 | 本地参考：`examples/claude-code-zh-cn`；[taekchef/claude-code-zh-cn](https://github.com/taekchef/claude-code-zh-cn)，以其仓库许可证声明为准 |
| Claude Desktop 中文补丁 | Desktop 安装发现、语言包校验与恢复流程 | 汉化仓库：[javaht/claude-desktop-zh-cn](https://github.com/javaht/claude-desktop-zh-cn)，以其仓库声明为准 |
| Code Switch | 本地代理、故障切换与 Claude Code/Codex 配置管理 | [daodao97/code-swtich](https://github.com/daodao97/code-swtich)，Apache-2.0 |

引用、移植或再分发对应项目代码时，请同时遵守其上游许可证和版权声明。

## 当前边界

- 会话恢复仅复制命令，不自动启动终端或执行命令；移入回收站前会先创建可校验归档。
- 不自动导入远端归档、合并远端冲突或提供团队分享。
- Claude Desktop 历史记录在官方提供稳定接口前不做私有格式解析。
- Claude Code 与 Claude Desktop 的供应商、激活状态和 live 配置始终独立。
