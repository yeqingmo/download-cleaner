use crate::file_ops::{move_and_remember, should_ignore_path, trash_path};
use crate::memory::{read_memory, top_destination};
use crate::metadata::extract_source_domain;
use crate::pathing::{file_name, folder_name};
use crate::types::AppConfig;
use anyhow::{anyhow, Result};
use rfd::FileDialog;
use slint::{ComponentHandle, ModelRc, SharedString, VecModel};
use std::cell::RefCell;
use std::cmp::Ordering;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::rc::Rc;
use std::time::{Duration, SystemTime};

slint::slint! {
import { VerticalBox, HorizontalBox, Button, LineEdit, ListView } from "std-widgets.slint";

export struct TableRow {
    name: string,
    domain: string,
    size: string,
    time: string,
    selected: bool,
}

export component SlimPanel inherits Window {
    width: 1160px;
    height: 760px;
    title: "Download Cleaner";

    in-out property <string> status_text: "已加载";
    in-out property <string> monitoring_text: "监控：未知";
    in-out property <string> search_text;
    in-out property <string> counter_text: "0 / 0";
    in-out property <string> suggestion_text: "建议目录：-";
    in-out property <string> selected_path_text: "路径：-";
    in-out property <string> name_header: "名称";
    in-out property <string> domain_header: "来源";
    in-out property <string> size_header: "大小";
    in-out property <string> time_header: "时间";
    in-out property <[TableRow]> rows: [];
    in-out property <bool> has_selection: false;

    callback refresh();
    callback open_downloads();
    callback open_memory();
    callback stop_monitoring();
    callback restart_monitoring();
    callback apply_search();
    callback select_all_visible();
    callback clear_selection();
    callback sort_name();
    callback sort_domain();
    callback sort_size();
    callback sort_time();
    callback row_click(int, bool);
    callback move_suggested();
    callback choose_other();
    callback trash_item();

    VerticalBox {
        spacing: 10px;
        padding: 12px;

        HorizontalBox {
            spacing: 8px;
            Button { text: "刷新"; clicked => { root.refresh(); } }
            Button { text: "打开下载文件夹"; clicked => { root.open_downloads(); } }
            Button { text: "打开记忆库"; clicked => { root.open_memory(); } }
            Button { text: "停止监控"; clicked => { root.stop_monitoring(); } }
            Button { text: "重启监控"; clicked => { root.restart_monitoring(); } }
            Text { text: root.monitoring_text; vertical-alignment: center; }
        }

        HorizontalBox {
            spacing: 8px;
            Text { text: "搜索："; vertical-alignment: center; }
            LineEdit { text <=> root.search_text; }
            Button { text: "应用"; clicked => { root.apply_search(); } }
            Button { text: "全选可见"; clicked => { root.select_all_visible(); } }
            Button { text: "清空选择"; clicked => { root.clear_selection(); } }
            Text { text: root.counter_text; vertical-alignment: center; }
        }

        Rectangle {
            background: #f8fafc;
            border-width: 1px;
            border-color: #d9dde5;
            border-radius: 8px;
            min-height: 470px;

            VerticalBox {
                spacing: 0px;

                Rectangle {
                    height: 38px;
                    background: #eef2f7;
                    border-width: 0px;
                    HorizontalBox {
                        spacing: 0px;
                        Button {
                            text: root.name_header;
                            clicked => { root.sort_name(); }
                            width: 450px;
                        }
                        Button {
                            text: root.domain_header;
                            clicked => { root.sort_domain(); }
                            width: 270px;
                        }
                        Button {
                            text: root.size_header;
                            clicked => { root.sort_size(); }
                            width: 140px;
                        }
                        Button {
                            text: root.time_header;
                            clicked => { root.sort_time(); }
                            width: 170px;
                        }
                    }
                }

                ListView {
                    for row[i] in root.rows : Rectangle {
                        height: 34px;
                        width: parent.width;
                        background: row.selected ? #dbeafe : (Math.mod(i, 2) == 0 ? #ffffff : #f8fafc);

                        HorizontalBox {
                            spacing: 0px;
                            Rectangle {
                                width: 450px;
                                Text {
                                    x: 8px;
                                    width: parent.width - 16px;
                                    text: row.name;
                                    vertical-alignment: center;
                                    wrap: no-wrap;
                                    overflow: elide;
                                }
                            }
                            Rectangle {
                                width: 270px;
                                Text {
                                    x: 8px;
                                    width: parent.width - 16px;
                                    text: row.domain;
                                    vertical-alignment: center;
                                    wrap: no-wrap;
                                    overflow: elide;
                                }
                            }
                            Rectangle {
                                width: 140px;
                                Text {
                                    x: 8px;
                                    width: parent.width - 16px;
                                    text: row.size;
                                    vertical-alignment: center;
                                    wrap: no-wrap;
                                    overflow: elide;
                                }
                            }
                            Rectangle {
                                width: 170px;
                                Text {
                                    x: 8px;
                                    width: parent.width - 16px;
                                    text: row.time;
                                    vertical-alignment: center;
                                    wrap: no-wrap;
                                    overflow: elide;
                                }
                            }
                        }

                        TouchArea {
                            width: parent.width;
                            height: parent.height;
                            mouse-cursor: pointer;
                            pointer-event(event) => {
                                if (event.button == PointerEventButton.left && event.kind == PointerEventKind.up) {
                                    root.row_click(i, event.modifiers.shift);
                                }
                            }
                        }
                    }
                }
            }
        }

        Text {
            text: root.suggestion_text;
            color: #495467;
        }

        Text {
            text: root.selected_path_text;
            color: #495467;
            wrap: no-wrap;
            overflow: elide;
        }

        HorizontalBox {
            spacing: 8px;
            Button {
                text: "移到建议";
                enabled: root.has_selection;
                clicked => { root.move_suggested(); }
            }
            Button {
                text: "选择目录";
                enabled: root.has_selection;
                clicked => { root.choose_other(); }
            }
            Button {
                text: "删除";
                enabled: root.has_selection;
                clicked => { root.trash_item(); }
            }
        }

        Text {
            text: root.status_text;
            wrap: word-wrap;
            color: #4b5563;
        }
    }
}
}

