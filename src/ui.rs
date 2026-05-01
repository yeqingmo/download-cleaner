use crate::memory::memory_summary;
use crate::pathing::{escape_applescript, folder_name};
use crate::types::{AppConfig, BatchChoice, Memory, ReadyDownload, UserChoice};
use anyhow::{anyhow, Context, Result};
use std::path::PathBuf;
use std::process::Command;

pub fn prompt_user(
    file_name: &str,
    domain: &str,
    suggestion: Option<&(PathBuf, u64)>,
) -> Result<UserChoice> {
    let title = format!("检测到新下载: {file_name}");

    if let Some((target, _count)) = suggestion {
        let target_name = folder_name(target);
        let move_button = format!("移动到 {target_name}");
        let body = format!("来源: {domain}。根据你的习惯，建议移至 {target_name}。");
        let script = format!(
            r#"use scripting additions
set resultButton to button returned of (display dialog "{}" with title "{}" buttons {{"放着不管", "选择其他...", "{}"}} default button "{}" with icon note)
return resultButton"#,
            escape_applescript(&body),
            escape_applescript(&title),
            escape_applescript(&move_button),
            escape_applescript(&move_button),
        );
        let answer = run_osascript(&script)?;

        if answer == "放着不管" {
            Ok(UserChoice::Ignore)
        } else if answer == "选择其他..." {
            Ok(UserChoice::ChooseOther)
        } else {
            Ok(UserChoice::MoveTo(target.clone()))
        }
    } else {
        let body = format!("来源: {domain}。暂无历史建议，可以选择一个归档目录。");
        let script = format!(
            r#"use scripting additions
set resultButton to button returned of (display dialog "{}" with title "{}" buttons {{"放着不管", "选择其他..."}} default button "选择其他..." with icon note)
return resultButton"#,
            escape_applescript(&body),
            escape_applescript(&title),
        );
        let answer = run_osascript(&script)?;

        if answer == "放着不管" {
            Ok(UserChoice::Ignore)
        } else {
            Ok(UserChoice::ChooseOther)
        }
    }
}

pub fn prompt_batch_user(
    domain: &str,
    downloads: &[ReadyDownload],
    suggestion: Option<&(PathBuf, u64)>,
) -> Result<BatchChoice> {
    let count = downloads.len();
    let sample = downloads
        .iter()
        .take(3)
        .map(|download| download.file_name.as_str())
        .collect::<Vec<_>>()
        .join("、");
    let title = format!("检测到同源下载: {domain}（{count} 个）");

    if let Some((target, _)) = suggestion {
        let target_name = folder_name(target);
        let move_button = format!("全部移到 {target_name}");
        let body = format!(
            "来源: {domain}。共 {count} 个文件：{sample}{}。建议统一移至 {target_name}。",
            if count > 3 { " 等" } else { "" }
        );
        let script = format!(
            r#"use scripting additions
set resultButton to button returned of (display dialog "{}" with title "{}" buttons {{"放着不管", "逐个处理", "选择其他...", "{}"}} default button "{}" with icon note)
return resultButton"#,
            escape_applescript(&body),
            escape_applescript(&title),
            escape_applescript(&move_button),
            escape_applescript(&move_button),
        );
        let answer = run_osascript(&script)?;

        if answer == "放着不管" {
            Ok(BatchChoice::IgnoreAll)
        } else if answer == "逐个处理" {
            Ok(BatchChoice::OneByOne)
        } else if answer == "选择其他..." {
            Ok(BatchChoice::ChooseOtherAll)
        } else {
            Ok(BatchChoice::MoveAllTo(target.clone()))
        }
    } else {
        let body = format!(
            "来源: {domain}。共 {count} 个文件：{sample}{}。当前没有历史建议。",
            if count > 3 { " 等" } else { "" }
        );
        let script = format!(
            r#"use scripting additions
set resultButton to button returned of (display dialog "{}" with title "{}" buttons {{"放着不管", "逐个处理", "选择其他..."}} default button "选择其他..." with icon note)
return resultButton"#,
            escape_applescript(&body),
            escape_applescript(&title),
        );
        let answer = run_osascript(&script)?;

        if answer == "放着不管" {
            Ok(BatchChoice::IgnoreAll)
        } else if answer == "逐个处理" {
            Ok(BatchChoice::OneByOne)
        } else {
            Ok(BatchChoice::ChooseOtherAll)
        }
    }
}

pub fn choose_folder(file_name: &str) -> Result<PathBuf> {
    let prompt = format!("选择「{file_name}」的归档目录");
    let script = format!(
        r#"use scripting additions
set selectedPath to POSIX path of (choose folder with prompt "{}")
return selectedPath"#,
        escape_applescript(&prompt)
    );
    let output = run_osascript(&script)?;
    Ok(PathBuf::from(output.trim()))
}

pub fn choose_batch_folder(domain: &str, count: usize) -> Result<PathBuf> {
    let prompt = format!("为 {domain} 的 {count} 个文件选择目标目录");
    let script = format!(
        r#"use scripting additions
set selectedPath to POSIX path of (choose folder with prompt "{}")
return selectedPath"#,
        escape_applescript(&prompt)
    );
    let output = run_osascript(&script)?;
    Ok(PathBuf::from(output.trim()))
}

pub fn run_native_panel(config: &AppConfig, memory: &Memory) -> Result<()> {
    let summary = memory_summary(memory);
    let script = r#"use scripting additions
on run argv
set msg to item 1 of argv
set resultButton to button returned of (display dialog msg with title "Download Cleaner 管理" buttons {"退出", "打开记忆库文件", "复制摘要"} default button "复制摘要" with icon note)
return resultButton
end run"#;
    let answer = run_osascript_script_with_args(script, &[summary.as_str()])?;

    if answer == "打开记忆库文件" {
        let _ = Command::new("open").arg(&config.memory_path).status();
    } else if answer == "复制摘要" {
        copy_to_clipboard(&summary)?;
    }

    Ok(())
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    let output = Command::new("osascript")
        .arg("-e")
        .arg(format!(
            r#"use scripting additions
set the clipboard to "{}""#,
            escape_applescript(text)
        ))
        .output()
        .context("无法复制到剪贴板")?;

    if !output.status.success() {
        return Err(anyhow!(
            "复制失败: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }

    Ok(())
}

pub fn run_osascript(script: &str) -> Result<String> {
    run_osascript_script_with_args(script, &[])
}

pub fn run_osascript_script_with_args(script: &str, args: &[&str]) -> Result<String> {
    let mut command = Command::new("osascript");
    command.arg("-l").arg("AppleScript").arg("-e").arg(script);
    for arg in args {
        command.arg(arg);
    }

    let output = command.output().context("无法执行 osascript")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(anyhow!("osascript 失败: {}", stderr.trim()));
    }

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}
