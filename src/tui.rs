//! 交互式 TUI：列出所有垃圾目录，让用户勾选后删除。
//!
//! 操作：
//! - ↑/↓ 或 j/k：移动光标
//! - 空格：勾选/取消当前项
//! - a：全选 / 全不选
//! - d：删除已勾选项（返回给 main 执行实际删除）
//! - q/Esc：退出，不做任何删除
//!
//! 设计：TUI 只负责「选」，不碰删除逻辑。退出时把用户的决定
//! （退出 or 删除哪些）返回给 main，由 main 调用 deleter。
//! 这样 TUI 与文件系统解耦，删除安全逻辑集中在 deleter 一处。

use crate::scanner::Finding;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use humansize::{DECIMAL, format_size};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, List, ListItem, ListState, Paragraph};

/// 用户在 TUI 中的最终决定。
pub enum Outcome {
    /// 退出，不删除任何东西。
    Quit,
    /// 删除这些 Finding。
    Delete(Vec<Finding>),
}

/// TUI 应用状态。选择逻辑全在这里，与终端绘制分离，便于单测。
struct App {
    findings: Vec<Finding>,
    selected: Vec<bool>,
    cursor: usize,
}

impl App {
    fn new(mut findings: Vec<Finding>) -> Self {
        // 按大小降序：最占地的排最前，用户一眼看到大头
        findings.sort_by_key(|f| std::cmp::Reverse(f.size));
        let n = findings.len();
        App {
            findings,
            selected: vec![false; n],
            cursor: 0,
        }
    }

    fn move_down(&mut self) {
        if self.cursor + 1 < self.findings.len() {
            self.cursor += 1;
        }
    }

    fn move_up(&mut self) {
        if self.cursor > 0 {
            self.cursor -= 1;
        }
    }

    fn toggle(&mut self) {
        if let Some(s) = self.selected.get_mut(self.cursor) {
            *s = !*s;
        }
    }

    /// 全选；若已全选则全部取消。
    fn toggle_all(&mut self) {
        let all = !self.selected.is_empty() && self.selected.iter().all(|&s| s);
        for s in self.selected.iter_mut() {
            *s = !all;
        }
    }

    /// 已勾选的条数与总字节数。
    fn selected_total(&self) -> (usize, u64) {
        let mut count = 0;
        let mut bytes = 0u64;
        for (i, &sel) in self.selected.iter().enumerate() {
            if sel {
                count += 1;
                bytes = bytes.saturating_add(self.findings[i].size);
            }
        }
        (count, bytes)
    }

    /// 消费 self，返回被勾选的 Finding。
    fn into_selected(self) -> Vec<Finding> {
        self.findings
            .into_iter()
            .zip(self.selected)
            .filter_map(|(f, sel)| if sel { Some(f) } else { None })
            .collect()
    }
}

/// 启动 TUI，阻塞直到用户退出或确认删除。
/// 空列表直接返回 Quit，不进终端。
pub fn run(findings: Vec<Finding>) -> std::io::Result<Outcome> {
    if findings.is_empty() {
        return Ok(Outcome::Quit);
    }

    let mut terminal = ratatui::init();
    let mut app = App::new(findings);

    let outcome = loop {
        terminal.draw(|frame| draw(frame, &app))?;

        if let Event::Key(key) = event::read()? {
            // Windows 下 crossterm 会同时发 Press 和 Release，只认 Press 防重复
            if key.kind != KeyEventKind::Press {
                continue;
            }
            match key.code {
                KeyCode::Char('q') | KeyCode::Esc => break Outcome::Quit,
                KeyCode::Down | KeyCode::Char('j') => app.move_down(),
                KeyCode::Up | KeyCode::Char('k') => app.move_up(),
                KeyCode::Char(' ') => app.toggle(),
                KeyCode::Char('a') => app.toggle_all(),
                KeyCode::Char('d') => {
                    let (count, _) = app.selected_total();
                    if count > 0 {
                        break Outcome::Delete(app.into_selected());
                    }
                }
                _ => {}
            }
        }
    };

    ratatui::restore();
    Ok(outcome)
}