#[derive(Clone)]
struct RowItem {
    path: PathBuf,
    name: String,
    domain: String,
    suggestion: Option<PathBuf>,
    size_bytes: u64,
    modified_ms: u128,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SortBy {
    NameAsc,
    NameDesc,
    DomainAsc,
    DomainDesc,
    SizeAsc,
    SizeDesc,
    TimeAsc,
    TimeDesc,
}

struct PanelState {
    config: AppConfig,
    items: Vec<RowItem>,
    visible: Vec<usize>,
    selected: HashSet<PathBuf>,
    focus_path: Option<PathBuf>,
    anchor_path: Option<PathBuf>,
    search: String,
    sort_by: SortBy,
    status: String,
    monitoring_running: Option<bool>,
}

impl PanelState {
    fn new(config: AppConfig) -> Result<Self> {
        let mut state = Self {
            config,
            items: Vec::new(),
            visible: Vec::new(),
            selected: HashSet::new(),
            focus_path: None,
            anchor_path: None,
            search: String::new(),
            sort_by: SortBy::TimeDesc,
            status: "已加载".to_string(),
            monitoring_running: crate::launch_agent::monitoring_running().ok(),
        };
        state.reload(None)?;
        Ok(state)
    }

    fn reload(&mut self, preferred_path: Option<PathBuf>) -> Result<()> {
        let memory = read_memory(&self.config.memory_path)?;
        let mut items = Vec::new();

        for entry in fs::read_dir(&self.config.downloads_dir)? {
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

            let modified_ms = metadata
                .modified()
                .unwrap_or(SystemTime::UNIX_EPOCH)
                .duration_since(SystemTime::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis();
            let domain = extract_source_domain(&path).unwrap_or_else(|_| "未知来源".to_string());
            let suggestion =
                top_destination(&memory, &domain, &self.config.downloads_dir).map(|(path, _)| path);
            items.push(RowItem {
                name: file_name(&path),
                path,
                domain,
                suggestion,
                size_bytes: metadata.len(),
                modified_ms,
            });
        }

        self.items = items;
        self.selected.retain(|path| self.items.iter().any(|item| &item.path == path));
        if self
            .anchor_path
            .as_ref()
            .is_some_and(|path| !self.items.iter().any(|item| &item.path == path))
        {
            self.anchor_path = None;
        }
        self.recompute_visible();
        self.reconcile_focus(preferred_path);
        Ok(())
    }

    fn reconcile_focus(&mut self, preferred_path: Option<PathBuf>) {
        if self.visible.is_empty() {
            self.focus_path = None;
            self.anchor_path = None;
            return;
        }

        if let Some(path) = preferred_path.filter(|path| self.path_is_visible(path)) {
            self.focus_path = Some(path);
            return;
        }

        if let Some(path) = self.focus_path.clone().filter(|path| self.path_is_visible(path)) {
            self.focus_path = Some(path);
            return;
        }

        if let Some(path) = self.selected_path_in_visible_order() {
            self.focus_path = Some(path);
            return;
        }

        self.focus_path = self.visible.first().and_then(|idx| self.items.get(*idx).map(|item| item.path.clone()));
    }

    fn path_is_visible(&self, path: &PathBuf) -> bool {
        self.visible
            .iter()
            .any(|idx| self.items.get(*idx).map(|item| &item.path == path).unwrap_or(false))
    }

    fn selected_path_in_visible_order(&self) -> Option<PathBuf> {
        self.visible.iter().find_map(|idx| {
            let item = self.items.get(*idx)?;
            if self.selected.contains(&item.path) {
                Some(item.path.clone())
            } else {
                None
            }
        })
    }

    fn recompute_visible(&mut self) {
        let keyword = self.search.trim().to_lowercase();
        self.visible = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if keyword.is_empty() {
                    return true;
                }
                item.name.to_lowercase().contains(&keyword)
                    || item.domain.to_lowercase().contains(&keyword)
            })
            .map(|(idx, _)| idx)
            .collect();

