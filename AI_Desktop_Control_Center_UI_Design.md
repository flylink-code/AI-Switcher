# AI Desktop Control Center UI Design

## Product Position

AI Switcher → AI Desktop Control Center

基于：

- Tauri 2
- Rust Backend
- React Frontend

定位：

> Local AI Gateway / Provider Manager / Usage Analytics Desktop Application

参考：

- Cursor
- Raycast
- Linear
- Vercel Dashboard
- VSCode


---

# 1. Design Philosophy

不要做传统后台管理系统。

目标：

从：

> 管理 API

变成：

> 控制 AI 基础设施


关键词：

- Desktop First
- Real-time
- Minimal
- Developer Friendly
- Data Driven
- Native Feeling


---

# 2. Application Layout

```
┌──────────────────────────────┐
│ Title Bar                    │
├────────────┬─────────────────┤
│ Sidebar    │ Main Workspace  │
│            │                 │
└────────────┴─────────────────┘
```


---

# 3. Sidebar

```
AI Switcher

HOME

⌂ Dashboard


AI

◉ Providers

⇄ Router

◎ Models


Analytics

▣ Usage

◌ Logs


System

⚙ Settings
```


---

# 4. Dashboard

首页关注：

- 服务状态
- 成本
- Token
- 请求量


```
[ Cost ]

$47.32


[ Tokens ]

60M


[ Requests ]

1318


[ Success ]

95%
```


Provider Status:

```
OpenAI      🟢
Claude      🟢
DeepSeek    🟢
Local       🔴
```


---

# 5. Provider Card

不要使用表格。

使用卡片。


```
┌───────────────────────┐

🟣 OpenAI

GPT-5


Status

🟢 Online


Latency

230ms


Today Usage

12.5M Token


└───────────────────────┘
```


显示：

一级：

- Provider
- Status

二级：

- Model
- Latency
- Usage

三级：

- Endpoint
- API Key
- Advanced Config


---

# 6. AI Router

使用 React Flow。


```
Client

 |

AI Gateway

 |

+-------------+
|             |

OpenAI     Claude

70%          30%
```


支持：

- Load Balance
- Failover
- Priority
- Weight


---

# 7. Usage Analytics

核心指标：

```
Today Cost

$3.24


Today Tokens

5.2M


Requests

2400
```


图表：

- Token Trend
- Model Ranking
- Cost Analysis


---

# 8. Request Logs

类似 VSCode Terminal。


```
10:32:01

POST /chat/completions


Model:
gpt-5


Latency:
320ms


Tokens:
2400


Cost:
$0.02
```


---

# 9. Command Palette


快捷键：

```
Ctrl + K
```


功能：

```
switch provider

restart gateway

view usage

add provider
```


---

# 10. Theme

Dark Mode:

Background:

```
#0F1117
```

Card:

```
#171A21
```

Border:

```
#252A34
```


Accent:

- OpenAI Green
- Claude Orange
- Gemini Blue


---

# 11. Tech Stack


Frontend:

- React
- TailwindCSS
- shadcn/ui
- Zustand
- Recharts
- React Flow


Backend:

- Tauri 2
- Rust
- SQLite


---

# 12. Development Roadmap


## Phase 1

- Sidebar
- Dark Theme
- Provider Card Grid


## Phase 2

- Dashboard
- Usage Analytics


## Phase 3

- React Flow Router
- Real-time Logs


## Phase 4

- Command Palette
- Plugin System


---

# Final Goal

AI Infrastructure Cockpit

不是：

> API 管理后台


而是：

> AI 桌面控制中心
