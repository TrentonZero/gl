use crate::{
    config::AppConfig,
    git::{BranchDiff, BranchInfo, DiffLineKind, RepoState},
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

pub fn draw(
    frame: &mut Frame<'_>,
    config: &AppConfig,
    repo: &RepoState,
    selected_index: usize,
    branch_diff: Option<&BranchDiff>,
    highlighted_diff: Option<&[Line<'static>]>,
    diff_scroll: usize,
    show_help: bool,
    focus: FocusedPane,
    search: Option<&str>,
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
        repo,
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
) {
    let hints = if detail {
        match focus {
            FocusedPane::BranchList => "j/k move  Enter open  Esc close  q quit  ? help",
            FocusedPane::Diff => {
                "j/k scroll  J/K files  gg/G ends  Ctrl-d/u page  / search  n/N next  Esc list"
            }
        }
    } else {
        "j/k move  Enter open  R refresh  q quit  ? help"
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
    repo: &RepoState,
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
                repo,
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
        None => draw_branch_list(frame, area, repo, selected_index, true),
    }
}

fn draw_branch_list(
    frame: &mut Frame<'_>,
    area: Rect,
    repo: &RepoState,
    selected_index: usize,
    focused: bool,
) {
    let items: Vec<ListItem<'_>> = repo
        .branches
        .iter()
        .map(branch_list_item)
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

fn branch_list_item(branch: &BranchInfo) -> Line<'static> {
    let mut spans = Vec::new();
    if branch.is_head {
        spans.push(Span::styled("● ", Style::default().fg(Color::Green)));
    } else {
        spans.push(Span::raw("  "));
    }

    spans.push(Span::styled(
        format!("{:<20}", branch.name),
        Style::default().fg(Color::White),
    ));

    if branch.commit_count > 0 {
        spans.push(Span::styled(
            format!(" {:>3}c", branch.commit_count),
            Style::default().fg(Color::Gray),
        ));
    }

    if branch.ahead == 0 && branch.behind == 0 && branch.upstream.is_some() {
        spans.push(Span::styled(" ✓", Style::default().fg(Color::Green)));
    } else {
        if branch.ahead > 0 {
            spans.push(Span::styled(
                format!(" ↑{}", branch.ahead),
                Style::default().fg(Color::Green),
            ));
        }
        if branch.behind > 0 {
            spans.push(Span::styled(
                format!(" ↓{}", branch.behind),
                Style::default().fg(Color::Red),
            ));
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
    let popup = centered_rect(72, 16, area);
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
            Line::from("Branches: j/k move, Enter open branch"),
            Line::from("Detail: Esc back to list, Tab focus diff/list"),
            Line::from("Diff scroll: j/k, Ctrl-d/u, gg/G"),
            Line::from("Diff files: J/K jump to next or previous file"),
            Line::from("Search: / start, Enter apply, n/N next or previous"),
            Line::from(""),
            Line::from("This build implements implementation plan phases 1 and 2."),
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
