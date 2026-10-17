//! `tui/` — Ratatui rendering and terminal management for `cuifiler` (SPEC-0300-0001).

use ratatui::layout::{Constraint, Direction, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Row, Table};
use ratatui::Frame;
use unicode_width::UnicodeWidthStr;

use crate::app::{normalize_tree_key, AppState, ClipMode, FocusPane, OverlayMode};
use crate::fs::query::EntryType;

/// Pure view rendering function (`coding-standards` §3: no side effects).
pub fn draw(frame: &mut Frame, app: &AppState) {
    let area = frame.area();

    // If fullscreen Viewer overlay is active, render it across the entire screen
    if let OverlayMode::Viewer {
        ref title,
        ref lines,
        scroll_y,
        scroll_x,
        ..
    } = app.overlay
    {
        draw_viewer(frame, area, title, lines, scroll_y, scroll_x);
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
            draw_input_modal(frame, area, title, buffer);
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
            draw_popup_list(frame, area, "Sort (j/k:Move Enter:Apply Esc:Close)", &items, *cursor);
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
            draw_popup_list(frame, area, title, &display_items, *cursor);
        }
        OverlayMode::ActionMenu { cursor } => {
            let mut items = vec![
                "Open (Enter)".to_string(),
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
                items.push(format!("[CUI] {} ({})", ext.name, ext.key_label));
            }
            draw_popup_list(frame, area, "Action Menu (j/k/Enter/Esc)", &items, *cursor);
        }
        OverlayMode::MenuBar { cat_idx, item_idx } => {
            draw_menubar_dropdown(frame, area, app, *cat_idx, *item_idx);
        }
    }
}

fn draw_menu_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let active_cat = match app.overlay {
        OverlayMode::MenuBar { cat_idx, .. } => Some(cat_idx),
        _ => None,
    };
    let labels = [
        " File(F) ",
        " Edit(E) ",
        " View(V) ",
        " Go(G) ",
    ];
    let mut spans = Vec::new();
    for (i, label) in labels.iter().enumerate() {
        let style = if active_cat == Some(i) {
            Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White).bg(Color::DarkGray)
        };
        spans.push(Span::styled(*label, style));
        spans.push(Span::styled(" ", Style::default().bg(Color::DarkGray)));
    }
    spans.push(Span::styled(
        "  tuif",
        Style::default().fg(Color::Gray).bg(Color::DarkGray),
    ));
    let bar = Paragraph::new(Line::from(spans)).style(Style::default().bg(Color::DarkGray));
    frame.render_widget(bar, area);
}

fn draw_path_and_search_bar(frame: &mut Frame, area: Rect, app: &AppState) {
    let mut lines = vec![Line::from(vec![
        Span::styled(" Path: ", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(app.current_dir.to_string_lossy().into_owned()),
    ])];
    if !app.search_query.is_empty() {
        let mode_tag = if app.search_regex { "[Regex]" } else { "[Substr]" };
        lines.push(Line::from(vec![
            Span::styled(
                format!(" Search {mode_tag}: "),
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD),
            ),
            Span::raw(app.search_query.clone()),
        ]));
    }
    frame.render_widget(Paragraph::new(lines), area);
}

