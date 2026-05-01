use crate::file_ops::{move_and_remember, should_ignore_path, trash_path};
use crate::memory::{read_memory, top_destination};
use crate::metadata::extract_source_domain;
use crate::pathing::file_name;
use crate::types::AppConfig;
use anyhow::Result;
use eframe::egui::{
    self, Color32, FontData, FontDefinitions, FontFamily, RichText, Sense, TextEdit, Vec2,
};
use rfd::FileDialog;
use std::collections::HashSet;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

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
    TimeDesc,
    TimeAsc,
    SizeDesc,
    SizeAsc,
}

pub fn run_gui(config: &AppConfig) -> Result<()> {
    let native = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Download Cleaner 管理")
            .with_inner_size([1360.0, 820.0])
            .with_min_inner_size([1120.0, 640.0]),
        ..Default::default()
    };

    let app = PanelApp::new(config.clone())?;
    eframe::run_native(
        "Download Cleaner 管理",
        native,
        Box::new(move |cc| {
            configure_fonts(&cc.egui_ctx);
            Ok(Box::new(app))
        }),
    )
    .map_err(|error| anyhow::anyhow!("GUI 启动失败: {error}"))?;
    Ok(())
}

struct PanelApp {
    config: AppConfig,
    items: Vec<RowItem>,
    status: String,
    search: String,
    sort_by: SortBy,
    selected: HashSet<PathBuf>,
    anchor_idx: Option<usize>,
}

impl PanelApp {
    fn new(config: AppConfig) -> Result<Self> {
        let mut app = Self {
            config,
            items: Vec::new(),
            status: "已加载".to_string(),
            search: String::new(),
            sort_by: SortBy::NameAsc,
            selected: HashSet::new(),
            anchor_idx: None,
        };
        app.reload()?;
        Ok(app)
    }

    fn reload(&mut self) -> Result<()> {
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
            let size_bytes = metadata.len();
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
                size_bytes,
                modified_ms,
            });
        }

        Self::apply_sort_by(self.sort_by, &mut items);
        self.items = items;
        self.selected
            .retain(|path| self.items.iter().any(|item| &item.path == path));
        Ok(())
    }

    fn apply_sort_by(sort_by: SortBy, items: &mut [RowItem]) {
        match sort_by {
            SortBy::NameAsc => items.sort_by(|a, b| a.name.cmp(&b.name)),
            SortBy::NameDesc => items.sort_by(|a, b| b.name.cmp(&a.name)),
            SortBy::DomainAsc => items.sort_by(|a, b| a.domain.cmp(&b.domain)),
            SortBy::DomainDesc => items.sort_by(|a, b| b.domain.cmp(&a.domain)),
            SortBy::TimeDesc => items.sort_by(|a, b| b.modified_ms.cmp(&a.modified_ms)),
            SortBy::TimeAsc => items.sort_by(|a, b| a.modified_ms.cmp(&b.modified_ms)),
            SortBy::SizeDesc => items.sort_by(|a, b| b.size_bytes.cmp(&a.size_bytes)),
            SortBy::SizeAsc => items.sort_by(|a, b| a.size_bytes.cmp(&b.size_bytes)),
        }
    }

    fn filtered_indices(&self) -> Vec<usize> {
        let keyword = self.search.trim().to_lowercase();
        self.items
            .iter()
            .enumerate()
            .filter(|(_, item)| {
                if keyword.is_empty() {
                    return true;
                }
                let text = format!(
                    "{} {}",
                    item.name.to_lowercase(),
                    item.domain.to_lowercase()
                );
                text.contains(&keyword)
            })
            .map(|(idx, _)| idx)
            .collect()
    }

    fn do_move_suggested(&mut self, item: &RowItem) {
        let target = if let Some(target) = item.suggestion.as_ref() {
            target.clone()
        } else if let Some(folder) = FileDialog::new().set_title("选择归档目录").pick_folder()
        {
            folder
        } else {
            self.status = format!("取消操作：{}", item.name);
            return;
        };

        match move_and_remember(&self.config, &item.path, &item.domain, &target)
            .and_then(|_| self.reload())
        {
            Ok(_) => self.status = format!("已移动：{}", item.name),
            Err(error) => self.status = format!("移动失败：{error:#}"),
        }
    }

    fn do_choose_other(&mut self, item: &RowItem) {
        let Some(folder) = FileDialog::new().set_title("选择归档目录").pick_folder() else {
            self.status = format!("取消操作：{}", item.name);
            return;
        };
        match move_and_remember(&self.config, &item.path, &item.domain, &folder)
            .and_then(|_| self.reload())
        {
            Ok(_) => self.status = format!("已移动：{}", item.name),
            Err(error) => self.status = format!("移动失败：{error:#}"),
        }
    }

    fn do_trash(&mut self, item: &RowItem) {
        match trash_path(&item.path).and_then(|_| self.reload()) {
            Ok(_) => self.status = format!("已移入废纸篓：{}", item.name),
            Err(error) => self.status = format!("删除失败：{error:#}"),
        }
    }

    fn bulk_move_suggested(&mut self) {
        let selected_paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
        if selected_paths.is_empty() {
            self.status = "请先勾选文件".to_string();
            return;
        }
        let mut success = 0usize;
        let mut skipped = 0usize;
        for path in selected_paths {
            let Some(item) = self.items.iter().find(|item| item.path == path).cloned() else {
                continue;
            };
            if let Some(target) = item.suggestion.as_ref() {
                if move_and_remember(&self.config, &item.path, &item.domain, target).is_ok() {
                    success += 1;
                }
            } else {
                skipped += 1;
            }
        }
        let _ = self.reload();
        self.status = format!("批量移动完成：成功 {success}，无建议跳过 {skipped}");
    }

    fn bulk_trash(&mut self) {
        let selected_paths: Vec<PathBuf> = self.selected.iter().cloned().collect();
        if selected_paths.is_empty() {
            self.status = "请先勾选文件".to_string();
            return;
        }
        let mut success = 0usize;
        for path in selected_paths {
            if trash_path(&path).is_ok() {
                success += 1;
            }
        }
        let _ = self.reload();
        self.status = format!("批量删除完成：{success} 个文件已进废纸篓");
    }

    fn apply_row_selection(&mut self, visible_indices: &[usize], visible_pos: usize, shift: bool) {
        let Some(&idx) = visible_indices.get(visible_pos) else {
            return;
        };
        let path = self.items[idx].path.clone();
        if shift {
            if let Some(anchor) = self.anchor_idx {
                let (start, end) = if anchor <= visible_pos {
                    (anchor, visible_pos)
                } else {
                    (visible_pos, anchor)
                };
                for pos in start..=end {
                    if let Some(&row_idx) = visible_indices.get(pos) {
                        self.selected.insert(self.items[row_idx].path.clone());
                    }
                }
            } else {
                self.selected.insert(path);
                self.anchor_idx = Some(visible_pos);
            }
            return;
        }
        if !self.selected.insert(path.clone()) {
            self.selected.remove(&path);
        }
        self.anchor_idx = Some(visible_pos);
    }

    fn toggle_sort(&mut self, asc: SortBy, desc: SortBy) {
        self.sort_by = if self.sort_by == asc { desc } else { asc };
        Self::apply_sort_by(self.sort_by, &mut self.items);
    }

    fn sort_label(&self, label: &str, asc: SortBy, desc: SortBy) -> String {
        if self.sort_by == asc {
            format!("{label} ↑")
        } else if self.sort_by == desc {
            format!("{label} ↓")
        } else {
            label.to_string()
        }
    }
}

