# Download Cleaner

主流程是一个轻量 Rust 守护进程：平时没有网页和窗口，只有检测到 `~/Downloads` 出现新下载且文件稳定后，才通过 macOS 原生 `osascript` 弹窗询问归档位置。

Node 版本只保留为临时管理面板/原型，不再作为默认主入口。

## 安装 Rust

当前机器还没有 `rustc` 和 `cargo`。装好后重新打开终端或执行对应的环境加载命令：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

也可以用 Homebrew：

```bash
brew install rust
```

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
```

## 可选管理面板

如果只是想临时看一眼 Downloads 和记忆库，可以运行旧的 Node 面板：

```bash
npm run panel
```

然后访问：

```text
http://127.0.0.1:4173
```
