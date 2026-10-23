//! `tui/` — Ratatui rendering and terminal management for `cuifiler` (SPEC-0300-0001).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, Gauge, List, ListItem, Paragraph, Row, Table};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{
    normalize_tree_key, tree_node_key, AppState, ClipMode, ConflictPromptData, FocusPane,
    OverlayMode, TransferProgressState,
};
use crate::fs::query::EntryType;
use crate::remote::profile::ConnectionProfile;

/// Pure view rendering function (`coding-standards` §3: no side effects).
pub fn draw(frame: &mut Frame, app: &AppState) {
    let area = frame.area();

    // Fill background if theme defines a background color
    if app.theme.bg != Color::Reset {
        frame.render_widget(
            Block::default().style(Style::default().bg(app.theme.bg)),
            area,
        );
    }

    // If fullscreen Viewer overlay is active, render it across the entire screen
    if matches!(app.overlay, OverlayMode::Viewer { .. }) {
        draw_viewer(frame, area, app);
        return;
    }

    let has_search = !app.search_query.is_empty();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(if has_search { 2 } else { 1 }),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);

    draw_menu_bar(frame, chunks[0], app);
    draw_path_and_search_bar(frame, chunks[1], app);
    draw_main_body(frame, chunks[2], app);
    draw_status_bar(frame, chunks[3], app);

    // Draw modal overlays on top
    match &app.overlay {
        OverlayMode::None | OverlayMode::Viewer { .. } => {}
        OverlayMode::Input { title, buffer, .. } => {
            draw_input_modal(frame, area, title, buffer, &app.theme);
        }
        OverlayMode::PasswordPrompt {
            title,
            buffer,
            save_password,
            ..
        } => {
            draw_password_prompt_modal(frame, area, title, buffer, *save_password, &app.theme);
        }
        OverlayMode::HostKeyConfirm {
            endpoint,
            fingerprint,
        } => {
            draw_host_key_confirm_modal(frame, area, endpoint, fingerprint);
        }
        OverlayMode::ConnectionManager { cursor } => {
            draw_connection_manager_modal(frame, area, app, *cursor);
        }
        OverlayMode::ConnectionEdit {
            is_new,
            profile,
            cursor,
            password_buffer,
        } => {
            draw_connection_edit_modal(
                frame,
                area,
                *is_new,
                profile,
                *cursor,
                password_buffer,
                &app.theme,
            );
        }
        OverlayMode::SortMenu { cursor } => {
            let items = vec![
                "Name (Asc)".to_string(),
                "Name (Desc)".to_string(),
                "Size (Asc)".to_string(),
                "Size (Desc)".to_string(),
                "Type (Asc)".to_string(),
                "Type (Desc)".to_string(),
                "Modified (Asc)".to_string(),
                "Modified (Desc)".to_string(),
            ];
            draw_popup_list(
                frame,
                area,
                "Sort (j/k:Move Enter:Apply Esc:Close)",
                &items,
                *cursor,
                &app.theme,
            );
        }
        OverlayMode::ListPopup {
            title,
            items,
            cursor,
            ..
        } => {
            let display_items = if items.is_empty() {
                vec!["(No items)".to_string()]
            } else {
                items.clone()
            };
            draw_popup_list(frame, area, title, &display_items, *cursor, &app.theme);
        }
        OverlayMode::ActionMenu { cursor } => {
            let mut items = vec![
                "Open (Enter)".to_string(),
                "Open with Default App (o / Shift+Enter)".to_string(),
                "Copy / Yank (y)".to_string(),
                "Cut (x)".to_string(),
                "Paste (p)".to_string(),
                "Rename (r)".to_string(),
                "Move to Trash (d / Delete)".to_string(),
                "New File (a)".to_string(),
                "New Folder (A)".to_string(),
                "Create ZIP (z)".to_string(),
                "Extract ZIP (Z)".to_string(),
            ];
            for ext in &app.external_apps {
                let tag = if ext.is_gui { "[GUI]" } else { "[CUI]" };
                items.push(format!("{tag} {} ({})", ext.name, ext.key_label));
            }
            draw_popup_list(
                frame,
                area,
                "Action Menu (j/k/Enter/Esc)",
                &items,
                *cursor,
                &app.theme,
            );
        }
        OverlayMode::MenuBar { cat_idx, item_idx } => {
            draw_menubar_dropdown(frame, area, app, *cat_idx, *item_idx);
        }
    }

    // If an active background transfer is running, draw progress modal on top of everything
    if let Some(transfer) = &app.active_transfer {
        draw_transfer_progress_modal(frame, area, &transfer.current_state);
        if let Some(prompt) = &transfer.conflict_prompt {
            draw_conflict_confirm_modal(frame, area, prompt);
        }
    }
}

fn draw_menu_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let active_cat = match app.overlay {
        OverlayMode::MenuBar { cat_idx, .. } => Some(cat_idx),
        _ => None,
    };
    let labels = [" File(F) ", " Edit(E) ", " View(V) ", " Go(G) "];
    let mut spans = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        let style = if active_cat == Some(i) {
            Style::default()
                .fg(app.theme.menu_bar_active_fg)
                .bg(app.theme.menu_bar_active_bg)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(app.theme.status_bar_fg)
                .bg(app.theme.status_bar_bg)
        };
        spans.push(Span::styled(*label, style));
        spans.push(Span::styled(
            " ",
            Style::default().bg(app.theme.status_bar_bg),
        ));
    }
    spans.push(Span::styled(
        "  tuif",
        Style::default()
            .fg(app.theme.border_inactive)
            .bg(app.theme.status_bar_bg),
    ));
    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(app.theme.status_bar_bg));
    frame.render_widget(bar, area);
}

fn draw_path_and_search_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let mut lines = vec![Line::from(vec![
        Span::styled(
            " Path: ",
            Style::default()
                .fg(app.theme.tree_current)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            app.active_display_path(),
            Style::default().fg(app.theme.file),
        ),
    ])];
    if !app.search_query.is_empty() {
        let mode_tag = if app.search_regex {
            "[Regex]"
        } else {
            "[Substr]"
        };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" Search {mode_tag}: "),
                Style::default()
                    .fg(app.theme.selected)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                app.search_query.clone(),
                Style::default().fg(app.theme.file),
            ),
        ]));
    }
    let p = Paragraph::new(lines).style(Style::default().bg(app.theme.bg));
    frame.render_widget(p, area);
}