impl eframe::App for PanelApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("top").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.heading("Download Cleaner");
                if ui.button("刷新").clicked() {
                    if let Err(error) = self.reload() {
                        self.status = format!("刷新失败：{error:#}");
                    } else {
                        self.status = "已刷新".to_string();
                    }
                }
                if ui.button("打开下载文件夹").clicked() {
                    let _ = Command::new("open")
                        .arg(&self.config.downloads_dir)
                        .status();
                }
                if ui.button("打开记忆库").clicked() {
                    let _ = Command::new("open").arg(&self.config.memory_path).status();
                }
            });

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                ui.label("搜索:");
                ui.add(TextEdit::singleline(&mut self.search).hint_text("按文件名/域名过滤"))
                    .changed();

                ui.separator();
                if ui.button("全选可见").clicked() {
                    for idx in self.filtered_indices() {
                        if let Some(item) = self.items.get(idx) {
                            self.selected.insert(item.path.clone());
                        }
                    }
                }
                if ui.button("清空勾选").clicked() {
                    self.selected.clear();
                }
                if ui.button("批量移到建议").clicked() {
                    self.bulk_move_suggested();
                }
                if ui.button("批量删除").clicked() {
                    self.bulk_trash();
                }
            });
            ui.label(RichText::new(&self.status).color(Color32::from_rgb(80, 88, 99)));
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.items.is_empty() {
                ui.label("当前没有可管理的下载文件。");
                return;
            }

            let indices = self.filtered_indices();
            if indices.is_empty() {
                ui.label("没有匹配当前搜索条件的文件。");
                return;
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| {
                    let visible_width = ui.clip_rect().width().min(ui.available_width());
                    let widths = ColumnWidths::new(visible_width);
                    egui::Grid::new("downloads_table")
                        .striped(true)
                        .spacing([10.0, 8.0])
                        .min_row_height(34.0)
                        .show(ui, |ui| {
                            ui.allocate_ui(Vec2::new(widths.check, 26.0), |_| {});
                            if ui
                                .add_sized(
                                    [widths.name, 26.0],
                                    egui::Button::new(self.sort_label(
                                        "名称",
                                        SortBy::NameAsc,
                                        SortBy::NameDesc,
                                    )),
                                )
                                .clicked()
                            {
                                self.toggle_sort(SortBy::NameAsc, SortBy::NameDesc);
                            }
                            if ui
                                .add_sized(
                                    [widths.domain, 26.0],
                                    egui::Button::new(self.sort_label(
                                        "来自",
                                        SortBy::DomainAsc,
                                        SortBy::DomainDesc,
                                    )),
                                )
                                .clicked()
                            {
                                self.toggle_sort(SortBy::DomainAsc, SortBy::DomainDesc);
                            }
                            if ui
                                .add_sized(
                                    [widths.size, 26.0],
                                    egui::Button::new(self.sort_label(
                                        "大小",
                                        SortBy::SizeAsc,
                                        SortBy::SizeDesc,
                                    )),
                                )
                                .clicked()
                            {
                                self.toggle_sort(SortBy::SizeAsc, SortBy::SizeDesc);
                            }
                            if ui
                                .add_sized(
                                    [widths.time, 26.0],
                                    egui::Button::new(self.sort_label(
                                        "时间",
                                        SortBy::TimeAsc,
                                        SortBy::TimeDesc,
                                    )),
                                )
                                .clicked()
                            {
                                self.toggle_sort(SortBy::TimeAsc, SortBy::TimeDesc);
                            }
                            ui.allocate_ui(Vec2::new(widths.move_btn, 26.0), |_| {});
                            ui.allocate_ui(Vec2::new(widths.choose_btn, 26.0), |_| {});
                            ui.allocate_ui(Vec2::new(widths.ignore_btn, 26.0), |_| {});
                            ui.allocate_ui(Vec2::new(widths.delete_btn, 26.0), |_| {});
                            ui.end_row();

                            for row_idx in 0..indices.len() {
                                let idx = indices[row_idx];
                                let item = self.items[idx].clone();
                                let row_height = row_height_for(&item, &widths);
                                let mut checked = self.selected.contains(&item.path);
                                if ui
                                    .add_sized(
                                        [widths.check, row_height],
                                        egui::Checkbox::without_text(&mut checked),
                                    )
                                    .changed()
                                {
                                    if checked {
                                        self.selected.insert(item.path.clone());
                                    } else {
                                        self.selected.remove(&item.path);
                                    }
                                    self.anchor_idx = Some(row_idx);
                                }

                                let name_response =
                                    clickable_name_cell(ui, &item.name, widths.name, row_height);
                                if name_response.clicked() {
                                    let shift = ctx.input(|i| i.modifiers.shift);
                                    self.apply_row_selection(&indices, row_idx, shift);
                                }

                                if clickable_cell(ui, &item.domain, widths.domain, row_height)
                                    .clicked()
                                {
                                    let shift = ctx.input(|i| i.modifiers.shift);
                                    self.apply_row_selection(&indices, row_idx, shift);
                                }
                                if clickable_cell(
                                    ui,
                                    &human_size(item.size_bytes),
                                    widths.size,
                                    row_height,
                                )
                                .clicked()
                                {
                                    let shift = ctx.input(|i| i.modifiers.shift);
                                    self.apply_row_selection(&indices, row_idx, shift);
                                }
                                if clickable_cell(
                                    ui,
                                    &relative_time(item.modified_ms),
                                    widths.time,
                                    row_height,
                                )
                                .clicked()
                                {
                                    let shift = ctx.input(|i| i.modifiers.shift);
                                    self.apply_row_selection(&indices, row_idx, shift);
                                }

                                let move_label = if let Some(name) = suggestion_name(&item) {
                                    format!("移到{name}")
                                } else {
                                    "移到建议".to_string()
                                };
                                if ui
                                    .add_sized(
                                        [widths.move_btn, row_height.min(34.0)],
                                        egui::Button::new(move_label),
                                    )
                                    .clicked()
                                {
                                    self.do_move_suggested(&item);
                                }
                                if ui
                                    .add_sized(
                                        [widths.choose_btn, row_height.min(34.0)],
                                        egui::Button::new("选择目录"),
                                    )
                                    .clicked()
                                {
                                    self.do_choose_other(&item);
                                }
                                if ui
                                    .add_sized(
                                        [widths.ignore_btn, row_height.min(34.0)],
                                        egui::Button::new("放着不管"),
                                    )
                                    .clicked()
                                {
                                    self.status = format!("保持原位：{}", item.name);
                                }
                                if ui
                                    .add_sized(
                                        [widths.delete_btn, row_height.min(34.0)],
                                        egui::Button::new("删除"),
                                    )
                                    .clicked()
                                {
                                    self.do_trash(&item);
                                }
                                ui.end_row();
                            }
                        });
                });
        });
    }
}

