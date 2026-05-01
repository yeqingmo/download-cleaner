# Download Cleaner

English | [中文文档](./README.zh-CN.md)

## About

Download Cleaner is a native macOS download organizer. It watches `~/Downloads`, reads source domains from new files, suggests destinations from your history, and prompts with native dialogs after downloads stabilize. It ships with a background daemon and a Slint-based panel for batch review.

## Features

- Auto-prompts after downloads stabilize
- Groups short bursts from the same source into batch prompts
- Keeps monitoring after the window closes
- Supports stop / restart monitoring
- Panel supports sorting, multi-select, and Shift-range selection
- Move to suggestion, choose a folder, or trash items
- Low-memory daemon

## How It Works

- Daemon: watches Downloads, identifies files, and prompts to organize
- Panel: browses files, batch-moves or deletes them, and controls monitoring

## Install

1. Download the DMG
2. Drag `Download Cleaner.app` to `Applications`
3. Open it once to install the LaunchAgent

## Usage

- Main app: open the app normally
- Panel: `cargo run -- panel`
- Panel alias: `cargo run -- panel-slint`
- AppleScript manager: `cargo run -- panel-script`
- Summary panel: `cargo run -- panel-summary`

## Behavior

- Single files prompt quickly after becoming stable
- Same-source downloads may batch together
- Closing the panel does not stop the daemon
- Use the panel to stop or restart monitoring

## Configuration

```bash
DOWNLOADS_DIR="$HOME/Downloads"
MEMORY_PATH="$HOME/.config/smart_dl_memory.json"
COMPLETE_DELAY_MS=500
BATCH_WINDOW_MS=1500
SCAN_EXISTING=1
```

## Build

```bash
./scripts/package_macos.sh
```

Outputs:

- `dist/Download Cleaner.app`
- `dist/Download Cleaner.dmg`

## Icon

Put a custom icon at:

`assets/macos/AppIcon.icns`

## Uninstall

```bash
launchctl bootout gui/$UID/com.yy.download-cleaner
rm ~/Library/LaunchAgents/com.yy.download-cleaner.plist
```

## Layout

- `src/main.rs`: entry dispatch
- `src/daemon.rs`: watcher and batching
- `src/ui.rs`: native dialogs
- `src/gui_panel_slint.rs`: panel UI
- `src/file_ops.rs`: move/delete/stability checks
- `src/metadata.rs`: source domain extraction
- `src/memory.rs`: preference memory
- `src/config.rs`: env config
- `src/pathing.rs`: path helpers
- `src/types.rs`: shared types