fn draw_main_body(frame: &mut Frame, area: Rect, app: &AppState) {
    if app.ui_state.ui_show_tree {
        let tw = app.layout.tree_width_percent.clamp(10, 50);
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(tw), Constraint::Percentage(100 - tw)])
            .split(area);
        draw_tree_pane(frame, h_chunks[0], app);
        draw_file_list_pane(frame, h_chunks[1], app);
    } else {
        draw_file_list_pane(frame, area, app);
    }
}

fn draw_tree_pane(frame: &mut Frame, area: Rect, app: &AppState) {
    let focused = app.focus_pane == FocusPane::Tree;
    let border_style = if focused {
        Style::default().fg(app.theme.border)
    } else {
        Style::default().fg(app.theme.border_inactive)
    };
    let inner_height = area.height.saturating_sub(2) as usize;
    let page_size = inner_height.max(1);
    let offset = app.general.scroll_offset.min(page_size / 2);
    let start_offset = if app.tree_cursor < page_size {
        0
    } else {
        (app.tree_cursor + offset + 1).saturating_sub(page_size)
    };
    let current_dir_key = normalize_tree_key(&app.current_dir);

    let items: Vec<ListItem> = app
        .tree_nodes
        .iter()
        .enumerate()
        .skip(start_offset)
        .take(page_size)
        .map(|(i, node)| {
            let is_current_dir =
                if let (Some(session), Some(rdir)) = (&app.remote_session, &app.remote_dir) {
                    tree_node_key(node) == format!("remote:{}:{rdir}", session.profile.id)
                } else {
                    node.remote.is_none() && normalize_tree_key(&node.path) == current_dir_key
                };
            let indent = "  ".repeat(node.depth);
            let caret = if node.expanded { "▾ " } else { "▸ " };
            let text = format!("{indent}{caret}{}", node.name);
            let style = if i == app.tree_cursor && focused {
                Style::default()
                    .fg(app.theme.cursor_fg)
                    .bg(app.theme.cursor_bg)
                    .add_modifier(Modifier::BOLD)
            } else if is_current_dir {
                Style::default()
                    .fg(app.theme.tree_current)
                    .add_modifier(Modifier::BOLD)
            } else if i == app.tree_cursor {
                Style::default().fg(app.theme.tree_cursor)
            } else {
                Style::default().fg(app.theme.file)
            };
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .style(Style::default().bg(app.theme.bg))
            .title(" Drives / Tree (Tab | l:Expand h:Collapse Enter:Open) "),
    );
    frame.render_widget(list, area);
}

fn draw_file_list_pane(frame: &mut Frame, area: Rect, app: &AppState) {
    let focused = app.focus_pane == FocusPane::FileList;
    let border_style = if focused {
        Style::default().fg(app.theme.border)
    } else {
        Style::default().fg(app.theme.border_inactive)
    };

    let rows_per_col = area.height.saturating_sub(2).max(1) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let mut min_col_width: usize = app.layout.min_col_width.clamp(10, 80);
    if app.ui_state.ui_show_size {
        min_col_width += 10;
    }
    if app.ui_state.ui_show_time {
        min_col_width += 20;
    }
    let max_fit_cols = (inner_width / min_col_width).max(1);
    let total_cols = app.visible_indices.len().div_ceil(rows_per_col).max(1);
    let visible_cols = max_fit_cols.min(total_cols).max(1);

    let focused_col = app.focused_vis_idx / rows_per_col;
    let start_col = (focused_col / visible_cols) * visible_cols;
    let end_col = (start_col + visible_cols).min(total_cols);
    let shown_cols = (end_col - start_col).max(1);

    let left_arrow = if start_col > 0 { "◀ " } else { "" };
    let right_arrow = if end_col < total_cols { " ▶" } else { "" };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .style(Style::default().bg(app.theme.bg))
        .title(format!(
            " {left_arrow}Files ({} items) [sort: {} {}]{right_arrow} ",
            app.visible_indices.len(),
            app.ui_state.sort_key,
            app.ui_state.sort_order
        ));

    if app.visible_indices.is_empty() {
        let empty = Paragraph::new("  No items")
            .style(
                Style::default()
                    .fg(app.theme.border_inactive)
                    .bg(app.theme.bg),
            )
            .block(block);
        frame.render_widget(empty, area);
        return;
    }

    let inner = block.inner(area);
    frame.render_widget(block, area);

    let constraints: Vec<Constraint> = (0..shown_cols)
        .map(|_| Constraint::Ratio(1, shown_cols as u32))
        .collect();
    let col_rects = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(inner);

    for (slot_idx, col_idx) in (start_col..end_col).enumerate() {
        let col_rect = col_rects[slot_idx];
        let has_right_neighbor = slot_idx + 1 < shown_cols;
        let table_rect = if has_right_neighbor && col_rect.width > 2 {
            Rect {
                width: col_rect.width - 1,
                ..col_rect
            }
        } else {
            col_rect
        };

        let mut reserved_cols: usize = 2; // "* " or "  "
        if app.ui_state.ui_show_size {
            reserved_cols += 11;
        }
        if app.ui_state.ui_show_time {
            reserved_cols += 21;
        }
        let max_name_width = (table_rect.width as usize)
            .saturating_sub(reserved_cols)
            .max(8);

        let col_start_vis = col_idx * rows_per_col;
        let mut rows = Vec::new();
        for (vis_idx, &real_idx) in app
            .visible_indices
            .iter()
            .enumerate()
            .skip(col_start_vis)
            .take(rows_per_col)
        {
            let entry = &app.entries[real_idx];
            let is_focused = vis_idx == app.focused_vis_idx;
            let is_selected = app.selected_paths.contains(&entry.path);

            let mark = if is_selected { "*" } else { " " };
            let name_col = format!(
                "{mark} {}",
                truncate_display_width(&entry.name, max_name_width)
            );
            let size_col = if entry.entry_type == EntryType::Dir {
                "<DIR>".to_string()
            } else {
                format_size(entry.size)
            };
            let time_col = format_timestamp(&entry.modified, &app.layout.time_format);

            let mut cells = vec![name_col];
            if app.ui_state.ui_show_size {
                cells.push(size_col);
            }
            if app.ui_state.ui_show_time {
                cells.push(time_col);
            }

            let is_exe = entry.name.ends_with(".exe")
                || entry.name.ends_with(".bat")
                || entry.name.ends_with(".cmd")
                || entry.name.ends_with(".ps1");

            let style = if is_focused && focused {
                Style::default()
                    .fg(app.theme.cursor_fg)
                    .bg(app.theme.cursor_bg)
                    .add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default()
                    .fg(app.theme.selected)
                    .add_modifier(Modifier::BOLD)
            } else if entry.entry_type == EntryType::Dir {
                Style::default()
                    .fg(app.theme.folder)
                    .add_modifier(Modifier::BOLD)
            } else if is_exe {
                Style::default().fg(app.theme.executable)
            } else {
                Style::default().fg(app.theme.file)
            };

            rows.push(Row::new(cells).style(style));
        }

        let mut widths = vec![Constraint::Min(10)];
        if app.ui_state.ui_show_size {
            widths.push(Constraint::Length(10));
        }
        if app.ui_state.ui_show_time {
            widths.push(Constraint::Length(20));
        }

        let table = Table::new(rows, widths).style(Style::default().bg(app.theme.bg));
        frame.render_widget(table, table_rect);

        if has_right_neighbor && col_rect.width > 2 {
            let sep_x = col_rect.x + col_rect.width - 1;
            let sep_lines: Vec<Line> = (0..col_rect.height)
                .map(|_| {
                    Line::from(Span::styled(
                        "│",
                        Style::default()
                            .fg(app.theme.border_inactive)
                            .bg(app.theme.bg),
                    ))
                })
                .collect();
            frame.render_widget(
                Paragraph::new(sep_lines).style(Style::default().bg(app.theme.bg)),
                Rect {
                    x: sep_x,
                    y: col_rect.y,
                    width: 1,
                    height: col_rect.height,
                },
            );
        }
    }
}

fn draw_status_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let clip_info = match &app.clipboard {
        Some(c) => {
            let tag = match c.mode {
                ClipMode::Copy => "Yank",
                ClipMode::Cut => "Cut",
            };
            format!(" [{tag}:{}]", c.items.len())
        }
        None => String::new(),
    };
    let sel_info = if app.selected_paths.is_empty() {
        String::new()
    } else {
        format!(" [Selected:{}]", app.selected_paths.len())
    };
    let text = format!(
        " Items: {}{}{} | {}",
        app.visible_indices.len(),
        sel_info,
        clip_info,
        app.status_message
    );
    let bar = Paragraph::new(text).style(
        Style::default()
            .fg(app.theme.status_bar_fg)
            .bg(app.theme.status_bar_bg),
    );
    frame.render_widget(bar, area);
}

