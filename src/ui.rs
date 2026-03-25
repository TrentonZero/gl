use crate::{
    config::AppConfig,
    git::{BranchDiff, DiffLineKind, DiffStat, RepoState},
};
use ratatui::{
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Wrap},
    Frame,
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FocusedPane {
    BranchList,
    Diff,
}

#[derive(Debug, Clone)]
pub enum BranchEntry {
    Header {
        label: String,
    },
    Branch {
        branch_name: String,
        is_head: bool,
        commit_count: usize,
        ahead: usize,
        behind: usize,
        has_upstream: bool,
        indent: usize,
        stale: bool,
    },
}

#[derive(Debug, Clone)]
pub struct StackEntry {
    pub branch_name: String,
    pub commit_count: usize,
    pub diff_stat: DiffStat,
    pub ahead: usize,
    pub behind: usize,
    pub has_upstream: bool,
    pub is_head: bool,
    pub stale: bool,
}

impl BranchEntry {
    pub fn is_header(&self) -> bool {
        matches!(self, BranchEntry::Header { .. })
    }

    pub fn branch_name(&self) -> &str {
        match self {
            BranchEntry::Branch { branch_name, .. } => branch_name,
            BranchEntry::Header { label } => label,
        }
    }
}

pub fn draw(
    frame: &mut Frame<'_>,
    config: &AppConfig,
    repo: &RepoState,
    display_entries: &[BranchEntry],
    selected_index: usize,
    branch_diff: Option<&BranchDiff>,
    highlighted_diff: Option<&[Line<'static>]>,
    diff_scroll: usize,
    show_help: bool,
    focus: FocusedPane,
    search: Option<&str>,
    stack_notice: Option<&str>,
    stack_view: Option<&crate::StackViewState>,
) {
    let frame_area = frame.size();
    let areas = if config.chrome {
        Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Min(1),
                Constraint::Length(1),
            ])
            .split(frame_area)
            .to_vec()
    } else {
        let body = frame_area;
        vec![
            Rect::new(body.x, body.y, body.width, 0),
            body,
            Rect::new(body.x, body.y + body.height, body.width, 0),
        ]
    };

    if config.chrome {
        draw_status_bar(frame, areas[0], repo, branch_diff.is_some());
        draw_help_bar(frame, areas[2], branch_diff.is_some(), focus, search);
    }

    draw_body(
        frame,
        areas[1],
        display_entries,
        selected_index,
        branch_diff,
        highlighted_diff,
        diff_scroll,
        focus,
        stack_notice,
        stack_view,
    );

    if show_help {
        draw_help_overlay(frame, frame_area);
    }
}

fn draw_status_bar(frame: &mut Frame<'_>, area: Rect, repo: &RepoState, detail: bool) {
    let title = if detail {
        "GL — Green Ledger · Detail"
    } else {
        "GL — Green Ledger · Branches"
    };
    let line = Line::from(vec![
        Span::styled(
            title,
            Style::default()
                .fg(Color::Black)
                .add_modifier(Modifier::BOLD),
        ),
        Span::raw(" "),
        Span::styled(
            repo.root.display().to_string(),
            Style::default().fg(Color::Black),
        ),
    ]);
    frame.render_widget(
        Paragraph::new(line)
            .style(Style::default().bg(Color::Blue))
            .alignment(Alignment::Left),
        area,
    );
}

fn draw_help_bar(
    frame: &mut Frame<'_>,
    area: Rect,
    detail: bool,
    focus: FocusedPane,
    search: Option<&str>,
) {
    let hints = if detail {
        match focus {
            FocusedPane::BranchList => {
                "j/k move  J/K stacks  Enter open  s stack  Esc close  q quit"
            }
            FocusedPane::Diff => {
                "j/k scroll  J/K files  gg/G ends  Ctrl-d/u page  / search  n/N next  Esc list"
            }
        }
    } else {
        "j/k move  J/K stacks  Enter open  s stack  2 stack view  R refresh"
    };

    let mut line = Line::from(Span::styled(hints, Style::default().fg(Color::Gray)));
    if let Some(search) = search {
        line.spans.push(Span::raw("  "));
        line.spans.push(Span::styled(
            format!("search: {search}"),
            Style::default().fg(Color::Yellow),
        ));
    }

    frame.render_widget(Paragraph::new(line), area);
}

