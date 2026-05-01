# Download Cleaner

<p align="center">
  <img src="https://github.com/user-attachments/assets/b3e055fc-ae9e-4581-9b02-a25881eebc2d" width="800" alt="Download Cleaner 10x Demo" />
</p>

English | [中文文档](./README.zh-CN.md)

## About

Download Cleaner is a native macOS download organizer. It watches `~/Downloads`, reads source domains from new files, and helps you move downloads into the folders you already use. After a download stabilizes, it prompts with native dialogs and remembers where files from the same source usually go.

The default workflow is organization by moving, not automatic deletion. Trash actions are available from the panel for batch cleanup when you explicitly choose them, and they move files to the current user's `~/.Trash` (Finder Trash) instead of permanently deleting them.

## Features

- Auto-prompts after downloads stabilize
- Groups short bursts from the same source into batch prompts
- Keeps monitoring after the window closes
- Supports stop / restart monitoring
- Panel supports sorting, multi-select, and Shift-range selection
- Move to suggested folders or choose a destination manually
- Batch trash selected items from the panel
- Low-memory daemon

## How It Works

- Daemon: watches Downloads, identifies files, and prompts you to move them into the right place
- Panel: browses files, supports batch moving, offers explicit trash actions, and controls monitoring

## Safety

- New-download prompts only offer move or ignore choices
- Trash actions are explicit panel actions for selected items
- Trashing moves regular files to the current user's `~/.Trash` (Finder Trash); it does not permanently delete them
- Non-regular files are rejected by the file operation layer

## Install

1. Download the DMG
2. Drag `Download Cleaner.app` to `Applications`
3. Open it once to install the LaunchAgent

## Usage

- Main app: open `Download Cleaner.app` normally
- Background monitoring keeps running after the window closes
- Use the panel to review Downloads, batch-move selected files, trash selected files, or stop / restart monitoring

Developer commands:

```bash
cargo run -- panel
cargo run -- panel-slint
cargo run -- panel-script
cargo run -- panel-summary
```

## Behavior

- Single files prompt quickly after becoming stable
- Same-source downloads may batch together
- Closing the panel does not stop the daemon
- Use the panel to stop or restart monitoring
- The organizer prefers move actions; trashing is an explicit panel action for selected items
- Files sent to Trash can be reviewed or restored from Finder Trash

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
- `src/file_ops.rs`: move/trash/stability checks
- `src/metadata.rs`: source domain extraction
- `src/memory.rs`: preference memory
- `src/config.rs`: env config
- `src/pathing.rs`: path helpers
- `src/types.rs`: shared types
