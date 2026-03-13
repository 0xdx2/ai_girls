<div align="center">

# 🎀 AI Girls Desktop

**macOS AI 桌面助手 — Live2D 虚拟形象 × 多模型 AI 大脑**

[![Rust](https://img.shields.io/badge/Rust-1.85+-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org/)
[![Tauri](https://img.shields.io/badge/Tauri-v1-24C8D8?logo=tauri&logoColor=white)](https://tauri.app/)
[![Platform](https://img.shields.io/badge/Platform-macOS-000?logo=apple&logoColor=white)](https://www.apple.com/macos/)
[![License](https://img.shields.io/badge/License-MIT-green)](LICENSE)

</div>

---

## ✨ 它是什么

AI Girls Desktop 是一款运行在 macOS 上的 AI 桌面助手，将 **Live2D 动态形象**与 **本地多模型 AI 推理**融合在一起。她会动、会说话、会根据上下文自动切换角色与服装，同时帮你完成编程、搜索、规划等日常任务。

---

## 📸 界面预览

### 启动 & 待机

<img src="docs/init.jpg" alt="启动界面" width="720"/>

> 应用启动后，Live2D 角色进入待机状态，背景光晕随状态动态变化。

---

### 向 AI 提问

<img src="docs/ai_ask.jpg" alt="AI 对话" width="720"/>

> 在右侧对话面板输入问题，AI 思考时角色头顶会出现「漫画思考泡泡」，回答流式渲染。

---

### Live2D 动作演示

<table>
  <tr>
    <td><img src="docs/motion_1.jpg" alt="动作演示 1" width="360"/></td>
    <td><img src="docs/motion_2.jpg" alt="动作演示 2" width="360"/></td>
  </tr>
  <tr>
    <td align="center">动作组 1 — 待机/呼吸</td>
    <td align="center">动作组 2 — 响应/互动</td>
  </tr>
</table>

---

## 🧠 AI 多模型支持

自动按优先级探测本地已安装的 CLI 工具，无需 API Key 配置文件：

| 优先级 | 模型 | 调用方式 |
|:---:|---|---|
| 1 | **Claude** (Anthropic) | `claude -p` |
| 2 | **Codex** (OpenAI) | `codex --approvals=never -q` |
| 3 | **GitHub Copilot** | `gh copilot explain` |
| 4 | **Gemini** (Google) | `gemini -p` |
| 5 | Mock（离线备用） | 内置 |

---

## 🎭 角色系统

AI 会根据对话上下文自动切换人设，每种角色有独立的主题色、表情和 Live2D 动作组：

| 角色 | 图标 | 触发场景 | 主题色 |
|---|:---:|---|---|
| Assistant | 🌸 | 默认对话 | 粉紫 |
| Coder | 💻 | 代码 / Rust / Python 相关 | 青金 |
| Researcher | 🔍 | 搜索 / 浏览 / 调研 | 蓝绿 |
| Planner | 📋 | 计划 / 任务 / TODO | 橙金 |
| Analyst | 📊 | 数据 / 指标 / 分析 | 紫蓝 |
| Security | 🛡️ | 安全 / 审计 / 漏洞 | 红黑 |
| Orchestrator | 🎼 | 多 Agent / 编排 | 金紫 |

---

## 🛠️ 内置工具能力

AI 可以在对话中直接调用以下工具：

- **Terminal** — 安全只读 Shell 命令（`ls` / `cat` / `grep` / `rg` 等白名单）
- **ReadFile** — 读取本地文件（最多 10,000 字符）
- **BrowsePage** — `curl` 抓取网页并提取纯文本（最多 8,000 字符，仅 http/https）
- **ListDir** — 列出目录结构
- **SearchFiles** — 使用 `rg` / `grep` 搜索文件内容
- **McpCall** — 通过 stdio 调用 MCP 服务器（JSON-RPC）

---

## 🎨 Live2D 角色资产

项目内置多套角色供切换：

| 目录 | 角色 | 格式 |
|---|---|---|
| `dujiaoshou_4/` | 独角兽系 | Cubism 4 (moc3) |
| `kelifulan_3/` | 克里夫兰 | Cubism 3 (moc3) |
| `mashiro_*/` | 真白 (三套服装) | Cubism 2 (moc) |
| `sagiri/` | 紗霧 | Cubism 2 (moc) |
| `nep/` | 涅普 | Cubism 2 (moc) |
| `tia/` | Tia | Cubism 2 (moc) |
| `xianghe_2/` | 香荷 | Cubism 3 (moc3) |

---

## 🚀 快速开始

### 环境要求

- macOS 12 Monterey 或更高
- [Rust 1.85+](https://rustup.rs/)
- [Node.js 18+](https://nodejs.org/)（用于 Tauri 前端工具链）
- [Tauri CLI v1](https://tauri.app/v1/guides/getting-started/prerequisites/)

### 安装 & 运行

```bash
# 克隆仓库
git clone https://github.com/0xdx2/ai_girls.git
cd ai_girls

# 开发模式启动（自动热重载）
cargo tauri dev --manifest-path sarah-tauri/Cargo.toml

# 生产打包（输出 .app + .dmg）
cargo tauri build --manifest-path sarah-tauri/Cargo.toml
```

### AI 模型配置（可选）

无需配置文件 — 只要本地装了对应 CLI 工具，启动时自动发现：

```bash
# Claude
brew install claude   # 或参考 https://docs.anthropic.com/

# GitHub Copilot CLI
gh extension install github/gh-copilot

# Gemini CLI
pip install gemini-cli
```

---

## 🔐 macOS 权限说明

首次运行时，应用会自动检测以下权限状态：

| 权限 | 用途 |
|---|---|
| **辅助功能 (Accessibility)** | macOS 自动化操作 |
| **麦克风** | 语音输入管线（可选） |
| **屏幕录制** | 屏幕上下文感知（可选） |

可通过环境变量跳过检测（CI 环境）：

```bash
export MACOS_ACCESSIBILITY_GRANTED=1
export MACOS_MICROPHONE_GRANTED=1
export MACOS_SCREEN_RECORDING_GRANTED=1
```

---

## 🏗️ 项目结构

```
ai_girls/
├── sarah-tauri/          # Rust 后端 (Tauri 进程)
│   └── src/
│       ├── main.rs           # Tauri 命令注册 & 状态管理
│       ├── ai_adapters.rs    # 多模型 CLI 适配器 & Fallback 链
│       ├── orchestrator.rs   # Agent 编排 & 多轮对话循环
│       ├── persona_system.rs # 角色系统 & 人设配置
│       ├── tool_runtime.rs   # 安全工具执行沙箱
│       ├── voice_pipeline.rs # 语音处理管线
│       ├── avatar_runtime.rs # Live2D 运行时控制
│       └── macos_integration.rs # macOS 权限 & 自动化
└── sarah-ui/             # 前端 (原生 JS + Pixi.js)
    ├── index.html
    ├── app.js
    ├── styles.css
    ├── assets/           # Live2D 模型资产
    └── vendor/           # Live2D Cubism SDK
```

---

## 🤝 贡献

欢迎 Issue 和 PR！提交前请确保：

```bash
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

---

## 📄 License

MIT — 详见 [LICENSE](LICENSE)

> Live2D 模型资产版权归原作者所有，使用前请阅读各模型目录内的许可协议。