fn draw_viewer(frame: &mut Frame, area: Rect, app: &AppState) {
    let OverlayMode::Viewer {
        ref title,
        is_markdown,
        preview_mode,
        ref raw_lines,
        ref preview_lines,
        scroll_y,
        scroll_x,
        ref search_query,
        ref search_input,
        ref matching_lines,
        match_cursor,
        ..
    } = app.overlay
    else {
        return;
    };
    let (lines, active_preview) = match (preview_mode, preview_lines.as_ref()) {
        (true, Some(prev)) => (prev.as_slice(), true),
        _ => (raw_lines.as_slice(), false),
    };
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(3), Constraint::Length(1)])
        .split(area);

    let view_height = chunks[0].height.saturating_sub(2) as usize;
    let visible_lines: Vec<Line> = lines
        .iter()
        .enumerate()
        .skip(scroll_y)
        .take(view_height)
        .map(|(idx, line)| {
            let is_match = matching_lines.contains(&idx);
            let is_current = match_cursor.and_then(|c| matching_lines.get(c)).copied() == Some(idx);
            let mut spans = if active_preview {
                if is_current {
                    vec![Span::styled(
                        "▶ ",
                        Style::default()
                            .fg(app.theme.selected)
                            .add_modifier(Modifier::BOLD),
                    )]
                } else if is_match {
                    vec![Span::styled(
                        "● ",
                        Style::default()
                            .fg(app.theme.tree_cursor)
                            .add_modifier(Modifier::BOLD),
                    )]
                } else {
                    vec![Span::raw("  ")]
                }
            } else {
                let gutter_style = if is_current {
                    Style::default()
                        .fg(app.theme.selected)
                        .add_modifier(Modifier::BOLD)
                } else if is_match {
                    Style::default()
                        .fg(app.theme.tree_cursor)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(app.theme.border_inactive)
                };
                let marker = if is_current {
                    "▶"
                } else if is_match {
                    "●"
                } else {
                    "│"
                };
                vec![Span::styled(
                    format!("{:>5} {marker} ", idx + 1),
                    gutter_style,
                )]
            };
            spans.extend(crate::viewer::highlight::parse_ansi_line_with_scroll(
                line, scroll_x,
            ));
            Line::from(spans)
        })
        .collect();

    let mode_tag = if is_markdown {
        if active_preview {
            " [Preview: leaf]"
        } else {
            " [Raw: bat]"
        }
    } else {
        ""
    };
    let search_tag = if let Some(ref q) = search_query {
        if matching_lines.is_empty() {
            format!(" [Pattern not found: \"{q}\"]")
        } else {
            let cur_pos = match_cursor.map(|c| c + 1).unwrap_or(1);
            format!(" [Match {cur_pos}/{} for \"{q}\"]", matching_lines.len())
        }
    } else {
        String::new()
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(app.theme.border))
        .style(Style::default().bg(app.theme.bg))
        .title(format!(
            " Viewer{mode_tag}: {title} (Line {}/{}){search_tag} ",
            scroll_y + 1,
            lines.len().max(1)
        ));
    frame.render_widget(
        Paragraph::new(visible_lines)
            .style(Style::default().bg(app.theme.bg))
            .block(block),
        chunks[0],
    );

    if let Some(ref input) = search_input {
        let prompt_line = Line::from(vec![
            Span::styled(
                " /",
                Style::default()
                    .fg(app.theme.menu_bar_active_fg)
                    .bg(app.theme.menu_bar_active_bg)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("{input}█"),
                Style::default()
                    .fg(app.theme.cursor_fg)
                    .bg(app.theme.cursor_bg),
            ),
            Span::styled(
                "  (Enter:Search, Esc:Cancel)",
                Style::default()
                    .fg(app.theme.status_bar_fg)
                    .bg(app.theme.status_bar_bg),
            ),
        ]);
        let search_footer =
            Paragraph::new(prompt_line).style(Style::default().bg(app.theme.status_bar_bg));
        frame.render_widget(search_footer, chunks[1]);
    } else {
        let footer_text = if is_markdown {
            " j/k:Scroll  Ctrl+d/u:Page  g/G:Top/Bottom  /:Search  n/N:Next/Prev  p/Tab:Preview/Raw  q/Esc:Close"
        } else {
            " j/k:Scroll  Ctrl+d/u:Page  g/G:Top/Bottom  h/l:Horiz  /:Search  n/N:Next/Prev  q/Esc:Close"
        };
        let footer = Paragraph::new(footer_text).style(
            Style::default()
                .fg(app.theme.cursor_fg)
                .bg(app.theme.cursor_bg),
        );
        frame.render_widget(footer, chunks[1]);
    }
}