        let sort_by = self.sort_by;
        self.visible.sort_by(|left_idx, right_idx| {
            compare_rows(&self.items[*left_idx], &self.items[*right_idx], sort_by)
        });
    }

    fn set_search(&mut self, search: String) {
        self.search = search;
        self.recompute_visible();
        self.reconcile_focus(None);
    }

    fn current(&self) -> Option<RowItem> {
        let path = self
            .selected_path_in_visible_order()
            .or_else(|| {
                self.focus_path
                    .clone()
                    .filter(|path| self.path_is_visible(path))
            })
            .or_else(|| {
                self.visible
                    .first()
                    .and_then(|idx| self.items.get(*idx).map(|item| item.path.clone()))
            })?;

        self.items.iter().find(|item| item.path == path).cloned()
    }

    fn select_all_visible(&mut self) {
        for idx in &self.visible {
            if let Some(item) = self.items.get(*idx) {
                self.selected.insert(item.path.clone());
            }
        }
        self.anchor_path = self
            .selected_path_in_visible_order()
            .or_else(|| self.visible.first().and_then(|idx| self.items.get(*idx).map(|item| item.path.clone())));
        self.reconcile_focus(None);
    }

    fn clear_selection(&mut self) {
        self.selected.clear();
        self.anchor_path = None;
        self.focus_path = None;
    }

    fn toggle_row(&mut self, visible_row: usize) {
        let Some(idx) = self.visible.get(visible_row).copied() else {
            return;
        };
        let Some(item) = self.items.get(idx) else {
            return;
        };
        let path = item.path.clone();
        if !self.selected.insert(path.clone()) {
            self.selected.remove(&path);
        }
        self.focus_path = Some(path);
    }

    fn row_click(&mut self, visible_row: usize, shift: bool) {
        let Some(clicked_path) = self
            .visible
            .get(visible_row)
            .and_then(|idx| self.items.get(*idx))
            .map(|item| item.path.clone())
        else {
            return;
        };

        if shift {
            if let Some(anchor_row) = self.anchor_visible_index() {
                let start = anchor_row.min(visible_row);
                let end = anchor_row.max(visible_row);
                for row in start..=end {
                    if let Some(idx) = self.visible.get(row).copied() {
                        if let Some(item) = self.items.get(idx) {
                            self.selected.insert(item.path.clone());
                        }
                    }
                }
            } else {
                self.toggle_row(visible_row);
                self.anchor_path = Some(clicked_path.clone());
            }
        } else {
            self.toggle_row(visible_row);
            self.anchor_path = Some(clicked_path.clone());
        }

        self.focus_path = Some(clicked_path);
    }

    fn anchor_visible_index(&self) -> Option<usize> {
        let anchor = self.anchor_path.as_ref()?;
        self.visible.iter().position(|idx| {
            self.items
                .get(*idx)
                .map(|item| &item.path == anchor)
                .unwrap_or(false)
        })
    }

