# Download Cleaner

Download Cleaner 是一个轻量级 macOS 下载整理工具。

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
- 支持移动到建议目录、选择目录、删除到废纸篓
- 轻量化，daemon内存占用仅2 Mb

## 工作方式

程序分两部分：

- 后台 daemon：负责监听 Downloads、识别文件、弹出整理流程
- 管理面板：负责查看文件、批量操作、控制监控状态

## 安装

1. 下载 DMG
2. 拖动 `Download Cleaner.app` 到 `Applications`
3. 首次打开会自动安装后台监控

## 使用

### 常规使用

直接打开应用即可，关闭GUI后不影响daemon。

### 管理面板

```bash
cargo run -- panel
```

也可用别名：

```bash
cargo run -- panel-slint
```

### 命令行模式

- `panel-script`：AppleScript 版管理器
- `panel-summary`：轻量摘要面板

## 监控行为

- 单文件会优先快速弹窗
- 同一来源持续到来时，会进入批处理弹窗
- 关闭面板不会停止后台监控
- 你可以在面板里手动停止 / 重启监控

## 配置

环境变量：

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

```text
assets/macos/AppIcon.icns
```

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
- `src/file_ops.rs`：移动 / 删除 / 稳定性判断
- `src/metadata.rs`：来源域名解析
- `src/memory.rs`：记忆库
- `src/config.rs`：环境变量
- `src/pathing.rs`：路径工具
- `src/types.rs`：共享数据结构
