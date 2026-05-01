# Download Cleaner

主流程是一个轻量 Rust 守护进程：平时没有网页和窗口，只有检测到 `~/Downloads` 出现新下载且文件稳定后，才通过 macOS 原生 `osascript` 弹窗询问归档位置。

同一来源在短时间多文件下载时，会聚合批处理弹窗；默认聚合窗口是 `5s`。

## 代码结构

当前源码已经按职责拆分：

- `src/main.rs`：入口分发（`daemon` / `panel`）。
- `src/daemon.rs`：监听、去抖、批处理分组与流程编排。
- `src/ui.rs`：AppleScript 弹窗、目录选择、轻量摘要面板。
- `src/gui_panel.rs`：Rust 原生管理面板。
- `src/file_ops.rs`：稳定性判断、移动文件、跨卷处理、忽略规则。
- `src/metadata.rs`：xattr + plist 解析下载来源域名。
- `src/memory.rs`：记忆库读写、建议目标、摘要。
- `src/config.rs`：环境变量与默认配置。
- `src/pathing.rs`：路径与通用工具函数。
- `src/types.rs`：共享数据结构。

## 编译

```bash
cargo build --release
```

产物在：

```text
target/release/download-cleaner
```

## 运行

```bash
./target/release/download-cleaner
```

默认只监听启动后的新下载，不会把 Downloads 里已有文件挨个弹一遍。调试时如果想处理现有文件：

```bash
SCAN_EXISTING=1 ./target/release/download-cleaner
```

## 弹窗行为

检测到新下载后会弹出系统对话框：

- 有历史建议：`移动到 [目录名]`、`放着不管`、`选择其他...`
- 没有历史建议：`选择其他...`、`放着不管`

`选择其他...` 会打开 macOS 原生文件夹选择器。移动成功后会更新：

```text
~/.config/smart_dl_memory.json
```

## launchd 自启动

模板在：

```text
launchd/com.yy.download-cleaner.plist
```

安装：

```bash
mkdir -p ~/Library/LaunchAgents
cp launchd/com.yy.download-cleaner.plist ~/Library/LaunchAgents/
launchctl load ~/Library/LaunchAgents/com.yy.download-cleaner.plist
```

停止：

```bash
launchctl unload ~/Library/LaunchAgents/com.yy.download-cleaner.plist
```

日志：

```text
/tmp/download-cleaner.out.log
/tmp/download-cleaner.err.log
```

## 环境变量

```bash
DOWNLOADS_DIR="$HOME/Downloads" ./target/release/download-cleaner
MEMORY_PATH="$HOME/.config/smart_dl_memory.json" ./target/release/download-cleaner
COMPLETE_DELAY_MS=1500 ./target/release/download-cleaner
BATCH_WINDOW_MS=5000 ./target/release/download-cleaner
```

## 原生管理面板（无 Node）

Rust 二进制默认管理器是原生 GUI（Rust）：

```bash
./target/release/download-cleaner panel
```

可用于：

- 列出当前 Downloads 文件（来源域名 + 建议目录）。
- 首页直接点击交互（不再进入二级操作面板）。
- `移动到建议目录`、`选择其他目录`、`放着不管`。
- `删除到废纸篓`（不依赖 Finder 删除）。
- 列表行使用交替底色区分相邻文件。

如果你想用 AppleScript 列表版管理器，也保留了：

```bash
./target/release/download-cleaner panel-script
```

如果你只想看轻量摘要菜单，也保留了：

```bash
./target/release/download-cleaner panel-summary
```