fn draw_main_body(frame: &mut Frame, area: Rect, app: &AppState) {
    if app.ui_state.ui_show_tree {
        let h_chunks = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(24), Constraint::Percentage(76)])
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
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let inner_height = area.height.saturating_sub(2) as usize;
    let page_size = inner_height.max(1);
    let start_offset = (app.tree_cursor / page_size) * page_size;
    let current_dir_key = normalize_tree_key(&app.current_dir);

    let items: Vec<ListItem> = app
        .tree_nodes
        .iter()
        .enumerate()
        .skip(start_offset)
        .take(page_size)
        .map(|(i, node)| {
            let is_current_dir = normalize_tree_key(&node.path) == current_dir_key;
            let indent = "  ".repeat(node.depth);
            let caret = if node.expanded { "▾ " } else { "▸ " };
            let text = format!("{indent}{caret}{}", node.name);
            let style = if i == app.tree_cursor && focused {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if is_current_dir {
                Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if i == app.tree_cursor {
                Style::default().fg(Color::Yellow)
            } else {
                Style::default()
            };
            ListItem::new(text).style(style)
        })
        .collect();

    let list = List::new(items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(border_style)
            .title(" Drives / Tree (Tab | l:Expand h:Collapse Enter:Open) "),
    );
    frame.render_widget(list, area);
}

fn draw_file_list_pane(frame: &mut Frame, area: Rect, app: &AppState) {
    let focused = app.focus_pane == FocusPane::FileList;
    let border_style = if focused {
        Style::default().fg(Color::Cyan)
    } else {
        Style::default().fg(Color::DarkGray)
    };

    let rows_per_col = area.height.saturating_sub(2).max(1) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let mut min_col_width: usize = 24;
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
        .title(format!(
            " {left_arrow}Files ({} items) [sort: {} {}]{right_arrow} ",
            app.visible_indices.len(),
            app.ui_state.sort_key,
            app.ui_state.sort_order
        ));

    if app.visible_indices.is_empty() {
        let empty = Paragraph::new("  No items")
            .style(Style::default().fg(Color::DarkGray))
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
        let max_name_width = (table_rect.width as usize).saturating_sub(reserved_cols).max(8);

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
            let name_col = format!("{mark} {}", truncate_display_width(&entry.name, max_name_width));
            let size_col = if entry.entry_type == EntryType::Dir {
                "<DIR>".to_string()
            } else {
                format_size(entry.size)
            };
            let time_col = entry.modified.clone();

            let mut cells = vec![name_col];
            if app.ui_state.ui_show_size {
                cells.push(size_col);
            }
            if app.ui_state.ui_show_time {
                cells.push(time_col);
            }

            let style = if is_focused && focused {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else if is_selected {
                Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)
            } else if entry.entry_type == EntryType::Dir {
                Style::default().fg(Color::LightBlue).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
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

        let table = Table::new(rows, widths);
        frame.render_widget(table, table_rect);

        if has_right_neighbor && col_rect.width > 2 {
            let sep_x = col_rect.x + col_rect.width - 1;
            let sep_lines: Vec<Line> = (0..col_rect.height)
                .map(|_| Line::from(Span::styled("│", Style::default().fg(Color::DarkGray))))
                .collect();
            frame.render_widget(
                Paragraph::new(sep_lines),
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
    let bar = Paragraph::new(text).style(Style::default().fg(Color::White).bg(Color::DarkGray));
    frame.render_widget(bar, area);
}

fn draw_viewer(
    frame: &mut Frame,
    area: Rect,
    title: &str,
    lines: &[String],
    scroll_y: usize,
    scroll_x: usize,
) {
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
            let mut spans = vec![Span::styled(
                format!("{:>5} │ ", idx + 1),
                Style::default().fg(Color::DarkGray),
            )];
            spans.extend(crate::viewer::highlight::parse_ansi_line_with_scroll(
                line, scroll_x,
            ));
            Line::from(spans)
        })
        .collect();

    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Cyan))
        .title(format!(" Viewer: {title} (Line {}/{}) ", scroll_y + 1, lines.len().max(1)));
    frame.render_widget(Paragraph::new(visible_lines).block(block), chunks[0]);

    let footer = Paragraph::new(" j/k/Up/Down:Scroll  Ctrl+d/u:Page  g/G:Top/Bottom  h/l:Horiz  q/Esc:Close")
        .style(Style::default().fg(Color::Black).bg(Color::Cyan));
    frame.render_widget(footer, chunks[1]);
}

fn draw_input_modal(frame: &mut Frame, area: Rect, title: &str, buffer: &str) {
    let popup = centered_rect(70, 5, area);
    frame.render_widget(Clear, popup);
    let block = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Yellow))
        .title(format!(" {title} "));
    let content = Paragraph::new(format!("{buffer}█")).block(block);
    frame.render_widget(content, popup);
}

fn draw_popup_list(frame: &mut Frame, area: Rect, title: &str, items: &[String], cursor: usize) {
    let height = (items.len() as u16 + 4).clamp(6, area.height.saturating_sub(4));
    let popup = centered_rect(65, height, area);
    frame.render_widget(Clear, popup);

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let style = if i == cursor {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!("  {item}")).style(style)
        })
        .collect();

    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan))
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
            v.push("Exit (q / Ctrl+Q)".to_string());
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
            format!("Tree [{}] (t / Ctrl+Shift+B)", if app.ui_state.ui_show_tree { "✓" } else { " " }),
            format!("Hidden Files [{}] (. / Ctrl+H)", if app.ui_state.ui_show_hidden { "✓" } else { " " }),
            format!("Size Column [{}] (Ctrl+Shift+S)", if app.ui_state.ui_show_size { "✓" } else { " " }),
            format!("Modified Column [{}] (Ctrl+Shift+T)", if app.ui_state.ui_show_time { "✓" } else { " " }),
            "Sort... (s)".to_string(),
        ],
        _ => vec![
            "Parent Folder (Backspace / -)".to_string(),
            "Focus Path Bar (Ctrl+L)".to_string(),
            "History (Ctrl+Y)".to_string(),
            "Jump List (Ctrl+J)".to_string(),
            "Add to Jump List (m / Ctrl+Shift+J)".to_string(),
        ],
    };

    let x_offset = (cat_idx as u16 * 10).min(area.width.saturating_sub(42));
    let height = (items.len() as u16 + 2).min(area.height.saturating_sub(2));
    let rect = Rect {
        x: area.x + x_offset,
        y: area.y + 1,
        width: 42.min(area.width),
        height,
    };
    frame.render_widget(Clear, rect);

    let list_items: Vec<ListItem> = items
        .iter()
        .enumerate()
        .map(|(i, s)| {
            let style = if i == item_idx {
                Style::default().fg(Color::Black).bg(Color::Cyan).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            ListItem::new(format!(" {s}")).style(style)
        })
        .collect();
    let list = List::new(list_items).block(
        Block::default()
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::Cyan)),
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
}