fn draw_input_modal(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    buffer: &str,
    theme: &crate::theme::Theme,
) {
    let popup = centered_rect(70, 5, area);
    frame.render_widget(Clear, popup);
    let popup_bg = if theme.bg != Color::Reset {
        theme.bg
    } else {
        Color::Black
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(popup_bg))
        .title(format!(" {title} "));
    let content = Paragraph::new(format!("{buffer}█"))
        .style(Style::default().fg(theme.file).bg(popup_bg))
        .block(block);
    frame.render_widget(content, popup);
}

pub fn popup_list_geometry(area: Rect, item_count: usize, cursor: usize) -> (Rect, usize, usize) {
    let height = (item_count as u16 + 4).clamp(6, area.height.saturating_sub(4));
    let popup = centered_rect(65, height, area);
    let page_size = popup.height.saturating_sub(2).max(1) as usize;
    let start_offset = (cursor / page_size) * page_size;
    (popup, start_offset, page_size)
}

pub fn menubar_dropdown_rect(area: Rect, cat_idx: usize, item_count: usize) -> Rect {
    let x_offset = (cat_idx as u16 * 10).min(area.width.saturating_sub(42));
    let height = (item_count as u16 + 2).min(area.height.saturating_sub(2));
    Rect {
        x: area.x + x_offset,
        y: area.y + 1,
        width: 42.min(area.width),
        height,
    }
}

pub fn main_panes_geometry(
    area: Rect,
    has_search: bool,
    show_tree: bool,
    tree_width_percent: u16,
) -> (Rect, Option<Rect>, Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(if has_search { 2 } else { 1 }),
            Constraint::Min(3),
            Constraint::Length(1),
        ])
        .split(area);
    let path_bar_rect = chunks[1];
    let main_rect = chunks[2];
    if show_tree {
        let tw = tree_width_percent.clamp(10, 50);
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(tw), Constraint::Percentage(100 - tw)])
            .split(main_rect);
        (path_bar_rect, Some(h_chunks[0]), h_chunks[1])
    } else {
        (path_bar_rect, None, main_rect)
    }
}

pub fn hit_test_file_list(
    file_list_rect: Rect,
    visible_len: usize,
    focused_vis_idx: usize,
    show_size: bool,
    show_time: bool,
    base_min_col_width: usize,
    mouse: (u16, u16),
) -> Option<usize> {
    let (mouse_x, mouse_y) = mouse;
    if visible_len == 0 || file_list_rect.width <= 2 || file_list_rect.height <= 2 {
        return None;
    }
    let inner = Rect {
        x: file_list_rect.x + 1,
        y: file_list_rect.y + 1,
        width: file_list_rect.width - 2,
        height: file_list_rect.height - 2,
    };
    if mouse_x < inner.x
        || mouse_x >= inner.x + inner.width
        || mouse_y < inner.y
        || mouse_y >= inner.y + inner.height
    {
        return None;
    }

    let rows_per_col = file_list_rect.height.saturating_sub(2).max(1) as usize;
    let inner_width = file_list_rect.width.saturating_sub(2) as usize;
    let mut min_col_width: usize = base_min_col_width.clamp(10, 80);
    if show_size {
        min_col_width += 10;
    }
    if show_time {
        min_col_width += 20;
    }
    let max_fit_cols = (inner_width / min_col_width).max(1);
    let total_cols = visible_len.div_ceil(rows_per_col).max(1);
    let visible_cols = max_fit_cols.min(total_cols).max(1);

    let focused_col = focused_vis_idx / rows_per_col;
    let start_col = (focused_col / visible_cols) * visible_cols;
    let end_col = (start_col + visible_cols).min(total_cols);
    let shown_cols = (end_col - start_col).max(1);

    let constraints: Vec<Constraint> = (0..shown_cols)
        .map(|_| Constraint::Ratio(1, shown_cols as u32))
        .collect();
    let col_rects = Layout::default()
        .direction(Direction::Horizontal)
        .constraints(constraints)
        .split(inner);

    let slot_idx = col_rects
        .iter()
        .position(|r| mouse_x >= r.x && mouse_x < r.x + r.width)?;
    let row_in_col = (mouse_y - inner.y) as usize;
    let vis_idx = (start_col + slot_idx) * rows_per_col + row_in_col;
    if vis_idx < visible_len {
        Some(vis_idx)
    } else {
        None
    }
}