fn draw_body(
    frame: &mut Frame<'_>,
    area: Rect,
    display_entries: &[BranchEntry],
    selected_index: usize,
    branch_diff: Option<&BranchDiff>,
    highlighted_diff: Option<&[Line<'static>]>,
    diff_scroll: usize,
    focus: FocusedPane,
    stack_notice: Option<&str>,
    stack_view: Option<&crate::StackViewState>,
) {
    if let Some(stack_view) = stack_view {
        draw_stack_view(frame, area, stack_view);
        return;
    }

    match branch_diff {
        Some(diff) => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(34), Constraint::Min(1)])
                .split(area);
            draw_branch_list(
                frame,
                panes[0],
                display_entries,
                selected_index,
                focus == FocusedPane::BranchList,
                stack_notice,
            );
            draw_diff(
                frame,
                panes[1],
                diff,
                highlighted_diff,
                diff_scroll,
                focus == FocusedPane::Diff,
            );
        }
        None => draw_branch_list(
            frame,
            area,
            display_entries,
            selected_index,
            true,
            stack_notice,
        ),
    }
}

fn draw_stack_view(frame: &mut Frame<'_>, area: Rect, stack_view: &crate::StackViewState) {
    let items: Vec<ListItem<'_>> = stack_view
        .entries
        .iter()
        .enumerate()
        .map(|(index, entry)| {
            ListItem::new(render_stack_entry(index, entry, stack_view.entries.len()))
        })
        .collect();

    let mut state = ListState::default();
    state.select(Some(stack_view.selected_index));

    let list = List::new(items)
        .block(
            Block::default()
                .title(format!("Stack View · {}", stack_view.stack_name))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::Blue)),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(51, 70, 124))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ");

    frame.render_stateful_widget(list, area, &mut state);
}

fn draw_branch_list(
    frame: &mut Frame<'_>,
    area: Rect,
    display_entries: &[BranchEntry],
    selected_index: usize,
    focused: bool,
    stack_notice: Option<&str>,
) {
    let items: Vec<ListItem<'_>> = display_entries
        .iter()
        .map(branch_entry_item)
        .map(ListItem::new)
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected_index));

    let block = Block::default()
        .title(match stack_notice {
            Some(_) => "Branches [degraded]",
            None => "Branches",
        })
        .borders(Borders::ALL)
        .border_style(if focused {
            Style::default().fg(Color::Blue)
        } else {
            Style::default().fg(Color::DarkGray)
        });

    let list = List::new(items)
        .block(block)
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(51, 70, 124))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ");

    frame.render_stateful_widget(list, area, &mut state);

    if let Some(message) = stack_notice {
        let notice_area = Rect::new(
            area.x.saturating_add(2),
            area.y.saturating_add(1),
            area.width.saturating_sub(4),
            1,
        );
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                message,
                Style::default().fg(Color::Yellow),
            ))),
            notice_area,
        );
    }
}

fn branch_entry_item(entry: &BranchEntry) -> Line<'static> {
    match entry {
        BranchEntry::Header { label } => Line::from(vec![Span::styled(
            format!(" {label}"),
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )]),
        BranchEntry::Branch {
            branch_name,
            is_head,
            commit_count,
            ahead,
            behind,
            has_upstream,
            indent,
            stale,
        } => {
            let mut spans = Vec::new();

            // Indentation with connecting line
            if *indent > 0 {
                for i in 0..*indent {
                    if i == indent - 1 {
                        spans.push(Span::styled("├ ", Style::default().fg(Color::DarkGray)));
                    } else {
                        spans.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
                    }
                }
            }

            // Stale indicator
            if *stale {
                spans.push(Span::styled("⚠ ", Style::default().fg(Color::Yellow)));
            }

            // Head indicator
            if *is_head {
                spans.push(Span::styled("● ", Style::default().fg(Color::Green)));
            } else if !*stale {
                spans.push(Span::raw("  "));
            }

            // Branch name - truncate to fit
            let max_name_len = if *indent > 0 { 20 - indent * 2 } else { 20 };
            let display_name = if branch_name.len() > max_name_len {
                format!("{:.width$}", branch_name, width = max_name_len)
            } else {
                format!("{:<width$}", branch_name, width = max_name_len)
            };
            spans.push(Span::styled(
                display_name,
                Style::default().fg(Color::White),
            ));

            // Commit count
            if *commit_count > 0 {
                spans.push(Span::styled(
                    format!(" {:>3}c", commit_count),
                    Style::default().fg(Color::Gray),
                ));
            }

            // Sync status
            if *ahead == 0 && *behind == 0 && *has_upstream {
                spans.push(Span::styled(" ✓", Style::default().fg(Color::Green)));
            } else {
                if *ahead > 0 {
                    spans.push(Span::styled(
                        format!(" ↑{}", ahead),
                        Style::default().fg(Color::Green),
                    ));
                }
                if *behind > 0 {
                    spans.push(Span::styled(
                        format!(" ↓{}", behind),
                        Style::default().fg(Color::Red),
                    ));
                }
            }

            Line::from(spans)
        }
    }
}

