# Claude Switcher P13–P15 优化任务

> 当前范围包含生产界面稳定、供应商地址/API 类型重构和多模型角色映射。P0–P12 已完成内容不在本文件重复规划。

## P13：界面稳定与响应式布局

### 已实施

- [x] 引入 Ant Design reset，并使用主题 Token 设置背景、文字、边框和状态色。
- [x] 移除全局 `.ant-layout { height: 100vh }`，将滚动限制在主内容区。
- [x] 为根布局和嵌套布局补齐 `min-width: 0`、`min-height: 0` 和溢出边界。
- [x] 将供应商操作按钮移到可换行工具栏，避免与卡片标题挤压。
- [x] 为供应商表格设置固定列宽、最小滚动宽度和长地址省略/复制。
- [x] 限制供应商弹窗主体高度，使表单在最小窗口下独立滚动。

### 待验收

- [ ] 检查 900×600、1000×650 和宽屏窗口下的供应商页、表格及弹窗。
- [ ] 检查亮色、暗色和跟随系统三种主题。
- [ ] 检查生产包中不存在控件重叠、双重滚动或主题色失效。

## P14：供应商基础地址与 API 类型

### 固定接口

- `ProtocolType` 保持 `anthropic`、`openai_chat`、`openai_responses` 三种公开值。
- `ProviderInput.baseUrl` 只保存 HTTPS 域名或网关路径前缀，不保存完整请求端点。
- API Key、模型、目标应用及凭据库存储方式保持不变。

### 已实施

- [x] 表单先选择 API 类型，再输入 HTTPS 基础地址。
- [x] 根据 API 类型实时预览最终请求地址。
- [x] 前后端拒绝 HTTP、用户名/密码、查询参数和 URL 片段。
- [x] 自动去除末尾斜杠以及已粘贴的 `models`、`messages`、`chat/completions`、`responses` 完整端点。
- [x] 连接测试、模型发现、导入和代理转发复用后端统一 URL 构造。
- [x] 保留 `/v1` 去重和自定义网关路径。
- [x] 数据库 v6 迁移规范化可安全识别的旧地址；无法解析的旧值原样保留。
- [x] 增加 URL 规范化、非法 URL、协议端点和旧数据迁移测试。

### 最终端点规则

| API 类型 | 程序追加路径 |
|---|---|
| Anthropic Messages | `/v1/messages` |
| OpenAI Chat Completions | `/v1/chat/completions` |
| OpenAI Responses | `/v1/responses` |
| 模型发现 | `/v1/models` |

### 待验收

- [ ] 验证根域名、已有 `/v1`、自定义网关前缀和粘贴完整端点的保存结果。
- [ ] 验证三种 API 类型的连接测试与实际代理请求使用正确端点。
- [ ] 验证新建、编辑、JSON 导入、当前配置导入和数据库升级结果一致。
- [ ] 验证无效旧地址在切换时返回可操作的配置错误且不会改写现有配置。

## 完成标准

- 用户只需填写 HTTPS 基础地址或网关路径前缀，并选择 API 类型。
- 不会生成 `/v1/v1/...` 或重复的完整请求路径。
- 供应商页在支持的最小窗口中无重叠、溢出和嵌套滚动异常。
- 编译、自动化测试和生产界面验收通过后，关闭 P13–P14。

## P15：Claude Code / Desktop 多模型角色映射

### 固定接口

- `Provider.model` 保留为必填默认模型。
- `Provider.modelMapping` 固定支持 Sonnet、Opus、Haiku、Fable 和 Subagent。
- Claude Code 与 Claude Desktop 的供应商及模型映射继续独立管理。
- 未配置角色统一回退默认模型。

### 已实施

- [x] 数据库升级至 v7，使用 `model_mapping_json` 保存角色映射，旧记录保持单模型回退行为。
- [x] 供应商表单支持模型发现下拉、手工输入、一键填充和角色映射预览。
- [x] Claude Code 写入默认、Sonnet、Opus、Haiku、Fable 和 Subagent 模型变量。
- [x] 新增模型变量已纳入配置所有权、备份、恢复和官方登录切换。
- [x] Claude Desktop 使用四个安全 `claude-*` 路由并通过 `labelOverride` 显示真实上游模型。
- [x] 第三方模型名、非 Anthropic 协议或显式角色映射会自动启用 Desktop 本地代理。
- [x] 本地代理提供 `/v1/models`，并为三种 API 类型复用统一模型解析结果。
- [x] 创建、编辑、JSON 导入导出和当前 Claude Code 配置导入支持角色映射。
- [x] 增加 v7 迁移、角色回退、配置写入、Desktop 模型目录和三协议请求模型测试。

### 待验收

- [ ] 在 Claude Code 中分别选择 Sonnet、Opus、Haiku、Fable 和 Subagent，确认上游收到对应模型。
- [ ] 在 Claude Desktop 模型菜单中确认四个安全角色可见，且实际请求映射到第三方模型。
- [ ] 验证角色留空时回退默认模型，编辑当前供应商后配置立即同步。
- [ ] 验证旧版单模型供应商升级后仍可切换，旧版导出 JSON 仍可导入。