/// Formats a 20-digit Unix timestamp string into the user's configured format string (e.g. "%Y/%m/%d %H:%M").
pub fn format_timestamp(raw: &str, format_str: &str) -> String {
    let Ok(secs) = raw.trim().parse::<i64>() else {
        return raw.to_string();
    };
    if secs <= 0 {
        return String::new();
    }
    let Ok(dt) = time::OffsetDateTime::from_unix_timestamp(secs) else {
        return raw.to_string();
    };

    let year = dt.year();
    let month = dt.month() as u8;
    let day = dt.day();
    let hour = dt.hour();
    let minute = dt.minute();
    let second = dt.second();

    let mut result = format_str.to_string();
    result = result.replace("%Y", &format!("{year:04}"));
    result = result.replace("%m", &format!("{month:02}"));
    result = result.replace("%d", &format!("{day:02}"));
    result = result.replace("%H", &format!("{hour:02}"));
    result = result.replace("%M", &format!("{minute:02}"));
    result = result.replace("%S", &format!("{second:02}"));
    result
}

fn draw_popup_list(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    items: &[String],
    cursor: usize,
    theme: &crate::theme::Theme,
) {
    let (popup, start_offset, page_size) = popup_list_geometry(area, items.len(), cursor);
    frame.render_widget(Clear, popup);

    let popup_bg = if theme.bg != Color::Reset {
        theme.bg
    } else {
        Color::Black
    };

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .skip(start_offset)
        .take(page_size)
        .map(|(i, item)| {
            let style = if i == cursor {
                Style::default()
                    .fg(theme.cursor_fg)
                    .bg(theme.cursor_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.file).bg(popup_bg)
            };
            ListItem::new(format!("  {item}")).style(style)
        })
        .collect();

    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(theme.border))
            .style(Style::default().bg(popup_bg))
            .title(format!(" {title} ")),
    );
    frame.render_widget(list, popup);
}

fn draw_menubar_dropdown(
    frame: &mut Frame,
    area: Rect,
    app: &AppState,
    cat_idx: usize,
    item_idx: usize,
) {
    let items: Vec<String> = match cat_idx {
        0 => {
            let mut v = vec![
                "Open (Enter)".to_string(),
                "Open with Default App (o / Shift+Enter)".to_string(),
                "New File... (a / Ctrl+N)".to_string(),
                "New Folder... (A / Ctrl+Shift+N)".to_string(),
                "Rename (r / F2)".to_string(),
                "Delete (d / Delete)".to_string(),
                "Create ZIP (z / Ctrl+Alt+Z)".to_string(),
                "Extract ZIP (Z / Ctrl+Alt+X)".to_string(),
            ];
            for ext in &app.external_apps {
                v.push(format!("{} ({})", ext.name, ext.key_label));
            }
            v.push("Exit (q)".to_string());
            v.push("Exit & Cd (Ctrl+Q)".to_string());
            v
        }
        1 => vec![
            "Cut (x / Ctrl+X)".to_string(),
            "Copy (y / Ctrl+C)".to_string(),
            "Paste (p / Ctrl+V)".to_string(),
            "Select All (V / Ctrl+A)".to_string(),
            "Clear Selection (Esc)".to_string(),
            "Search (/)".to_string(),
        ],
        2 => vec![
            format!(
                "Tree [{}] (t / Ctrl+Shift+B)",
                if app.ui_state.ui_show_tree {
                    "✓"
                } else {
                    " "
                }
            ),
            format!(
                "Hidden Files [{}] (. / Ctrl+H)",
                if app.ui_state.ui_show_hidden {
                    "✓"
                } else {
                    " "
                }
            ),
            format!(
                "Size Column [{}] (Ctrl+Shift+S)",
                if app.ui_state.ui_show_size {
                    "✓"
                } else {
                    " "
                }
            ),
            format!(
                "Modified Column [{}] (Ctrl+Shift+T)",
                if app.ui_state.ui_show_time {
                    "✓"
                } else {
                    " "
                }
            ),
            "Sort... (s)".to_string(),
        ],
        _ => vec![
            "Parent Folder (Backspace / -)".to_string(),
            "Focus Path Bar (Ctrl+L)".to_string(),
            "History (Ctrl+Y)".to_string(),
            "Jump List (Ctrl+J)".to_string(),
            "Add to Jump List (m / Ctrl+Shift+J)".to_string(),
            "Fuzzy Find File... (Ctrl+P / Ctrl+F)".to_string(),
            "Fuzzy Live Grep... (Ctrl+G / F)".to_string(),
            "Fuzzy Jump Folder... (Alt+C / f)".to_string(),
            "SFTP Connections... (Ctrl+S)".to_string(),
        ],
    };

    let rect = menubar_dropdown_rect(area, cat_idx, items.len());
    frame.render_widget(Clear, rect);

    let popup_bg = if app.theme.bg != Color::Reset {
        app.theme.bg
    } else {
        Color::Black
    };

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == item_idx {
                Style::default()
                    .fg(app.theme.menu_bar_active_fg)
                    .bg(app.theme.menu_bar_active_bg)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(app.theme.file).bg(popup_bg)
            };
            ListItem::new(format!(" {s}")).style(style)
        })
        .collect();
    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(app.theme.border))
            .style(Style::default().bg(popup_bg)),
    );
    frame.render_widget(list, rect);
}

fn centered_rect(percent_x: u16, height: u16, r: Rect) -> Rect {
    let width = (r.width * percent_x / 100).max(20).min(r.width);
    let h = height.min(r.height);
    Rect {
        x: r.x + (r.width.saturating_sub(width)) / 2,
        y: r.y + (r.height.saturating_sub(h)) / 2,
        width,
        height: h,
    }
}

/// Truncates a string by terminal display column width using `unicode-width` (`coding-standards` §3).
pub fn truncate_display_width(s: &str, max_cols: usize) -> String {
    if UnicodeWidthStr::width(s) <= max_cols {
        return s.to_string();
    }
    let mut out = String::new();
    let mut width = 0;
    for ch in s.chars() {
        let cw = unicode_width::UnicodeWidthChar::width(ch).unwrap_or(0);
        if width + cw + 1 > max_cols {
            out.push('…');
            break;
        }
        out.push(ch);
        width += cw;
    }
    out
}