    fn selected_items_in_order(&self) -> Vec<RowItem> {
        self.visible
            .iter()
            .filter_map(|idx| {
                let item = self.items.get(*idx)?;
                if self.selected.contains(&item.path) {
                    Some(item.clone())
                } else {
                    None
                }
            })
            .collect()
    }

    fn move_selected_to_suggestions(&mut self) {
        let items = self.selected_items_in_order();
        if items.is_empty() {
            self.status = "请先选择文件".to_string();
            return;
        }

        let mut success = 0usize;
        let mut skipped = 0usize;
        for item in &items {
            let Some(target) = item.suggestion.as_ref() else {
                skipped += 1;
                continue;
            };
            if move_and_remember(&self.config, &item.path, &item.domain, target).is_ok() {
                success += 1;
            }
        }

        let focus = self.focus_path.clone();
        let _ = self.reload(focus);
        self.status = format!("批量移动完成：成功 {success}，无建议跳过 {skipped}");
    }

    fn move_selected_to_folder(&mut self) {
        let items = self.selected_items_in_order();
        if items.is_empty() {
            self.status = "请先选择文件".to_string();
            return;
        }

        let Some(folder) = FileDialog::new().set_title("选择归档目录").pick_folder() else {
            self.status = "已取消批量操作".to_string();
            return;
        };

        let mut success = 0usize;
        for item in &items {
            if move_and_remember(&self.config, &item.path, &item.domain, &folder).is_ok() {
                success += 1;
            }
        }

        let focus = self.focus_path.clone();
        let _ = self.reload(focus);
        self.status = format!("批量移动完成：{success} 个文件");
    }

    fn trash_selected(&mut self) {
        let items = self.selected_items_in_order();
        if items.is_empty() {
            self.status = "请先选择文件".to_string();
            return;
        }

        let mut success = 0usize;
        for item in &items {
            if trash_path(&item.path).is_ok() {
                success += 1;
            }
        }

        let focus = self.focus_path.clone();
        let _ = self.reload(focus);
        self.status = format!("批量删除完成：{success} 个文件已进废纸篓");
    }

    fn counter_text(&self) -> String {
        format!("已选 {} / {}", self.selected.len(), self.visible.len())
    }

    fn monitoring_text(&self) -> String {
        match self.monitoring_running {
            Some(true) => "监控：运行中".to_string(),
            Some(false) => "监控：已停止".to_string(),
            None => "监控：未知".to_string(),
        }
    }

    fn suggestion_text(&self) -> String {
        let Some(item) = self.current() else {
            return "建议目录：-".to_string();
        };
        if let Some(path) = item.suggestion.as_ref() {
            format!("建议目录：{}", folder_name(path))
        } else {
            "建议目录：暂无".to_string()
        }
    }

    fn selected_path_text(&self) -> String {
        let Some(item) = self.current() else {
            return "路径：-".to_string();
        };
        format!("路径：{}", item.path.display())
    }

    fn row_model(&self) -> Vec<TableRow> {
        self.visible
            .iter()
            .filter_map(|idx| self.items.get(*idx))
            .map(|item| TableRow {
                name: SharedString::from(item.name.clone()),
                domain: SharedString::from(item.domain.clone()),
                size: SharedString::from(human_size(item.size_bytes)),
                time: SharedString::from(relative_time(item.modified_ms)),
                selected: self.selected.contains(&item.path),
            })
            .collect()
    }
}

pub fn run_gui(config: &AppConfig) -> Result<()> {
    if let Err(error) = crate::launch_agent::maybe_install_launch_agent() {
        crate::pathing::log(&format!("LaunchAgent 初始化失败: {error:#}"));
    }

    let ui = SlimPanel::new().map_err(|error| anyhow!("Slint GUI 初始化失败: {error}"))?;
    let state = Rc::new(RefCell::new(PanelState::new(config.clone())?));
    sync_ui(&ui, &state.borrow());

    let weak = ui.as_weak();
    wire_callbacks(&ui, weak, state.clone());

    ui.run().map_err(|error| anyhow!("Slint GUI 启动失败: {error}"))?;
    Ok(())
}