struct ColumnWidths {
    check: f32,
    name: f32,
    domain: f32,
    size: f32,
    time: f32,
    move_btn: f32,
    choose_btn: f32,
    ignore_btn: f32,
    delete_btn: f32,
}

impl ColumnWidths {
    fn new(total_width: f32) -> Self {
        let check = 28.0;
        let move_btn = 118.0;
        let choose_btn = 82.0;
        let ignore_btn = 82.0;
        let delete_btn = 54.0;
        let domain = (total_width * 0.13).clamp(136.0, 200.0);
        let size = 82.0;
        let time = 92.0;
        let grid_gaps = 8.0 * 10.0;
        let side_padding = 48.0;
        let used = check
            + domain
            + size
            + time
            + move_btn
            + choose_btn
            + ignore_btn
            + delete_btn
            + grid_gaps
            + side_padding;
        let name = (total_width - used).max(260.0);
        Self {
            check,
            name,
            domain,
            size,
            time,
            move_btn,
            choose_btn,
            ignore_btn,
            delete_btn,
        }
    }
}

fn row_height_for(item: &RowItem, widths: &ColumnWidths) -> f32 {
    let name_lines = wrapped_line_count(&item.name, widths.name, 11.0);
    let domain_lines = wrapped_line_count(&item.domain, widths.domain, 10.0);
    let lines = name_lines.max(domain_lines).min(5);
    (lines as f32 * 24.0).max(34.0)
}

