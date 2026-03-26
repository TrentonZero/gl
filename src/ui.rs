use crate::{
    config::AppConfig,
    git::{BranchDiff, DiffLineKind, RepoState},
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
    notice: Option<&str>,
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
        draw_help_bar(
            frame,
            areas[2],
            branch_diff.is_some(),
            focus,
            search,
            notice,
        );
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
    notice: Option<&str>,
) {
    let line = help_bar_line(detail, focus, search, notice);
    frame.render_widget(Paragraph::new(line), area);
}

fn help_bar_line(
    detail: bool,
    focus: FocusedPane,
    search: Option<&str>,
    notice: Option<&str>,
) -> Line<'static> {
    let hints = if detail {
        match focus {
            FocusedPane::BranchList => {
                "j/k move  J/K stacks  Enter open  Esc close  q quit  ? help"
            }
            FocusedPane::Diff => {
                "j/k scroll  J/K files  gg/G ends  Ctrl-d/u page  / search  n/N next  Esc list"
            }
        }
    } else {
        "j/k move  J/K stacks  Enter open  R refresh  q quit  ? help"
    };

    let mut line = Line::from(Span::styled(hints, Style::default().fg(Color::Gray)));
    if let Some(search) = search {
        line.spans.push(Span::raw("  "));
        line.spans.push(Span::styled(
            format!("search: {search}"),
            Style::default().fg(Color::Yellow),
        ));
    }

    if let Some(notice) = notice {
        line.spans.push(Span::raw("  "));
        line.spans.push(Span::styled(
            notice.to_string(),
            Style::default().fg(Color::Yellow),
        ));
    }

    line
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
) {
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
        None => draw_branch_list(frame, area, display_entries, selected_index, true),
    }
}

fn draw_branch_list(
    frame: &mut Frame<'_>,
    area: Rect,
    display_entries: &[BranchEntry],
    selected_index: usize,
    focused: bool,
) {
    let content_width = area.width.saturating_sub(3) as usize;
    let items: Vec<ListItem<'_>> = display_entries
        .iter()
        .map(|entry| branch_entry_item(entry, content_width))
        .map(ListItem::new)
        .collect();

    let mut state = ListState::default();
    state.select(Some(selected_index));

    let block = Block::default()
        .title("Branches")
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
}

fn branch_entry_item(entry: &BranchEntry, available_width: usize) -> Line<'static> {
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

            let prefix_width = indent * 2 + 2;
            let suffix_width = branch_suffix_width(*commit_count, *ahead, *behind, *has_upstream);
            let name_width = available_width
                .saturating_sub(prefix_width + suffix_width)
                .max(1);
            let display_name = format_branch_name(branch_name, name_width);
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

fn branch_suffix_width(
    commit_count: usize,
    ahead: usize,
    behind: usize,
    has_upstream: bool,
) -> usize {
    let mut width = 0;

    if commit_count > 0 {
        width += 2 + commit_count.to_string().len() + 1;
    }

    if ahead == 0 && behind == 0 && has_upstream {
        width += 2;
    } else {
        if ahead > 0 {
            width += 2 + ahead.to_string().len();
        }
        if behind > 0 {
            width += 2 + behind.to_string().len();
        }
    }

    width
}

fn format_branch_name(branch_name: &str, width: usize) -> String {
    let name_len = branch_name.chars().count();
    if name_len <= width {
        return format!("{branch_name:<width$}");
    }

    if width <= 1 {
        return branch_name.chars().take(width).collect();
    }

    let mut truncated: String = branch_name.chars().take(width - 1).collect();
    truncated.push('~');
    truncated
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

    let visible_height = area.height.saturating_sub(2) as usize;
    let lines = match highlighted_diff {
        Some(lines) => visible_highlighted_lines(lines, diff_scroll, visible_height),
        None => visible_plain_diff_lines(diff, diff_scroll, visible_height),
    };

    frame.render_widget(
        Paragraph::new(lines)
            .block(block)
            .scroll((0, 0))
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn visible_highlighted_lines(
    lines: &[Line<'static>],
    diff_scroll: usize,
    visible_height: usize,
) -> Vec<Line<'static>> {
    visible_slice_bounds(lines.len(), diff_scroll, visible_height)
        .map(|(start, end)| lines[start..end].to_vec())
        .unwrap_or_default()
}

fn visible_plain_diff_lines(
    diff: &BranchDiff,
    diff_scroll: usize,
    visible_height: usize,
) -> Vec<Line<'static>> {
    let Some((start, end)) = visible_slice_bounds(diff.lines.len(), diff_scroll, visible_height)
    else {
        return Vec::new();
    };

    diff.lines
        .iter()
        .enumerate()
        .skip(start)
        .take(end - start)
        .map(|(idx, line)| render_diff_line(idx, line))
        .collect()
}

fn visible_slice_bounds(
    total_lines: usize,
    diff_scroll: usize,
    visible_height: usize,
) -> Option<(usize, usize)> {
    if total_lines == 0 || visible_height == 0 {
        return None;
    }

    let start = diff_scroll.min(total_lines.saturating_sub(1));
    let end = (start + visible_height).min(total_lines);
    Some((start, end))
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
            Line::from("Detail: Esc back to list, Tab focus diff/list"),
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
        let line = branch_entry_item(&entry, 40);
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
        let line = branch_entry_item(&entry, 40);
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
        let line = branch_entry_item(&entry, 40);
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
        let line = branch_entry_item(&entry, 40);
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
        let line = branch_entry_item(&entry, 40);
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
        let line = branch_entry_item(&entry, 40);
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
        let line = branch_entry_item(&entry, 40);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("│"));
        assert!(text.contains("├"));
    }

    #[test]
    fn branch_item_uses_available_width_for_deeply_indented_name() {
        let entry = BranchEntry::Branch {
            branch_name: "feature/payments-api-long".into(),
            is_head: false,
            commit_count: 1,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            indent: 3,
            stale: false,
        };
        let line = branch_entry_item(&entry, 40);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("feature/payments-api-long"));
    }

    #[test]
    fn branch_item_truncates_only_when_pane_is_too_narrow() {
        let entry = BranchEntry::Branch {
            branch_name: "feature/payments-api-long".into(),
            is_head: false,
            commit_count: 12,
            ahead: 3,
            behind: 0,
            has_upstream: true,
            indent: 3,
            stale: false,
        };
        let line = branch_entry_item(&entry, 24);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("~"));
    }

    #[test]
    fn help_bar_shows_non_blocking_notice() {
        let line = help_bar_line(
            false,
            FocusedPane::BranchList,
            None,
            Some("Graphite unavailable; showing inferred local branch relationships."),
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains("R refresh"));
        assert!(text.contains("Graphite unavailable"));
    }
}