fn format_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = 1024 * KB;
    const GB: u64 = 1024 * MB;
    if bytes >= GB {
        format!("{:.1} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.1} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.1} KB", bytes as f64 / KB as f64)
    } else {
        format!("{bytes} B")
    }
}

fn draw_password_prompt_modal(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    buffer: &str,
    save_password: bool,
    theme: &crate::theme::Theme,
) {
    let width = 60.min(area.width.saturating_sub(4)).max(30);
    let height = 6;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup);

    let masked: String = "*".repeat(buffer.len());
    let save_tag = if save_password {
        "[✓] Save password in Windows Credential Manager"
    } else {
        "[ ] Save password in Windows Credential Manager"
    };

    let popup_bg = if theme.bg != Color::Reset {
        theme.bg
    } else {
        Color::Black
    };

    let lines = vec![
        Line::from(vec![
            Span::styled(
                " Password: ",
                Style::default()
                    .fg(theme.selected)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(masked, Style::default().fg(theme.file)),
            Span::styled("█", Style::default().fg(theme.tree_cursor)),
        ]),
        Line::from(vec![Span::styled(
            format!(" {save_tag} (Tab to toggle)"),
            Style::default().fg(theme.border_inactive),
        )]),
        Line::from(vec![Span::styled(
            " [Enter] Connect  [Esc] Cancel",
            Style::default().fg(theme.border_inactive),
        )]),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(popup_bg))
        .title(format!(" {title} "));
    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(popup_bg))
            .block(block),
        popup,
    );
}

fn draw_host_key_confirm_modal(frame: &mut Frame, area: Rect, endpoint: &str, fingerprint: &str) {
    let width = 72.min(area.width.saturating_sub(4)).max(40);
    let height = 9;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup);

    let lines = vec![
        Line::from(Span::styled(
            "Unknown host key for:",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            format!("  {endpoint}"),
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            "Fingerprint (SHA256):",
            Style::default().fg(Color::Yellow),
        )),
        Line::from(Span::styled(
            format!("  {fingerprint}"),
            Style::default().fg(Color::Cyan),
        )),
        Line::from(""),
        Line::from(Span::styled(
            "Trust this host? (y/Enter: Trust, n/Esc: Cancel)",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Red))
        .title(" ⚠ Unknown Host Key ");
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn draw_connection_manager_modal(frame: &mut Frame, area: Rect, app: &AppState, cursor: usize) {
    let width = 70.min(area.width.saturating_sub(4)).max(40);
    let height = 14.min(area.height.saturating_sub(4)).max(8);
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup);

    let inner_h = height.saturating_sub(4) as usize;
    let page_size = inner_h.max(1);
    let start_offset = (cursor / page_size) * page_size;

    let popup_bg = if app.theme.bg != Color::Reset {
        app.theme.bg
    } else {
        Color::Black
    };

    let items: Vec<ListItem> = if app.remote_profiles.is_empty() {
        vec![
            ListItem::new("  (No profiles configured. Press 'a' to add new profile)")
                .style(Style::default().fg(app.theme.border_inactive).bg(popup_bg)),
        ]
    } else {
        app.remote_profiles
            .iter()
            .enumerate()
            .skip(start_offset)
            .take(page_size)
            .map(|(i, p)| {
                let sel = i == cursor;
                let style = if sel {
                    Style::default()
                        .fg(app.theme.cursor_fg)
                        .bg(app.theme.cursor_bg)
                        .add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(app.theme.file).bg(popup_bg)
                };
                let label = format!("  {}  ({}:{})", p.display_label(), p.host, p.port);
                ListItem::new(label).style(style)
            })
            .collect()
    };

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(1), Constraint::Length(2)])
        .split(popup);

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::TOP | Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(app.theme.border))
            .style(Style::default().bg(popup_bg))
            .title(" SFTP Connection Profiles "),
    );
    frame.render_widget(list, chunks[0]);

    let help = Paragraph::new(Line::from(vec![Span::styled(
        " [Enter] Connect  [a] Add  [e] Edit  [d] Delete  [Esc] Close ",
        Style::default()
            .fg(app.theme.status_bar_fg)
            .bg(app.theme.status_bar_bg),
    )]))
    .style(Style::default().bg(popup_bg))
    .block(
        Block::default()
            .borders(Borders::BOTTOM | Borders::LEFT | Borders::RIGHT)
            .border_style(Style::default().fg(app.theme.border))
            .style(Style::default().bg(popup_bg)),
    );
    frame.render_widget(help, chunks[1]);
}

fn draw_connection_edit_modal(
    frame: &mut Frame,
    area: Rect,
    is_new: bool,
    profile: &ConnectionProfile,
    cursor: usize,
    password_buffer: &str,
    theme: &crate::theme::Theme,
) {
    let width = 64.min(area.width.saturating_sub(4)).max(40);
    let height = 12;
    let x = (area.width.saturating_sub(width)) / 2;
    let y = (area.height.saturating_sub(height)) / 2;
    let popup = Rect::new(x, y, width, height);
    frame.render_widget(Clear, popup);

    let port_str = profile.port.to_string();
    let pw_masked = "*".repeat(password_buffer.len());
    let save_str = if profile.save_password {
        "[✓] Yes (Space to toggle)"
    } else {
        "[ ] No (Space to toggle)"
    };

    let fields: [(&str, &str); 7] = [
        ("Profile Name", profile.name.as_str()),
        ("Host / IP", profile.host.as_str()),
        ("Port", &port_str),
        ("Username", profile.user.as_str()),
        ("Password", &pw_masked),
        ("Save Password", save_str),
        ("Initial Dir", profile.initial_dir.as_str()),
    ];

    let lines: Vec<Line> = fields
        .iter()
        .enumerate()
        .map(|(i, (label, val))| {
            let is_cur = i == cursor;
            let label_style = if is_cur {
                Style::default()
                    .fg(theme.selected)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.border_inactive)
            };
            let val_style = if is_cur {
                Style::default()
                    .fg(theme.cursor_fg)
                    .bg(theme.selected)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(theme.file)
            };
            Line::from(vec![
                Span::styled(format!("  {:14}: ", label), label_style),
                Span::styled(
                    format!(" {} ", if val.is_empty() { "(empty)" } else { val }),
                    val_style,
                ),
            ])
        })
        .collect();

    let title = if is_new {
        " New SFTP Profile "
    } else {
        " Edit SFTP Profile "
    };
    let popup_bg = if theme.bg != Color::Reset {
        theme.bg
    } else {
        Color::Black
    };
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.border))
        .style(Style::default().bg(popup_bg))
        .title(format!("{title}(Tab/Down:Next, Enter:Save, Esc:Cancel) "));

    frame.render_widget(
        Paragraph::new(lines)
            .style(Style::default().bg(popup_bg))
            .block(block),
        popup,
    );
}