fn render_stack_entry(index: usize, entry: &StackEntry, total: usize) -> Line<'static> {
    let mut spans = Vec::new();

    if index + 1 < total {
        spans.push(Span::styled("│ ", Style::default().fg(Color::DarkGray)));
    } else {
        spans.push(Span::styled("● ", Style::default().fg(Color::DarkGray)));
    }

    if entry.stale {
        spans.push(Span::styled("⚠ ", Style::default().fg(Color::Yellow)));
    } else if entry.is_head {
        spans.push(Span::styled("● ", Style::default().fg(Color::Green)));
    } else {
        spans.push(Span::raw("  "));
    }

    spans.push(Span::styled(
        format!("{:<20}", entry.branch_name),
        Style::default().fg(Color::White),
    ));
    spans.push(Span::styled(
        format!(
            " {:>3}c {:>2}f +{} -{}",
            entry.commit_count,
            entry.diff_stat.files_changed,
            entry.diff_stat.insertions,
            entry.diff_stat.deletions
        ),
        Style::default().fg(Color::Gray),
    ));

    if entry.ahead == 0 && entry.behind == 0 && entry.has_upstream {
        spans.push(Span::styled(" ✓", Style::default().fg(Color::Green)));
    } else {
        if entry.ahead > 0 {
            spans.push(Span::styled(
                format!(" ↑{}", entry.ahead),
                Style::default().fg(Color::Green),
            ));
        }
        if entry.behind > 0 {
            spans.push(Span::styled(
                format!(" ↓{}", entry.behind),
                Style::default().fg(Color::Red),
            ));
        }
        if !entry.has_upstream {
            spans.push(Span::styled(" local", Style::default().fg(Color::Yellow)));
        }
    }

    Line::from(spans)
}

