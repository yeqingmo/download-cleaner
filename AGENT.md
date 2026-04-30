# 角色设定
你是一个资深的 macOS 架构师和高级 Rust 工程师，精通 Rust 系统级编程、macOS 底层 API 调用（如 xattr）以及通过 AppleScript 实现原生 UI 自动化交互。

# 项目背景
我需要开发一个名为 "Mac downloadad-cleaner的轻量级本地后台守护进程（Daemon）。它的核心痛点是：macOS 的下载文件夹经常堆积如山，而市面上的自动化整理工具大多是“静默执行”的，缺乏交互感。我需要一个能够“监听下载、读取来源、带有频次记忆、且弹窗询问归宿”的半自动化工具。该工具必须保持极低的内存占用和极高的稳定性。

# 核心工作流 (Workflow)
1. **监听 (Watch)：** 使用 `notify` crate 持续监听 `~/Downloads` 文件夹，当有新文件完成下载时（排除 `.crdownload`, `.part`, `.download` 等临时后缀以及隐藏文件）触发动作。
2. **提取元数据 (Extract)：** 使用 `xattr` crate 读取新文件的 macOS 扩展属性 `com.apple.metadata:kMDItemWhereFroms`，通过适当的反序列化（bplist解析）或简单的正则过滤，提取出该文件的下载来源域名（Domain）。
3. **读取记忆 (Memory)：** 使用 `serde` 和 `serde_json` 读取本地配置文件 `~/.config/smart_dl_memory.json`，查找该域名历史归档最频繁的 1 到 2 个目标文件夹。
4. **原生交互 (UI)：** 通过 `std::process::Command` 调用 `osascript` 弹出 macOS 原生对话框，不引入沉重的 GUI 库，不脱离当前工作流进行交互。
5. **执行与更新 (Execute & Update)：** 根据用户点击的按钮选项，使用 `std::fs` 移动文件，并更新本地 JSON 记忆库中的权重。

# 详细技术规范与要求

## 1. 记忆库逻辑 (Memory Logic)
* 数据结构应为一个序列化的 JSON，记录每个域名对应的目标文件夹及其选择频次。
  ```json
  {
    "github.com": {
      "/Users/yy/Code": 5,
      "/Users/yy/Downloads": 1
    },
    "sciencedirect.com": {
      "/Users/yy/Documents/Papers": 12
    }
  }
每次用户做出选择后，相应路径的 value +1，并安全地写回本地。

2. 弹窗交互设计 (AppleScript/osascript)
当检测到文件下载完成时，通过子进程执行 AppleScript，弹出如下系统提示框：

标题： 检测到新下载: [文件名]

正文： 来源: [域名]。根据你的习惯，建议移至 [记忆库推算的最高频文件夹名称]。

按钮选项 (最多 3 个)：

移动到 [最高频文件夹] (默认按钮，按回车直接触发)

放着不管 (留在 Downloads 文件夹)

选择其他... (点击后弹出一个标准的 macOS 文件夹选择器 choose folder 供用户自选)

3. 技术栈限制
使用 Rust (2021 Edition) 编写。

核心依赖建议：notify (文件监听), serde_json & serde (配置读写), xattr (扩展属性读取)。

避免使用任何庞大的跨平台 GUI 框架（如 Tauri 或 egui），UI 必须依赖 osascript。

代码必须包含健壮的错误处理机制（使用 Result 和 anyhow），防止后台服务因单个文件的读取失败而崩溃。

你的任务
输出完整的 Cargo.toml 依赖文件。

输出完整、模块化且可读性强的 src/main.rs（或拆分为合适的模块），并包含详细的中文注释。

提供一份简明的 README，说明如何编译构建（release 模式），以及如何编写 .plist 文件将其设置为 macOS 的 launchd 后台自动启动项。