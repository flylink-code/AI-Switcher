# Claude Switcher

> 统一管理 **Claude Desktop** 和 **Claude Code** 的第三方 API 配置：可视化添加供应商、一键切换，无需手动编辑配置文件。

技术栈：**Tauri 2 + React 19 + TypeScript + Ant Design 6 + Zustand + i18next + SQLite (rusqlite)**。

当前状态：**P0 脚手架** —— 可运行的空壳 + 基础库（配置目录探测、原子写入、备份轮换、SQLite 层、系统托盘）。供应商切换等业务功能在 P1+ 阶段实现，详见 [`task.md`](./task.md)。

---

## 目录结构

```
claudedesktopconfig/
├── src/                      # React 前端
│   ├── pages/                # Providers/MCP/Prompts/Skills/Usage 占位页 + Environment 验证页
│   ├── components/AppLayout  # 侧边栏 + 主题/语言切换
│   ├── stores/               # Zustand: themeStore, appStore
│   ├── i18n/                 # zh-CN / en-US
│   └── services/api.ts       # Tauri IPC 封装
├── src-tauri/                # Rust 后端
│   └── src/
│       ├── config/           # 路径探测 (paths, claude_desktop) + 原子写入 (atomic)
│       ├── database/         # SQLite: Database 封装 + schema + dao
│       ├── commands/         # Tauri 命令: ping / get_paths / get_db_info / backup_now
│       ├── backup.rs         # 文件级备份轮换（保留 10 份）
│       ├── tray.rs           # 系统托盘
│       └── lib.rs            # 入口：插件注册、DB 初始化、托盘、窗口事件
├── scripts/
│   ├── gen-icons.py          # 重新生成图标集
│   ├── cargo-msvc.bat        # 在 MSVC 环境中运行 cargo（见下）
│   └── tauri-msvc.bat        # 在 MSVC 环境中运行 pnpm tauri
└── task.md                   # 规划文档（阶段、机制、架构）
```

## 开发环境要求

- **Node.js** ≥ 20（已用 24 测试）、**pnpm** ≥ 9
- **Rust**（stable，MSVC 目标）—— 通过 [rustup](https://rustup.rs/) 安装
- **Windows**：Visual Studio 2022 的「使用 C++ 的桌面开发」工作负载 + Windows 10/11 SDK
  （macOS / Linux 理论可编译，但 P0 仅在 Windows 验证）

## ⚠️ Windows 编译须知：MSVC 环境

`*-sys` crate（如 `vswhom-sys`）调用 `lib.exe` 时需要 MSVC 的 `INCLUDE`/`LIB` 环境变量。
在 Git Bash / 普通终端中这些变量为空，会导致链接失败（`lib.exe` 退出码 1107）。

**解决方法**：通过仓库自带的脚本在 MSVC 环境中运行 cargo/tauri，脚本会先 `call vcvars64.bat`：

```bash
# 安装依赖
pnpm install

# 开发模式（启动 Tauri dev，热重载）
scripts/tauri-msvc.bat dev          # 等价于 pnpm tauri dev，但带 MSVC 环境

# 仅检查/测试 Rust 后端
cd src-tauri && ../scripts/cargo-msvc.bat check
cd src-tauri && ../scripts/cargo-msvc.bat test

# 打包（debug，不产出安装包）
scripts/tauri-msvc.bat build --debug --no-bundle
```

> 在「Developer Command Prompt for VS 2022」或已 `call vcvars64.bat` 的终端里，可直接用
> `pnpm tauri dev` / `cargo test`，无需上述脚本。

## P0 验证（Environment 页）

启动应用后默认进入「环境信息」页，可视化校验基础库：

- **ping** —— 确认前后端 IPC 通路（返回 `pong`）
- 探测路径：`~/.claude`、`~/.claude.json`、Claude Desktop `configLibrary`、`~/.claude-switcher/`
- **立即备份** —— 在 `~/.claude-switcher/backups/` 生成带时间戳的备份，超过 10 份自动清理
- 数据库：`~/.claude-switcher/app.db`，Schema 版本与供应商数量
- 主题（浅/深/跟随系统）+ 语言（中/英）切换
- 关闭窗口最小化到系统托盘

## 数据目录

应用数据存放在 `~/.claude-switcher/`：

| 路径 | 说明 |
|------|------|
| `app.db` | SQLite 主库（供应商、MCP、请求日志、定价、设置） |
| `backups/` | 自动轮换的备份（默认保留 10 份） |

## 路线图（见 `task.md`）

| 阶段 | 状态 | 内容 |
|------|------|------|
| **P0 脚手架** | ✅ 本版本 | 工程初始化、SQLite、路径探测、原子写入、备份、托盘 |
| P1 | 待开发 | 供应商 CRUD + 预设；`settings.json` 读写切换 |
| P2 | 待开发 | Claude Desktop `configLibrary` 写入；Rust 本地代理 |
| P3 | 待开发 | MCP 统一面板 + Prompts 编辑器 |
| P4 | 待开发 | 用量统计仪表盘 |
| P5 | 待开发 | Skills、托盘快速切换、i18n/自启、Windows 打包 |