fn draw_diff(
    frame: &mut Frame<'_>,
    area: Rect,
    diff: &BranchDiff,
    highlighted_diff: Option<&[Line<'static>]>,
    diff_scroll: usize,
    focused: bool,
) {
    let title = match &diff.base_ref {
        Some(base_ref) => format!("{} vs {}", diff.branch_name, base_ref),
        None => diff.branch_name.clone(),
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if focused {
            Style::default().fg(Color::Blue)
        } else {
            Style::default().fg(Color::DarkGray)
        });

    let lines: Vec<Line<'_>> = match highlighted_diff {
        Some(lines) => lines.to_vec(),
        None => diff
            .lines
            .iter()
            .enumerate()
            .map(|(idx, line)| render_diff_line(idx, line))
            .collect(),
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((diff_scroll as u16, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn render_diff_line(index: usize, line: &crate::git::DiffLine) -> Line<'static> {
    let (fg, bg, bold) = match line.kind {
        DiffLineKind::File => (Color::Magenta, Color::Reset, true),
        DiffLineKind::Hunk => (Color::Cyan, Color::Reset, false),
        DiffLineKind::Context => (Color::Gray, Color::Reset, false),
        DiffLineKind::Add => (Color::Green, Color::Rgb(31, 53, 31), false),
        DiffLineKind::Del => (Color::Red, Color::Rgb(59, 22, 22), false),
        DiffLineKind::Meta => (Color::Yellow, Color::Reset, false),
    };

    let mut style = Style::default().fg(fg).bg(bg);
    if bold {
        style = style.add_modifier(Modifier::BOLD);
    }

    let gutter = if matches!(line.kind, DiffLineKind::File | DiffLineKind::Hunk) {
        "    ".to_string()
    } else {
        format!("{:>4}", index + 1)
    };

    Line::from(vec![
        Span::styled(format!("{gutter} "), Style::default().fg(Color::DarkGray)),
        Span::styled(line.text.clone(), style),
    ])
}

fn draw_help_overlay(frame: &mut Frame<'_>, area: Rect) {
    let popup = centered_rect(72, 18, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(vec![
            Line::from(Span::styled(
                "GL Help",
                Style::default()
                    .fg(Color::White)
                    .add_modifier(Modifier::BOLD),
            )),
            Line::from(""),
            Line::from("Global: q quit, ? toggle help, R refresh"),
            Line::from("Branches: j/k move, J/K jump stacks, gg/G ends"),
            Line::from("          Ctrl-d/u half-page, Enter open branch"),
            Line::from("          s or 2 open Stack View"),
            Line::from("Detail: Esc back to list, Tab focus diff/list"),
            Line::from("Stack View: j/k move, gg/G ends, Enter open, Esc back"),
            Line::from("Diff scroll: j/k, Ctrl-d/u, gg/G"),
            Line::from("Diff files: J/K jump to next or previous file"),
            Line::from("Search: / start, Enter apply, n/N next or previous"),
            Line::from(""),
            Line::from("Stack groups shown when Graphite CLI (gt) is available."),
        ])
        .block(Block::default().title("Help").borders(Borders::ALL))
        .alignment(Alignment::Left)
        .wrap(Wrap { trim: false }),
        popup,
    );
}

fn centered_rect(width: u16, height: u16, area: Rect) -> Rect {
    let x = area.x + area.width.saturating_sub(width) / 2;
    let y = area.y + area.height.saturating_sub(height) / 2;
    Rect::new(x, y, width.min(area.width), height.min(area.height))
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- BranchEntry ---

    #[test]
    fn header_is_header() {
        let entry = BranchEntry::Header {
            label: "test".into(),
        };
        assert!(entry.is_header());
    }

    #[test]
    fn branch_is_not_header() {
        let entry = BranchEntry::Branch {
            branch_name: "main".into(),
            is_head: false,
            commit_count: 0,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            indent: 0,
            stale: false,
        };
        assert!(!entry.is_header());
    }

    #[test]
    fn branch_name_on_branch() {
        let entry = BranchEntry::Branch {
            branch_name: "feature/auth".into(),
            is_head: false,
            commit_count: 0,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            indent: 0,
            stale: false,
        };
        assert_eq!(entry.branch_name(), "feature/auth");
    }

    #[test]
    fn branch_name_on_header() {
        let entry = BranchEntry::Header {
            label: "my stack".into(),
        };
        assert_eq!(entry.branch_name(), "my stack");
    }

    // --- centered_rect ---

    #[test]
    fn centered_rect_centers_in_area() {
        let area = Rect::new(0, 0, 100, 50);
        let popup = centered_rect(40, 20, area);
        assert_eq!(popup.x, 30);
        assert_eq!(popup.y, 15);
        assert_eq!(popup.width, 40);
        assert_eq!(popup.height, 20);
    }

    #[test]
    fn centered_rect_clamps_to_area() {
        let area = Rect::new(0, 0, 20, 10);
        let popup = centered_rect(40, 20, area);
        assert_eq!(popup.width, 20);
        assert_eq!(popup.height, 10);
    }

    #[test]
    fn centered_rect_with_offset_area() {
        let area = Rect::new(10, 5, 80, 40);
        let popup = centered_rect(40, 20, area);
        assert_eq!(popup.x, 30); // 10 + (80-40)/2
        assert_eq!(popup.y, 15); // 5 + (40-20)/2
    }

    // --- branch_entry_item rendering ---

    #[test]
    fn header_item_contains_label() {
        let entry = BranchEntry::Header {
            label: "auth stack".into(),
        };
        let line = branch_entry_item(&entry);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("auth stack"));
    }

    #[test]
    fn branch_item_shows_head_indicator() {
        let entry = BranchEntry::Branch {
            branch_name: "main".into(),
            is_head: true,
            commit_count: 0,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            indent: 0,
            stale: false,
        };
        let line = branch_entry_item(&entry);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("●"));
    }

    #[test]
    fn branch_item_shows_commit_count() {
        let entry = BranchEntry::Branch {
            branch_name: "feat".into(),
            is_head: false,
            commit_count: 5,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            indent: 0,
            stale: false,
        };
        let line = branch_entry_item(&entry);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("5c"));
    }

    #[test]
    fn branch_item_shows_synced_check() {
        let entry = BranchEntry::Branch {
            branch_name: "feat".into(),
            is_head: false,
            commit_count: 1,
            ahead: 0,
            behind: 0,
            has_upstream: true,
            indent: 0,
            stale: false,
        };
        let line = branch_entry_item(&entry);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("✓"));
    }

    #[test]
    fn branch_item_shows_ahead_behind() {
        let entry = BranchEntry::Branch {
            branch_name: "feat".into(),
            is_head: false,
            commit_count: 1,
            ahead: 3,
            behind: 2,
            has_upstream: true,
            indent: 0,
            stale: false,
        };
        let line = branch_entry_item(&entry);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("↑3"));
        assert!(text.contains("↓2"));
    }

    #[test]
    fn branch_item_shows_stale_indicator() {
        let entry = BranchEntry::Branch {
            branch_name: "feat".into(),
            is_head: false,
            commit_count: 1,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            indent: 1,
            stale: true,
        };
        let line = branch_entry_item(&entry);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("⚠"));
    }

    #[test]
    fn branch_item_shows_indentation() {
        let entry = BranchEntry::Branch {
            branch_name: "feat".into(),
            is_head: false,
            commit_count: 0,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            indent: 2,
            stale: false,
        };
        let line = branch_entry_item(&entry);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("│"));
        assert!(text.contains("├"));
    }
}
