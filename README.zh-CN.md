# Download Cleaner

<p align="center">
  <img src="https://github.com/user-attachments/assets/b3e055fc-ae9e-4581-9b02-a25881eebc2d" width="800" alt="Download Cleaner 10x Demo" />
</p>

中文 | [English](./README.md)

## About

Download Cleaner 是一个 macOS 原生的下载整理工具。它常驻监听 `~/Downloads`，根据来源域名和历史习惯推荐归档目录，在新下载稳定后弹出系统原生提示，帮你把文件移动到本来就会使用的目录里。

它的默认工作流是移动归档，不是自动删除。删除只作为管理面板里的显式批量维护动作存在，需要你主动选择文件后触发；实际行为是移入当前用户的 `~/.Trash`（访达废纸篓），不是永久删除。

它会：
- 监控 `~/Downloads`
- 识别新下载的来源域名
- 根据历史习惯推荐归档目录
- 支持单文件处理和同源批量处理
- 提供后台监控 daemon 和可视化管理面板

## 特性

- 新下载稳定后自动弹窗
- 同一来源短时间内连续下载可自动聚合
- 关闭窗口不影响后台监控
- 支持停止 / 重启监控
- 面板支持排序、多选、Shift 连选
- 支持移动到建议目录或手动选择目录
- 支持在面板中批量将选中文件移到废纸篓
- 轻量化，daemon 内存占用较低

## 工作方式

- 后台 daemon：负责监听 Downloads、识别文件、弹出移动归档流程
- 管理面板：负责查看文件、批量移动、显式废纸篓操作和监控状态控制

## 安全边界

- 新下载弹窗只提供移动或忽略，不会直接删除
- 废纸篓操作只在面板中对选中文件显式执行
- 移入废纸篓会把普通文件移动到当前用户的 `~/.Trash`（访达废纸篓），不是永久删除
- 文件操作层会拒绝处理非普通文件

## 安装

1. 下载 DMG
2. 拖动 `Download Cleaner.app` 到 `Applications`
3. 首次打开会自动安装后台监控

## 使用

- 常规使用：直接打开 `Download Cleaner.app`
- 关闭 GUI 后，后台监控仍会继续运行
- 在管理面板里可以查看 Downloads、批量移动选中文件、将选中文件移到废纸篓，或停止 / 重启监控

开发调试入口：

```bash
cargo run -- panel
cargo run -- panel-slint
cargo run -- panel-script
cargo run -- panel-summary
```

## 监控行为

- 单文件会优先快速弹窗
- 同一来源持续到来时，会进入批处理弹窗
- 关闭面板不会停止后台监控
- 你可以在面板里手动停止 / 重启监控
- 默认整理动作以移动归档为主；删除是面板中针对选中文件的显式操作
- 移入废纸篓后的文件可以在访达废纸篓中查看或恢复

## 配置

```bash
DOWNLOADS_DIR="$HOME/Downloads"
MEMORY_PATH="$HOME/.config/smart_dl_memory.json"
COMPLETE_DELAY_MS=500
BATCH_WINDOW_MS=1500
SCAN_EXISTING=1
```

## 打包

```bash
./scripts/package_macos.sh
```

产物：

- `dist/Download Cleaner.app`
- `dist/Download Cleaner.dmg`

## 图标

放置自定义图标文件：

`assets/macos/AppIcon.icns`

然后重新打包即可。

## 卸载

```bash
launchctl bootout gui/$UID/com.yy.download-cleaner
rm ~/Library/LaunchAgents/com.yy.download-cleaner.plist
```

## 目录结构

- `src/main.rs`：入口分发
- `src/daemon.rs`：监听与批处理
- `src/ui.rs`：系统弹窗
- `src/gui_panel_slint.rs`：管理面板
- `src/file_ops.rs`：移动 / 废纸篓 / 稳定性判断
- `src/metadata.rs`：来源域名解析
- `src/memory.rs`：记忆库
- `src/config.rs`：环境变量
- `src/pathing.rs`：路径工具
- `src/types.rs`：共享数据结构