fn wrapped_line_count(text: &str, width: f32, avg_char_width: f32) -> usize {
    let chars_per_line = (width / avg_char_width).floor().max(8.0) as usize;
    let mut lines = 1usize;
    let mut current = 0usize;
    for ch in text.chars() {
        current += if ch.is_ascii() { 1 } else { 2 };
        if ch == '\n' || current >= chars_per_line {
            lines += 1;
            current = 0;
        }
    }
    lines
}

fn suggestion_name(item: &RowItem) -> Option<String> {
    item.suggestion
        .as_ref()
        .and_then(|path| path.file_name())
        .and_then(|name| name.to_str())
        .map(compact_name)
}

fn compact_name(name: &str) -> String {
    let limit = 8usize;
    let mut chars = name.chars();
    let head: String = chars.by_ref().take(limit).collect();
    if chars.next().is_some() {
        format!("{head}...")
    } else {
        head
    }
}

fn clickable_name_cell(ui: &mut egui::Ui, text: &str, width: f32, height: f32) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(width, height),
        egui::Layout::left_to_right(egui::Align::Center),
        |ui| {
            ui.add(
                egui::Label::new(RichText::new(text).strong())
                    .sense(Sense::click())
                    .wrap(),
            )
        },
    )
    .inner
}

fn clickable_cell(ui: &mut egui::Ui, text: &str, width: f32, height: f32) -> egui::Response {
    ui.allocate_ui_with_layout(
        Vec2::new(width, height),
        egui::Layout::centered_and_justified(egui::Direction::LeftToRight),
        |ui| ui.add(egui::Label::new(text).sense(Sense::click()).wrap()),
    )
    .inner
}

fn configure_fonts(ctx: &egui::Context) {
    let mut fonts = FontDefinitions::default();
    let candidates = [
        "/System/Library/Fonts/PingFang.ttc",
        "/System/Library/Fonts/Hiragino Sans GB.ttc",
        "/Library/Fonts/Arial Unicode.ttf",
    ];
    for path in candidates {
        if let Ok(bytes) = fs::read(path) {
            fonts
                .font_data
                .insert("cjk".to_string(), FontData::from_owned(bytes).into());
            if let Some(list) = fonts.families.get_mut(&FontFamily::Proportional) {
                list.insert(0, "cjk".to_string());
            }
            if let Some(list) = fonts.families.get_mut(&FontFamily::Monospace) {
                list.insert(0, "cjk".to_string());
            }
            break;
        }
    }
    ctx.set_fonts(fonts);
}

fn human_size(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let kb = bytes as f64 / 1024.0;
    if kb < 1024.0 {
        return format!("{kb:.1} KB");
    }
    let mb = kb / 1024.0;
    if mb < 1024.0 {
        return format!("{mb:.1} MB");
    }
    let gb = mb / 1024.0;
    format!("{gb:.2} GB")
}

fn relative_time(modified_ms: u128) -> String {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis();
    let delta = now.saturating_sub(modified_ms);
    let minute = 60_000u128;
    let hour = 60 * minute;
    let day = 24 * hour;
    if delta < minute {
        "刚刚".to_string()
    } else if delta < hour {
        format!("{} 分钟前", delta / minute)
    } else if delta < day {
        format!("{} 小时前", delta / hour)
    } else {
        format!("{} 天前", delta / day)
    }
}
