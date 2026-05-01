use crate::file_ops::{move_and_remember, should_ignore_path, trash_path};
use crate::memory::{read_memory, top_destination};
use crate::metadata::extract_source_domain;
use crate::pathing::file_name;
use crate::types::AppConfig;
use crate::ui::{choose_folder, run_osascript_script_with_args};
use anyhow::{anyhow, Result};
use std::fs;
use std::path::PathBuf;
use std::process::Command;

#[derive(Debug, Clone)]
struct ManageItem {
    path: PathBuf,
    file_name: String,
    domain: String,
    suggestion: Option<PathBuf>,
}

#[derive(Debug, Clone)]
enum EntryAction {
    MoveSuggested(usize),
    ChooseOther(usize),
    Ignore(usize),
    DeleteTrash(usize),
    RevealInFinder(usize),
}

pub fn run_manager(config: &AppConfig) -> Result<()> {
    loop {
        let memory = read_memory(&config.memory_path)?;
        let mut items = load_items(config, &memory)?;

        if items.is_empty() {
            let action = empty_state_action(&config.memory_path)?;
            match action.as_str() {
                "打开下载文件夹" => {
                    let _ = Command::new("open").arg(&config.downloads_dir).status();
                }
                "打开记忆库文件" => {
                    let _ = Command::new("open").arg(&config.memory_path).status();
                }
                _ => return Ok(()),
            }
            continue;
        }

        items.sort_by(|a, b| a.file_name.cmp(&b.file_name));
        let (labels, actions) = build_action_entries(&items);
        let picked = choose_action_entry(&labels)?;
        let Some(picked_label) = picked else {
            return Ok(());
        };

        let idx = labels
            .iter()
            .position(|value| value == &picked_label)
            .ok_or_else(|| anyhow!("无法定位选择项"))?;
        let action = actions
            .get(idx)
            .cloned()
            .ok_or_else(|| anyhow!("动作索引越界"))?;

        execute_action(config, &items, action)?;
    }
}

fn load_items(config: &AppConfig, memory: &crate::types::Memory) -> Result<Vec<ManageItem>> {
    let mut items = Vec::new();

    for entry in fs::read_dir(&config.downloads_dir)? {
        let entry = match entry {
            Ok(value) => value,
            Err(_) => continue,
        };
        let path = entry.path();
        if should_ignore_path(&path) {
            continue;
        }
        let metadata = match fs::metadata(&path) {
            Ok(value) => value,
            Err(_) => continue,
        };
        if !metadata.is_file() {
            continue;
        }
        let domain = extract_source_domain(&path).unwrap_or_else(|_| "未知来源".to_string());
        let suggestion =
            top_destination(memory, &domain, &config.downloads_dir).map(|(target, _)| target);
        items.push(ManageItem {
            file_name: file_name(&path),
            path,
            domain,
            suggestion,
        });
    }

    Ok(items)
}

fn build_action_entries(items: &[ManageItem]) -> (Vec<String>, Vec<EntryAction>) {
    let mut labels = Vec::new();
    let mut actions = Vec::new();

    for (idx, item) in items.iter().enumerate() {
        let line_tag = if idx % 2 == 0 { "[白]" } else { "[灰]" };
        let suggest = item
            .suggestion
            .as_ref()
            .and_then(|path| path.file_name())
            .and_then(|name| name.to_str())
            .unwrap_or("无建议");
        let base = format!(
            "{line_tag} {} · {} · {}",
            item.file_name, item.domain, suggest
        );

        labels.push(format!("{} ｜移动到建议目录", base));
        actions.push(EntryAction::MoveSuggested(idx));

        labels.push(format!("{} ｜选择其他目录...", base));
        actions.push(EntryAction::ChooseOther(idx));

        labels.push(format!("{} ｜放着不管", base));
        actions.push(EntryAction::Ignore(idx));

        labels.push(format!("{} ｜删除到废纸篓", base));
        actions.push(EntryAction::DeleteTrash(idx));

        labels.push(format!("{} ｜在访达中显示", base));
        actions.push(EntryAction::RevealInFinder(idx));
    }

    (labels, actions)
}

fn choose_action_entry(items: &[String]) -> Result<Option<String>> {
    let script = r#"use scripting additions
on run argv
set picked to choose from list argv with title "Download Cleaner 管理" with prompt "选择动作并直接执行" OK button name "执行" cancel button name "退出"
if picked is false then return "__CANCEL__"
return item 1 of picked
end run"#;
    let argv: Vec<&str> = items.iter().map(String::as_str).collect();
    let result = run_osascript_script_with_args(script, &argv)?;
    if result == "__CANCEL__" {
        Ok(None)
    } else {
        Ok(Some(result))
    }
}

fn execute_action(config: &AppConfig, items: &[ManageItem], action: EntryAction) -> Result<()> {
    match action {
        EntryAction::MoveSuggested(index) => {
            let item = items.get(index).ok_or_else(|| anyhow!("索引越界"))?;
            if let Some(target) = item.suggestion.as_ref() {
                move_and_remember(config, &item.path, &item.domain, target)?;
            } else {
                let target = choose_folder(&item.file_name)?;
                move_and_remember(config, &item.path, &item.domain, &target)?;
            }
        }
        EntryAction::ChooseOther(index) => {
            let item = items.get(index).ok_or_else(|| anyhow!("索引越界"))?;
            let target = choose_folder(&item.file_name)?;
            move_and_remember(config, &item.path, &item.domain, &target)?;
        }
        EntryAction::Ignore(_index) => {}
        EntryAction::DeleteTrash(index) => {
            let item = items.get(index).ok_or_else(|| anyhow!("索引越界"))?;
            trash_path(&item.path)?;
        }
        EntryAction::RevealInFinder(index) => {
            let item = items.get(index).ok_or_else(|| anyhow!("索引越界"))?;
            let _ = Command::new("open").arg("-R").arg(&item.path).status();
        }
    }
    Ok(())
}

fn empty_state_action(memory_path: &PathBuf) -> Result<String> {
    let script = r#"use scripting additions
on run argv
set m to item 1 of argv
set msg to "当前没有可管理的下载文件。" & return & "记忆库: " & m
set picked to choose from list {"打开下载文件夹", "打开记忆库文件", "退出"} with title "Download Cleaner 管理" with prompt msg OK button name "执行" cancel button name "退出"
if picked is false then return "退出"
return item 1 of picked
end run"#;
    run_osascript_script_with_args(script, &[&memory_path.to_string_lossy()])
}