fn draw_transfer_progress_modal(frame: &mut Frame, area: Rect, state: &TransferProgressState) {
    let width = 64.min(area.width.saturating_sub(4)).max(30);
    let height = 11.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_rect = Rect {
        x,
        y,
        width,
        height,
    };

    frame.render_widget(Clear, modal_rect);

    let border_style = if state.is_cancelling {
        Style::default()
            .fg(Color::Yellow)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    };

    let title_text = format!(" {} ", state.title);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(title_text, border_style));

    let inner = block.inner(modal_rect);
    frame.render_widget(block, modal_rect);

    if inner.width < 4 || inner.height < 5 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Current file label
            Constraint::Length(1), // Current file gauge
            Constraint::Length(1), // Blank separator
            Constraint::Length(1), // Overall label
            Constraint::Length(1), // Overall gauge
            Constraint::Min(1),    // Cancel note
        ])
        .split(inner);

    // 1. Current file label
    let cur_fname = if state.current_file.is_empty() {
        "Preparing transfer...".to_string()
    } else {
        format!(
            "File: {}",
            truncate_display_width(
                &state.current_file,
                (inner.width as usize).saturating_sub(6)
            )
        )
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            cur_fname,
            Style::default().fg(Color::White),
        )])),
        chunks[0],
    );

    // 2. Current file gauge
    let cur_ratio = if state.current_file_total > 0 {
        (state.current_file_bytes as f64 / state.current_file_total as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let cur_percent = (cur_ratio * 100.0) as u16;
    let cur_label = if state.current_file_total > 0 {
        format!(
            "{} / {} ({}%)",
            format_size(state.current_file_bytes),
            format_size(state.current_file_total),
            cur_percent
        )
    } else {
        format_size(state.current_file_bytes)
    };
    let cur_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Cyan).bg(Color::DarkGray))
        .ratio(cur_ratio)
        .label(Span::styled(
            cur_label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(cur_gauge, chunks[1]);

    // 3. Overall label
    let total_files_str = if state.total_files > 0 {
        format!("{} / {} files", state.files_transferred, state.total_files)
    } else {
        format!("{} files", state.files_transferred)
    };
    let total_bytes_str = if state.total_bytes > 0 {
        format!(
            "{} / {}",
            format_size(state.total_bytes_transferred),
            format_size(state.total_bytes)
        )
    } else {
        format_size(state.total_bytes_transferred)
    };
    let overall_text = format!("Total: {total_files_str} ({total_bytes_str})");
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            overall_text,
            Style::default().fg(Color::White),
        )])),
        chunks[3],
    );

    // 4. Overall gauge
    let overall_ratio = if state.total_bytes > 0 {
        (state.total_bytes_transferred as f64 / state.total_bytes as f64).clamp(0.0, 1.0)
    } else if state.total_files > 0 {
        (state.files_transferred as f64 / state.total_files as f64).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let overall_percent = (overall_ratio * 100.0) as u16;
    let overall_label = format!("{overall_percent}%");
    let overall_gauge = Gauge::default()
        .gauge_style(Style::default().fg(Color::Green).bg(Color::DarkGray))
        .ratio(overall_ratio)
        .label(Span::styled(
            overall_label,
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        ));
    frame.render_widget(overall_gauge, chunks[4]);

    // 5. Cancel note
    let cancel_line = if state.is_cancelling {
        Line::from(vec![Span::styled(
            " Cancelling transfer... Please wait ",
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )])
    } else {
        Line::from(vec![
            Span::styled(" Press ", Style::default().fg(Color::DarkGray)),
            Span::styled(
                "[Esc]",
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" to cancel ", Style::default().fg(Color::DarkGray)),
        ])
    };
    frame.render_widget(Paragraph::new(cancel_line), chunks[5]);
}