fn wire_callbacks(ui: &SlimPanel, weak: slint::Weak<SlimPanel>, state: Rc<RefCell<PanelState>>) {
    let weak_refresh = weak.clone();
    let state_refresh = state.clone();
    ui.on_refresh(move || {
        let mut state = state_refresh.borrow_mut();
        let focus = state.focus_path.clone();
        state.status = match state.reload(focus) {
            Ok(_) => "已刷新".to_string(),
            Err(error) => format!("刷新失败：{error:#}"),
        };
        if let Some(ui) = weak_refresh.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let downloads_dir = state.borrow().config.downloads_dir.clone();
    ui.on_open_downloads(move || {
        let _ = Command::new("open").arg(&downloads_dir).status();
    });

    let memory_path = state.borrow().config.memory_path.clone();
    ui.on_open_memory(move || {
        let _ = Command::new("open").arg(&memory_path).status();
    });

    let weak_stop = weak.clone();
    let state_stop = state.clone();
    ui.on_stop_monitoring(move || {
        let mut state = state_stop.borrow_mut();
        match crate::launch_agent::stop_monitoring() {
            Ok(_) => {
                state.monitoring_running = Some(false);
                state.status = "已停止监控".to_string();
            }
            Err(error) => state.status = format!("停止监控失败：{error:#}"),
        }
        if let Some(ui) = weak_stop.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_restart = weak.clone();
    let state_restart = state.clone();
    ui.on_restart_monitoring(move || {
        let mut state = state_restart.borrow_mut();
        match crate::launch_agent::restart_monitoring() {
            Ok(_) => {
                state.monitoring_running = Some(true);
                state.status = "已重启监控".to_string();
            }
            Err(error) => state.status = format!("重启监控失败：{error:#}"),
        }
        if let Some(ui) = weak_restart.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_search = weak.clone();
    let state_search = state.clone();
    ui.on_apply_search(move || {
        let Some(ui) = weak_search.upgrade() else {
            return;
        };
        let mut state = state_search.borrow_mut();
        state.set_search(ui.get_search_text().to_string());
        sync_ui(&ui, &state);
    });

    let weak_select_all = weak.clone();
    let state_select_all = state.clone();
    ui.on_select_all_visible(move || {
        let mut state = state_select_all.borrow_mut();
        state.select_all_visible();
        if let Some(ui) = weak_select_all.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_clear = weak.clone();
    let state_clear = state.clone();
    ui.on_clear_selection(move || {
        let mut state = state_clear.borrow_mut();
        state.clear_selection();
        if let Some(ui) = weak_clear.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_sort_name = weak.clone();
    let state_sort_name = state.clone();
    ui.on_sort_name(move || {
        let mut state = state_sort_name.borrow_mut();
        state.sort_by = match state.sort_by {
            SortBy::NameAsc => SortBy::NameDesc,
            _ => SortBy::NameAsc,
        };
        state.recompute_visible();
        state.reconcile_focus(None);
        if let Some(ui) = weak_sort_name.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_sort_domain = weak.clone();
    let state_sort_domain = state.clone();
    ui.on_sort_domain(move || {
        let mut state = state_sort_domain.borrow_mut();
        state.sort_by = match state.sort_by {
            SortBy::DomainAsc => SortBy::DomainDesc,
            _ => SortBy::DomainAsc,
        };
        state.recompute_visible();
        state.reconcile_focus(None);
        if let Some(ui) = weak_sort_domain.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_sort_size = weak.clone();
    let state_sort_size = state.clone();
    ui.on_sort_size(move || {
        let mut state = state_sort_size.borrow_mut();
        state.sort_by = match state.sort_by {
            SortBy::SizeAsc => SortBy::SizeDesc,
            _ => SortBy::SizeAsc,
        };
        state.recompute_visible();
        state.reconcile_focus(None);
        if let Some(ui) = weak_sort_size.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_sort_time = weak.clone();
    let state_sort_time = state.clone();
    ui.on_sort_time(move || {
        let mut state = state_sort_time.borrow_mut();
        state.sort_by = match state.sort_by {
            SortBy::TimeDesc => SortBy::TimeAsc,
            _ => SortBy::TimeDesc,
        };
        state.recompute_visible();
        state.reconcile_focus(None);
        if let Some(ui) = weak_sort_time.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_row_click = weak.clone();
    let state_row_click = state.clone();
    ui.on_row_click(move |row, shift| {
        let Some(row) = usize::try_from(row).ok() else {
            return;
        };
        let mut state = state_row_click.borrow_mut();
        state.row_click(row, shift);
        if let Some(ui) = weak_row_click.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_move = weak.clone();
    let state_move = state.clone();
    ui.on_move_suggested(move || {
        let mut state = state_move.borrow_mut();
        state.move_selected_to_suggestions();
        if let Some(ui) = weak_move.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let weak_choose = weak.clone();
    let state_choose = state.clone();
    ui.on_choose_other(move || {
        let mut state = state_choose.borrow_mut();
        state.move_selected_to_folder();
        if let Some(ui) = weak_choose.upgrade() {
            sync_ui(&ui, &state);
        }
    });

    let state_trash = state;
    ui.on_trash_item(move || {
        let mut state = state_trash.borrow_mut();
        state.trash_selected();
        if let Some(ui) = weak.upgrade() {
            sync_ui(&ui, &state);
        }
    });
}

fn sync_ui(ui: &SlimPanel, state: &PanelState) {
    ui.set_status_text(SharedString::from(state.status.clone()));
    ui.set_monitoring_text(SharedString::from(state.monitoring_text()));
    ui.set_search_text(SharedString::from(state.search.clone()));
    ui.set_counter_text(SharedString::from(state.counter_text()));
    ui.set_suggestion_text(SharedString::from(state.suggestion_text()));
    ui.set_selected_path_text(SharedString::from(state.selected_path_text()));
    ui.set_name_header(SharedString::from(sort_header("名称", state.sort_by, SortBy::NameAsc, SortBy::NameDesc)));
    ui.set_domain_header(SharedString::from(sort_header("来源", state.sort_by, SortBy::DomainAsc, SortBy::DomainDesc)));
    ui.set_size_header(SharedString::from(sort_header("大小", state.sort_by, SortBy::SizeAsc, SortBy::SizeDesc)));
    ui.set_time_header(SharedString::from(sort_header("时间", state.sort_by, SortBy::TimeAsc, SortBy::TimeDesc)));
    ui.set_has_selection(!state.selected.is_empty());
    ui.set_rows(ModelRc::new(VecModel::from(state.row_model())));
}

fn compare_rows(left: &RowItem, right: &RowItem, sort_by: SortBy) -> Ordering {
    match sort_by {
        SortBy::NameAsc => left.name.cmp(&right.name).then(left.modified_ms.cmp(&right.modified_ms)),
        SortBy::NameDesc => right.name.cmp(&left.name).then(right.modified_ms.cmp(&left.modified_ms)),
        SortBy::DomainAsc => left.domain.cmp(&right.domain).then(left.name.cmp(&right.name)),
        SortBy::DomainDesc => right.domain.cmp(&left.domain).then(right.name.cmp(&left.name)),
        SortBy::SizeAsc => left.size_bytes.cmp(&right.size_bytes).then(left.name.cmp(&right.name)),
        SortBy::SizeDesc => right.size_bytes.cmp(&left.size_bytes).then(right.name.cmp(&left.name)),
        SortBy::TimeAsc => left.modified_ms.cmp(&right.modified_ms).then(left.name.cmp(&right.name)),
        SortBy::TimeDesc => right.modified_ms.cmp(&left.modified_ms).then(right.name.cmp(&left.name)),
    }
}

fn sort_header(label: &str, current: SortBy, asc: SortBy, desc: SortBy) -> String {
    if current == asc {
        format!("{label} ↑")
    } else if current == desc {
        format!("{label} ↓")
    } else {
        label.to_string()
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut size = bytes as f64;
    let mut unit = 0;
    while size >= 1024.0 && unit + 1 < UNITS.len() {
        size /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} {}", UNITS[unit])
    } else {
        format!("{size:.1} {}", UNITS[unit])
    }
}

fn relative_time(modified_ms: u128) -> String {
    let now_ms = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0))
        .as_millis();
    let delta = now_ms.saturating_sub(modified_ms);

    let secs = (delta / 1000) as u64;
    if secs < 60 {
        format!("{secs} 秒前")
    } else if secs < 3600 {
        format!("{} 分钟前", secs / 60)
    } else if secs < 86_400 {
        format!("{} 小时前", secs / 3600)
    } else {
        format!("{} 天前", secs / 86_400)
    }
}