fn draw(frame: &mut Frame, app: &App) {
    let chunks = Layout::vertical([
        Constraint::Length(1), // 标题
        Constraint::Min(1),    // 列表
        Constraint::Length(1), // 帮助 / 已选汇总
    ])
    .split(frame.area());

    // ---- 标题 ----
    let total_bytes: u64 = app.findings.iter().map(|f| f.size).sum();
    let title = format!(
        " reclaim — 共 {} 项垃圾，合计 {} ",
        app.findings.len(),
        format_size(total_bytes, DECIMAL)
    );
    frame.render_widget(
        Paragraph::new(title).style(Style::new().add_modifier(Modifier::BOLD)),
        chunks[0],
    );

    // ---- 列表 ----
    let items: Vec<ListItem> = app
        .findings
        .iter()
        .enumerate()
        .map(|(i, f)| {
            let check = if app.selected[i] { "[x]" } else { "[ ]" };
            let caution = if f.caution { " ⚠ 谨慎" } else { "" };
            let line = format!(
                "{check} {:>10}  {:<9} {}{caution}",
                format_size(f.size, DECIMAL),
                f.ecosystem,
                f.path.display()
            );
            let style = if f.caution {
                Style::new().fg(Color::Yellow)
            } else {
                Style::new()
            };
            ListItem::new(line).style(style)
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(app.cursor));
    let list = List::new(items)
        .block(Block::new().borders(Borders::ALL))
        .highlight_style(Style::new().add_modifier(Modifier::REVERSED))
        .highlight_symbol("› ");
    frame.render_stateful_widget(list, chunks[1], &mut state);

    // ---- 底部帮助 + 已选汇总 ----
    let (count, bytes) = app.selected_total();
    let help = format!(
        " ↑/↓ 移动   空格 勾选   a 全选   d 删除   q 退出    │    已选 {} 项 / {} ",
        count,
        format_size(bytes, DECIMAL)
    );
    frame.render_widget(
        Paragraph::new(help).style(Style::new().fg(Color::Cyan)),
        chunks[2],
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn mk(size: u64) -> Finding {
        Finding {
            path: PathBuf::from(format!("/tmp/junk-{size}")),
            ecosystem: "Test",
            target: "x",
            note: "n",
            caution: false,
            size,
            newest_mtime_secs: Some(1_700_000_000),
        }
    }

    #[test]
    fn sorts_by_size_desc() {
        let app = App::new(vec![mk(10), mk(100), mk(50)]);
        assert_eq!(app.findings[0].size, 100);
        assert_eq!(app.findings[1].size, 50);
        assert_eq!(app.findings[2].size, 10);
    }

    #[test]
    fn toggle_marks_current_and_totals() {
        let mut app = App::new(vec![mk(100), mk(50)]);
        app.toggle(); // 勾选光标处（最大的 100）
        let (c, b) = app.selected_total();
        assert_eq!(c, 1);
        assert_eq!(b, 100);
    }

    #[test]
    fn toggle_all_selects_then_clears() {
        let mut app = App::new(vec![mk(1), mk(2)]);
        app.toggle_all();
        assert_eq!(app.selected_total().0, 2);
        app.toggle_all();
        assert_eq!(app.selected_total().0, 0);
    }

    #[test]
    fn into_selected_returns_only_checked() {
        let mut app = App::new(vec![mk(100), mk(50), mk(25)]);
        app.toggle(); // 100
        app.move_down();
        app.move_down();
        app.toggle(); // 25
        let sel = app.into_selected();
        assert_eq!(sel.len(), 2);
        // 应包含 100 和 25，不含 50
        assert!(sel.iter().any(|f| f.size == 100));
        assert!(sel.iter().any(|f| f.size == 25));
        assert!(!sel.iter().any(|f| f.size == 50));
    }

    #[test]
    fn cursor_stays_in_bounds() {
        let mut app = App::new(vec![mk(1), mk(2)]);
        app.move_up(); // 已在顶，不应越界
        assert_eq!(app.cursor, 0);
        app.move_down();
        app.move_down(); // 已在底，不应越界
        assert_eq!(app.cursor, 1);
    }
}