fn draw_conflict_confirm_modal(frame: &mut Frame, area: Rect, prompt: &ConflictPromptData) {
    let width = 76.min(area.width.saturating_sub(2)).max(30);
    let height = 9.min(area.height.saturating_sub(2));
    let x = area.x + (area.width.saturating_sub(width)) / 2;
    let y = area.y + (area.height.saturating_sub(height)) / 2;
    let modal_rect = Rect {
        x,
        y,
        width,
        height,
    };

    frame.render_widget(Clear, modal_rect);

    let border_style = Style::default()
        .fg(Color::Yellow)
        .add_modifier(Modifier::BOLD);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(border_style)
        .title(Span::styled(" Confirm File Replace ", border_style));

    let inner = block.inner(modal_rect);
    frame.render_widget(block, modal_rect);

    if inner.width < 4 || inner.height < 5 {
        return;
    }

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // Warning text
            Constraint::Length(1), // Target filename
            Constraint::Length(1), // Blank
            Constraint::Length(1), // Action buttons
            Constraint::Min(1),    // Shortcut hint
        ])
        .split(inner);

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Destination file already exists:",
            Style::default().fg(Color::White),
        )])),
        chunks[0],
    );

    let display_fname = truncate_display_width(&prompt.file_name, inner.width as usize);
    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            format!("  {display_fname}"),
            Style::default()
                .fg(Color::Yellow)
                .add_modifier(Modifier::BOLD),
        )])),
        chunks[1],
    );

    // Buttons
    let buttons = [
        (" Overwrite(o) ", 0),
        (" Skip(s) ", 1),
        (" OverwriteAll(a) ", 2),
        (" SkipAll(k) ", 3),
        (" Cancel(Esc) ", 4),
    ];
    let mut btn_spans = Vec::new();
    for (label, idx) in buttons {
        if idx == prompt.cursor {
            btn_spans.push(Span::styled(
                label,
                Style::default()
                    .fg(Color::Black)
                    .bg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            ));
        } else {
            btn_spans.push(Span::styled(
                label,
                Style::default().fg(Color::White).bg(Color::DarkGray),
            ));
        }
        btn_spans.push(Span::raw(" "));
    }
    frame.render_widget(Paragraph::new(Line::from(btn_spans)), chunks[3]);

    frame.render_widget(
        Paragraph::new(Line::from(vec![Span::styled(
            "Left/Right/Tab: Select | Enter: Confirm | o/s/a/k/Esc: Quick key",
            Style::default().fg(Color::DarkGray),
        )])),
        chunks[4],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;

    #[test]
    fn headless_test_backend_renders_app_state_and_japanese_filenames() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("日本語ファイル.txt"), b"hello world").unwrap();
        std::fs::create_dir(tmp.path().join("サブフォルダ")).unwrap();

        let app = AppState::new(tmp.path().to_path_buf());
        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();

        terminal.draw(|f| draw(f, &app)).unwrap();

        let buffer = terminal.backend().buffer();
        let mut rendered_text = String::new();
        for y in 0..buffer.area.height {
            let mut x = 0;
            while x < buffer.area.width {
                let sym = buffer[(x, y)].symbol();
                rendered_text.push_str(sym);
                let w = unicode_width::UnicodeWidthStr::width(sym).max(1) as u16;
                x += w;
            }
            rendered_text.push('\n');
        }

        assert!(rendered_text.contains("日本語ファイル.txt"));
        assert!(rendered_text.contains("サブフォルダ"));
        assert!(rendered_text.contains("Files ("));
        assert!(rendered_text.contains("tuif"));
    }

    #[test]
    fn markdown_viewer_renders_leaf_preview_and_toggles_raw_mode() {
        use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join("README.md"),
            b"# Title\n\n- item 1\n- item 2\n",
        )
        .unwrap();

        let mut app = AppState::new(tmp.path().to_path_buf());
        app.handle_key_event(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

        match &app.overlay {
            OverlayMode::Viewer {
                is_markdown,
                preview_mode,
                preview_lines,
                ..
            } => {
                assert!(*is_markdown);
                if preview_lines.is_some() {
                    assert!(*preview_mode);
                }
            }
            other => panic!("expected Viewer overlay, got {other:?}"),
        }

        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        // Press 'p' to toggle between Preview and Raw mode when leaf is available
        if let OverlayMode::Viewer {
            preview_lines: Some(_),
            preview_mode: initial_mode,
            ..
        } = &app.overlay
        {
            let before = *initial_mode;
            app.handle_key_event(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::NONE));
            if let OverlayMode::Viewer { preview_mode, .. } = &app.overlay {
                assert_eq!(*preview_mode, !before);
            }
            terminal.draw(|f| draw(f, &app)).unwrap();
        }
    }

    #[test]
    fn transfer_progress_modal_renders_correctly() {
        use crate::app::ActiveTransfer;
        use std::sync::atomic::AtomicBool;
        use std::sync::{mpsc, Arc};

        let tmp = tempfile::tempdir().unwrap();
        let mut app = AppState::new(tmp.path().to_path_buf());

        let (_tx, rx) = mpsc::channel();
        app.active_transfer = Some(ActiveTransfer {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            rx,
            current_state: TransferProgressState {
                title: "Uploading to Remote...".to_string(),
                current_file: "large_data.bin".to_string(),
                current_file_bytes: 50 * 1024 * 1024,
                current_file_total: 100 * 1024 * 1024,
                total_bytes_transferred: 50 * 1024 * 1024,
                total_bytes: 200 * 1024 * 1024,
                files_transferred: 1,
                total_files: 3,
                is_cancelling: false,
            },
            conflict_prompt: None,
        });

        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = (0..buffer.area.height)
            .map(|y| {
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                row
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Uploading to Remote..."));
        assert!(rendered.contains("large_data.bin"));
        assert!(rendered.contains("[Esc]"));
    }

    #[test]
    fn conflict_confirm_modal_renders_choices_and_filename() {
        use crate::app::{ActiveTransfer, ConflictPromptData};
        use std::sync::atomic::AtomicBool;
        use std::sync::{mpsc, Arc};

        let tmp = tempfile::tempdir().unwrap();
        let mut app = AppState::new(tmp.path().to_path_buf());

        let (_tx, rx) = mpsc::channel();
        let (reply_tx, _reply_rx) = mpsc::channel();
        app.active_transfer = Some(ActiveTransfer {
            cancel_flag: Arc::new(AtomicBool::new(false)),
            rx,
            current_state: TransferProgressState {
                title: "Uploading...".to_string(),
                current_file: "duplicate.txt".to_string(),
                current_file_bytes: 10,
                current_file_total: 100,
                total_bytes_transferred: 10,
                total_bytes: 100,
                files_transferred: 0,
                total_files: 1,
                is_cancelling: false,
            },
            conflict_prompt: Some(ConflictPromptData {
                file_name: "duplicate.txt".to_string(),
                reply_tx,
                cursor: 0,
            }),
        });

        let backend = TestBackend::new(100, 28);
        let mut terminal = Terminal::new(backend).unwrap();
        terminal.draw(|f| draw(f, &app)).unwrap();

        let buffer = terminal.backend().buffer();
        let rendered: String = (0..buffer.area.height)
            .map(|y| {
                let row: String = (0..buffer.area.width)
                    .map(|x| buffer[(x, y)].symbol())
                    .collect();
                row
            })
            .collect::<Vec<_>>()
            .join("\n");

        assert!(rendered.contains("Confirm File Replace"));
        assert!(rendered.contains("duplicate.txt"));
        assert!(rendered.contains("Overwrite(o)"));
        assert!(rendered.contains("Skip(s)"));
        assert!(rendered.contains("OverwriteAll(a)"));
        assert!(rendered.contains("SkipAll(k)"));
        assert!(rendered.contains("Cancel(Esc)"));
    }
}
