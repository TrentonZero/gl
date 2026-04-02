use crate::{
    config::{AppConfig, ColorScheme, DiffViewMode, KeyBindings},
    git::{BranchDiff, DetailKind, DiffLineKind, GraphCommit, RepoState, WorktreeInfo},
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackView {
    pub title: String,
    pub selected_branch: String,
    pub parent_branch: Option<String>,
    pub child_branch: Option<String>,
    pub base_ref: Option<String>,
    pub stale: bool,
    pub branches: Vec<StackViewBranch>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StackViewBranch {
    pub name: String,
    pub is_selected: bool,
    pub is_head: bool,
    pub commit_count: usize,
    pub ahead: usize,
    pub behind: usize,
    pub has_upstream: bool,
    pub stale: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GraphView<'a> {
    pub commits: &'a [GraphCommit],
    pub selected_index: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WorktreeView<'a> {
    pub worktrees: &'a [WorktreeInfo],
    pub selected_index: usize,
}

#[derive(Debug, Clone)]
pub enum BranchEntry {
    Header {
        label: String,
        expanded: Option<bool>,
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
        worktree_label: Option<String>,
    },
}

impl BranchEntry {
    pub fn is_header(&self) -> bool {
        matches!(self, BranchEntry::Header { .. })
    }

    pub fn branch_name(&self) -> &str {
        match self {
            BranchEntry::Branch { branch_name, .. } => branch_name,
            BranchEntry::Header { label, .. } => label,
        }
    }
}

pub struct DrawState<'a> {
    pub config: &'a AppConfig,
    pub repo: &'a RepoState,
    pub display_entries: &'a [BranchEntry],
    pub selected_index: usize,
    pub stack_view: Option<&'a StackView>,
    pub graph_view: Option<GraphView<'a>>,
    pub worktree_view: Option<WorktreeView<'a>>,
    pub detail_kind: Option<DetailKind>,
    pub branch_diff: Option<&'a BranchDiff>,
    pub highlighted_diff: Option<&'a [Line<'static>]>,
    pub diff_scroll: usize,
    pub diff_view: DiffViewMode,
    pub show_help: bool,
    pub focus: FocusedPane,
    pub commit_list_overlay: Option<Vec<String>>,
    pub commit_list_selected: Option<usize>,
    pub info_overlay: Option<&'a [String]>,
    pub search: Option<&'a str>,
    pub notice: Option<&'a str>,
    pub command_input: Option<&'a str>,
}

struct BodyState<'a> {
    display_entries: &'a [BranchEntry],
    selected_index: usize,
    stack_view: Option<&'a StackView>,
    graph_view: Option<GraphView<'a>>,
    worktree_view: Option<WorktreeView<'a>>,
    diff: Option<DiffState<'a>>,
    focus: FocusedPane,
}

struct HelpBarState<'a> {
    keybindings: &'a KeyBindings,
    stack_view_open: bool,
    detail_kind: Option<DetailKind>,
    focus: FocusedPane,
    search: Option<&'a str>,
    notice: Option<&'a str>,
}

struct DiffState<'a> {
    detail_kind: Option<DetailKind>,
    diff: &'a BranchDiff,
    highlighted_diff: Option<&'a [Line<'static>]>,
    diff_scroll: usize,
    diff_view: DiffViewMode,
    commit_list_overlay: Option<&'a [String]>,
    commit_list_selected: Option<usize>,
    info_overlay: Option<&'a [String]>,
}

pub fn draw(frame: &mut Frame<'_>, state: DrawState<'_>) {
    let frame_area = frame.size();
    let areas = if state.config.chrome {
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

    if state.config.chrome {
        draw_status_bar(frame, areas[0], state.config, state.repo, state.detail_kind);
        draw_help_bar(
            frame,
            areas[2],
            &HelpBarState {
                keybindings: &state.config.keybindings,
                stack_view_open: state.stack_view.is_some(),
                detail_kind: state.detail_kind,
                focus: state.focus,
                search: state.search,
                notice: state.notice,
            },
        );
    }

    let body_state = BodyState {
        display_entries: state.display_entries,
        selected_index: state.selected_index,
        stack_view: state.stack_view,
        graph_view: state.graph_view,
        worktree_view: state.worktree_view,
        diff: state.branch_diff.map(|diff| DiffState {
            detail_kind: state.detail_kind,
            diff,
            highlighted_diff: state.highlighted_diff,
            diff_scroll: state.diff_scroll,
            diff_view: state.diff_view,
            commit_list_overlay: state.commit_list_overlay.as_deref(),
            commit_list_selected: state.commit_list_selected,
            info_overlay: state.info_overlay,
        }),
        focus: state.focus,
    };
    draw_body(frame, areas[1], &body_state);

    if state.show_help {
        draw_help_overlay(frame, frame_area, state.config);
    }

    if let Some(command_input) = state.command_input {
        draw_command_overlay(frame, frame_area, command_input);
    }
}

fn draw_status_bar(
    frame: &mut Frame<'_>,
    area: Rect,
    config: &AppConfig,
    repo: &RepoState,
    detail_kind: Option<DetailKind>,
) {
    let title = match detail_kind {
        Some(DetailKind::BranchDiff) => "GL - Green Ledger · Detail",
        Some(DetailKind::Status) => "GL - Green Ledger · Status",
        None => "GL - Green Ledger · Branches",
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
            .style(Style::default().bg(accent_color(config.color_scheme)))
            .alignment(Alignment::Left),
        area,
    );
}

fn draw_help_bar(frame: &mut Frame<'_>, area: Rect, state: &HelpBarState<'_>) {
    let line = help_bar_line(
        state.keybindings,
        state.stack_view_open,
        state.detail_kind,
        state.focus,
        state.search,
        state.notice,
    );
    frame.render_widget(Paragraph::new(line), area);
}

fn help_bar_line(
    keybindings: &KeyBindings,
    stack_view_open: bool,
    detail_kind: Option<DetailKind>,
    focus: FocusedPane,
    search: Option<&str>,
    notice: Option<&str>,
) -> Line<'static> {
    let hints = if detail_kind.is_some() {
        match (detail_kind, focus) {
            (_, FocusedPane::BranchList) => branch_list_detail_help(keybindings),
            (Some(DetailKind::BranchDiff), FocusedPane::Diff) => branch_diff_help(),
            (Some(DetailKind::Status), FocusedPane::Diff) => status_diff_help(),
            _ => String::new(),
        }
    } else if stack_view_open {
        stack_view_help(keybindings)
    } else {
        branch_list_help(keybindings)
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

fn branch_list_help(keybindings: &KeyBindings) -> String {
    format!(
        "j/k move  J/K stacks  h/l fold  Enter open  {} status  {} stack  {} refresh  {} quit  {} help",
        keybindings.status_view,
        keybindings.stack_view,
        keybindings.refresh,
        keybindings.quit,
        keybindings.help,
    )
}

fn branch_list_detail_help(keybindings: &KeyBindings) -> String {
    format!(
        "j/k move  J/K stacks  h/l fold  Enter open  {} status  Esc close  {} quit  {} help",
        keybindings.status_view, keybindings.quit, keybindings.help,
    )
}

fn stack_view_help(keybindings: &KeyBindings) -> String {
    format!(
        "j/k move  J/K stacks  h/l fold  Enter open diff  {} stack  Esc close  {} refresh  {} quit",
        keybindings.stack_view, keybindings.refresh, keybindings.quit,
    )
}

fn branch_diff_help() -> String {
    "j/k scroll  J/K files  Tab commits  v view  w whitespace  Enter open commit  Backspace branch  i info  / search  Esc list".to_string()
}

fn status_diff_help() -> String {
    "j/k scroll  J/K files  gg/G ends  Ctrl-d/u page  v view  w whitespace  / search  n/N next  Esc list".to_string()
}

fn help_overlay_lines(keybindings: &KeyBindings) -> Vec<Line<'static>> {
    vec![
        Line::from(Span::styled(
            "GL Help",
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(format!(
            "Global: {} quit, {} toggle help, {} refresh, {} graph, 3/w worktrees, {} command",
            keybindings.quit,
            keybindings.help,
            keybindings.refresh,
            keybindings.graph_view,
            keybindings.command,
        )),
        Line::from(format!(
            "Branches: j/k move, J/K jump stacks, h/l fold or unfold, {} stack",
            keybindings.stack_view,
        )),
        Line::from(format!(
            "          Ctrl-d/u half-page, Enter open branch, {} status",
            keybindings.status_view,
        )),
        Line::from("Stack view: Esc back to list, Enter open selected diff"),
        Line::from("Branch detail: Tab commits, Enter commit diff, Backspace branch diff"),
        Line::from("Branch detail: i info overlay, any key dismisses"),
        Line::from("Status detail: Tab focus diff/list, Esc back to list"),
        Line::from("Diff scroll: j/k, Ctrl-d/u, gg/G, J/K file jumps, v view, w whitespace"),
        Line::from("Search: / start, Enter apply, n/N next or previous"),
        Line::from(""),
        Line::from("Stack groups shown when Graphite CLI (gt) is available."),
    ]
}

fn draw_body(frame: &mut Frame<'_>, area: Rect, state: &BodyState<'_>) {
    match state.diff.as_ref() {
        Some(diff) => {
            let panes = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Length(34), Constraint::Min(1)])
                .split(area);
            draw_branch_list(
                frame,
                panes[0],
                state.display_entries,
                state.selected_index,
                state.focus == FocusedPane::BranchList,
            );
            draw_diff(frame, panes[1], diff, state.focus == FocusedPane::Diff);
        }
        None => {
            if let Some(stack_view) = state.stack_view {
                let panes = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(34), Constraint::Min(1)])
                    .split(area);
                draw_branch_list(
                    frame,
                    panes[0],
                    state.display_entries,
                    state.selected_index,
                    true,
                );
                draw_stack_view(frame, panes[1], stack_view);
            } else if let Some(graph_view) = state.graph_view {
                let panes = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(34), Constraint::Min(1)])
                    .split(area);
                draw_branch_list(
                    frame,
                    panes[0],
                    state.display_entries,
                    state.selected_index,
                    state.focus == FocusedPane::BranchList,
                );
                draw_graph_view(
                    frame,
                    panes[1],
                    graph_view,
                    state.focus == FocusedPane::Diff,
                );
            } else if let Some(worktree_view) = state.worktree_view {
                let panes = Layout::default()
                    .direction(Direction::Horizontal)
                    .constraints([Constraint::Length(34), Constraint::Min(1)])
                    .split(area);
                draw_branch_list(
                    frame,
                    panes[0],
                    state.display_entries,
                    state.selected_index,
                    state.focus == FocusedPane::BranchList,
                );
                draw_worktree_view(
                    frame,
                    panes[1],
                    worktree_view,
                    state.focus == FocusedPane::Diff,
                );
            } else {
                draw_branch_list(
                    frame,
                    area,
                    state.display_entries,
                    state.selected_index,
                    true,
                );
            }
        }
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
        BranchEntry::Header { label, expanded } => Line::from(vec![Span::styled(
            format!(
                " {} {label}",
                match expanded {
                    Some(true) => "▾",
                    Some(false) => "▸",
                    None => " ",
                }
            ),
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
            worktree_label,
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

            if let Some(worktree_label) = worktree_label {
                spans.push(Span::styled(
                    format!("  [{worktree_label}]"),
                    Style::default().fg(Color::Cyan),
                ));
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

fn draw_diff(frame: &mut Frame<'_>, area: Rect, state: &DiffState<'_>, focused: bool) {
    let title = state
        .diff
        .title
        .clone()
        .unwrap_or_else(|| match state.detail_kind {
            Some(DetailKind::Status) => format!("{} working tree", state.diff.branch_name),
            _ => match &state.diff.base_ref {
                Some(base_ref) => format!("{} vs {}", state.diff.branch_name, base_ref),
                None => state.diff.branch_name.clone(),
            },
        });
    let title = if state.diff.ignore_whitespace {
        format!("{title} [w]")
    } else {
        title
    };

    let block = Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(if focused {
            Style::default().fg(Color::Blue)
        } else {
            Style::default().fg(Color::DarkGray)
        });

    match state.diff_view {
        DiffViewMode::Unified => {
            let visible_height = area.height.saturating_sub(2) as usize;
            let lines = match state.highlighted_diff {
                Some(lines) => visible_highlighted_lines(lines, state.diff_scroll, visible_height),
                None => visible_plain_diff_lines(state.diff, state.diff_scroll, visible_height),
            };

            frame.render_widget(
                Paragraph::new(lines)
                    .block(block)
                    .scroll((0, 0))
                    .wrap(Wrap { trim: false }),
                area,
            );
        }
        DiffViewMode::SideBySide => {
            frame.render_widget(block, area);
            let inner = Rect::new(
                area.x.saturating_add(1),
                area.y.saturating_add(1),
                area.width.saturating_sub(2),
                area.height.saturating_sub(2),
            );
            draw_side_by_side_diff(frame, inner, state.diff, state.diff_scroll);
        }
    }

    if let Some(lines) = state.commit_list_overlay {
        draw_commit_list_overlay(frame, area, lines, state.commit_list_selected.unwrap_or(0));
    }

    if let Some(lines) = state.info_overlay {
        draw_info_overlay(frame, area, lines);
    }
}

fn draw_commit_list_overlay(
    frame: &mut Frame<'_>,
    area: Rect,
    items: &[String],
    selected_index: usize,
) {
    let overlay_height = (items.len() as u16 + 2)
        .min(area.height.saturating_sub(2))
        .max(3);
    let overlay_width = area.width.saturating_sub(4).clamp(24, 72);
    let overlay = Rect::new(area.x + 2, area.y + 1, overlay_width, overlay_height);
    frame.render_widget(Clear, overlay);

    let list_items: Vec<ListItem<'_>> = items
        .iter()
        .map(|item| ListItem::new(Line::from(item.clone())))
        .collect();
    let mut state = ListState::default();
    state.select(Some(selected_index.min(items.len().saturating_sub(1))));

    let list = List::new(list_items)
        .block(Block::default().title("Commits").borders(Borders::ALL))
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(51, 70, 124))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ");
    frame.render_stateful_widget(list, overlay, &mut state);
}

fn draw_info_overlay(frame: &mut Frame<'_>, area: Rect, lines: &[String]) {
    let popup = centered_rect(72, (lines.len() as u16 + 2).min(area.height), area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(
            lines
                .iter()
                .map(|line| Line::from(line.clone()))
                .collect::<Vec<_>>(),
        )
        .block(Block::default().title("Branch Info").borders(Borders::ALL))
        .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_stack_view(frame: &mut Frame<'_>, area: Rect, stack_view: &StackView) {
    let block = Block::default()
        .title(stack_view.title.clone())
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::Blue));

    frame.render_widget(
        Paragraph::new(stack_view_lines(stack_view))
            .block(block)
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_graph_view(frame: &mut Frame<'_>, area: Rect, graph_view: GraphView<'_>, focused: bool) {
    let items: Vec<ListItem<'_>> = graph_view
        .commits
        .iter()
        .map(graph_commit_line)
        .map(ListItem::new)
        .collect();

    let mut state = ListState::default();
    state.select(Some(
        graph_view
            .selected_index
            .min(graph_view.commits.len().saturating_sub(1)),
    ));

    let block = Block::default()
        .title("Graph")
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

fn draw_worktree_view(
    frame: &mut Frame<'_>,
    area: Rect,
    worktree_view: WorktreeView<'_>,
    focused: bool,
) {
    let items: Vec<ListItem<'_>> = worktree_view
        .worktrees
        .iter()
        .map(worktree_line)
        .map(ListItem::new)
        .collect();
    let mut state = ListState::default();
    state.select(Some(
        worktree_view
            .selected_index
            .min(worktree_view.worktrees.len().saturating_sub(1)),
    ));
    let list = List::new(items)
        .block(
            Block::default()
                .title("Worktrees")
                .borders(Borders::ALL)
                .border_style(if focused {
                    Style::default().fg(Color::Blue)
                } else {
                    Style::default().fg(Color::DarkGray)
                }),
        )
        .highlight_style(
            Style::default()
                .bg(Color::Rgb(51, 70, 124))
                .add_modifier(Modifier::BOLD),
        )
        .highlight_symbol(" ");
    frame.render_stateful_widget(list, area, &mut state);
}

fn worktree_line(worktree: &WorktreeInfo) -> Line<'static> {
    let status = if worktree.is_dirty { "dirty" } else { "clean" };
    let active = if worktree.is_active { "* " } else { "  " };
    let branch = worktree.branch.as_deref().unwrap_or("(detached)");
    let suffix = if worktree.is_bare { "bare" } else { status };
    Line::from(format!(
        "{active}{branch:<16} {suffix:<5} {}",
        worktree.path.display()
    ))
}

fn graph_commit_line(commit: &GraphCommit) -> Line<'static> {
    let labels = if commit.branch_labels.is_empty() {
        String::new()
    } else {
        format!("  [{}]", commit.branch_labels.join(", "))
    };
    Line::from(vec![
        Span::styled(
            format!(" {} ", commit.graph),
            Style::default().fg(Color::Cyan),
        ),
        Span::styled(
            format!("{:<8}", commit.short_oid),
            Style::default().fg(Color::Yellow),
        ),
        Span::styled(commit.subject.clone(), Style::default().fg(Color::White)),
        Span::styled(labels, Style::default().fg(Color::Green)),
    ])
}

fn stack_view_lines(stack_view: &StackView) -> Vec<Line<'static>> {
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                "Selected: ",
                Style::default()
                    .fg(Color::Gray)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                stack_view.selected_branch.clone(),
                Style::default().fg(Color::White),
            ),
        ]),
        Line::from(""),
        kv_line(
            "Parent",
            stack_view.parent_branch.as_deref().unwrap_or("none"),
        ),
        kv_line(
            "Child",
            stack_view.child_branch.as_deref().unwrap_or("none"),
        ),
        kv_line(
            "Diff base",
            stack_view.base_ref.as_deref().unwrap_or("none"),
        ),
        kv_line("Stale", if stack_view.stale { "yes" } else { "no" }),
        Line::from(""),
        Line::from(Span::styled(
            "Stack Branches",
            Style::default()
                .fg(Color::Magenta)
                .add_modifier(Modifier::BOLD),
        )),
    ];

    for branch in &stack_view.branches {
        lines.push(stack_branch_line(branch));
    }

    lines
}

fn kv_line(label: &str, value: &str) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            format!("{label:<9}"),
            Style::default()
                .fg(Color::Gray)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(value.to_string(), Style::default().fg(Color::White)),
    ])
}

fn stack_branch_line(branch: &StackViewBranch) -> Line<'static> {
    let mut spans = Vec::new();
    if branch.is_selected {
        spans.push(Span::styled("▶ ", Style::default().fg(Color::Blue)));
    } else {
        spans.push(Span::raw("  "));
    }

    if branch.stale {
        spans.push(Span::styled("⚠ ", Style::default().fg(Color::Yellow)));
    }

    if branch.is_head {
        spans.push(Span::styled("● ", Style::default().fg(Color::Green)));
    } else if !branch.stale {
        spans.push(Span::raw("  "));
    }

    spans.push(Span::styled(
        branch.name.clone(),
        if branch.is_selected {
            Style::default()
                .fg(Color::White)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(Color::White)
        },
    ));

    if branch.commit_count > 0 {
        spans.push(Span::styled(
            format!("  {}c", branch.commit_count),
            Style::default().fg(Color::Gray),
        ));
    }
    if branch.ahead == 0 && branch.behind == 0 && branch.has_upstream {
        spans.push(Span::styled("  ✓", Style::default().fg(Color::Green)));
    } else {
        if branch.ahead > 0 {
            spans.push(Span::styled(
                format!("  ↑{}", branch.ahead),
                Style::default().fg(Color::Green),
            ));
        }
        if branch.behind > 0 {
            spans.push(Span::styled(
                format!("  ↓{}", branch.behind),
                Style::default().fg(Color::Red),
            ));
        }
    }

    Line::from(spans)
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

fn draw_side_by_side_diff(
    frame: &mut Frame<'_>,
    area: Rect,
    diff: &BranchDiff,
    diff_scroll: usize,
) {
    if area.width == 0 || area.height == 0 {
        return;
    }

    let rows = side_by_side_rows(diff);
    let Some((start, end)) = visible_slice_bounds(rows.len(), diff_scroll, area.height as usize)
    else {
        return;
    };
    let visible_rows = &rows[start..end];
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    let left_width = columns[0].width.saturating_sub(5) as usize;
    let right_width = columns[1].width.saturating_sub(5) as usize;

    let left_lines: Vec<_> = visible_rows
        .iter()
        .map(|row| side_by_side_line(&row.left, left_width))
        .collect();
    let right_lines: Vec<_> = visible_rows
        .iter()
        .map(|row| side_by_side_line(&row.right, right_width))
        .collect();

    frame.render_widget(
        Paragraph::new(left_lines).wrap(Wrap { trim: false }),
        columns[0],
    );
    frame.render_widget(
        Paragraph::new(right_lines).wrap(Wrap { trim: false }),
        columns[1],
    );
}

#[derive(Clone, Copy)]
struct SideBySideCell<'a> {
    gutter: Option<usize>,
    text: &'a str,
    kind: DiffLineKind,
}

#[derive(Clone, Copy)]
struct SideBySideRow<'a> {
    left: SideBySideCell<'a>,
    right: SideBySideCell<'a>,
}

fn side_by_side_rows(diff: &BranchDiff) -> Vec<SideBySideRow<'_>> {
    let mut rows = Vec::new();
    let mut index = 0usize;

    while index < diff.lines.len() {
        let line = &diff.lines[index];
        match line.kind {
            DiffLineKind::Del => {
                let mut dels = Vec::new();
                while index < diff.lines.len() && diff.lines[index].kind == DiffLineKind::Del {
                    dels.push((index + 1, diff.lines[index].text.as_str()));
                    index += 1;
                }

                let mut adds = Vec::new();
                while index < diff.lines.len() && diff.lines[index].kind == DiffLineKind::Add {
                    adds.push((index + 1, diff.lines[index].text.as_str()));
                    index += 1;
                }

                for pair_index in 0..dels.len().max(adds.len()) {
                    let left = dels.get(pair_index).copied();
                    let right = adds.get(pair_index).copied();
                    rows.push(SideBySideRow {
                        left: SideBySideCell {
                            gutter: left.map(|(gutter, _)| gutter),
                            text: left.map(|(_, text)| text).unwrap_or(""),
                            kind: DiffLineKind::Del,
                        },
                        right: SideBySideCell {
                            gutter: right.map(|(gutter, _)| gutter),
                            text: right.map(|(_, text)| text).unwrap_or(""),
                            kind: DiffLineKind::Add,
                        },
                    });
                }
            }
            DiffLineKind::Add => {
                rows.push(SideBySideRow {
                    left: SideBySideCell {
                        gutter: None,
                        text: "",
                        kind: DiffLineKind::Context,
                    },
                    right: SideBySideCell {
                        gutter: Some(index + 1),
                        text: line.text.as_str(),
                        kind: DiffLineKind::Add,
                    },
                });
                index += 1;
            }
            _ => {
                rows.push(SideBySideRow {
                    left: SideBySideCell {
                        gutter: Some(index + 1),
                        text: line.text.as_str(),
                        kind: line.kind,
                    },
                    right: SideBySideCell {
                        gutter: Some(index + 1),
                        text: line.text.as_str(),
                        kind: line.kind,
                    },
                });
                index += 1;
            }
        }
    }

    rows
}

fn side_by_side_line(cell: &SideBySideCell<'_>, width: usize) -> Line<'static> {
    let style = match cell.kind {
        DiffLineKind::File => Style::default()
            .fg(Color::Magenta)
            .add_modifier(Modifier::BOLD),
        DiffLineKind::Hunk => Style::default().fg(Color::Cyan),
        DiffLineKind::Context => Style::default().fg(Color::Gray),
        DiffLineKind::Add => Style::default().fg(Color::Green).bg(Color::Rgb(31, 53, 31)),
        DiffLineKind::Del => Style::default().fg(Color::Red).bg(Color::Rgb(59, 22, 22)),
        DiffLineKind::Meta => Style::default().fg(Color::Yellow),
    };
    let gutter = cell
        .gutter
        .map(|value| format!("{value:>4} "))
        .unwrap_or_else(|| "     ".to_string());

    Line::from(vec![
        Span::styled(gutter, Style::default().fg(Color::DarkGray)),
        Span::styled(truncate_to_width(cell.text, width), style),
    ])
}

fn truncate_to_width(text: &str, width: usize) -> String {
    if width == 0 {
        return String::new();
    }

    let chars: Vec<_> = text.chars().collect();
    if chars.len() <= width {
        return chars.into_iter().collect();
    }

    if width == 1 {
        return "~".to_string();
    }

    let mut truncated: String = chars.into_iter().take(width - 1).collect();
    truncated.push('~');
    truncated
}

fn draw_help_overlay(frame: &mut Frame<'_>, area: Rect, config: &AppConfig) {
    let popup = centered_rect(72, 18, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(help_overlay_lines(&config.keybindings))
            .block(Block::default().title("Help").borders(Borders::ALL))
            .alignment(Alignment::Left)
            .wrap(Wrap { trim: false }),
        popup,
    );
}

fn draw_command_overlay(frame: &mut Frame<'_>, area: Rect, command_input: &str) {
    let popup = centered_rect(72, 3, area);
    frame.render_widget(Clear, popup);
    frame.render_widget(
        Paragraph::new(Line::from(format!(":{}", command_input)))
            .block(Block::default().title("Command").borders(Borders::ALL)),
        popup,
    );
}

fn accent_color(color_scheme: ColorScheme) -> Color {
    match color_scheme {
        ColorScheme::Ocean => Color::Blue,
        ColorScheme::Forest => Color::Green,
        ColorScheme::Amber => Color::Yellow,
        ColorScheme::Violet => Color::Magenta,
        ColorScheme::Rose => Color::Red,
        ColorScheme::Teal => Color::Cyan,
    }
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
            expanded: Some(true),
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
            worktree_label: None,
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
            worktree_label: None,
        };
        assert_eq!(entry.branch_name(), "feature/auth");
    }

    #[test]
    fn branch_name_on_header() {
        let entry = BranchEntry::Header {
            label: "my stack".into(),
            expanded: Some(false),
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

    #[test]
    fn accent_color_matches_configured_scheme() {
        assert_eq!(accent_color(ColorScheme::Ocean), Color::Blue);
        assert_eq!(accent_color(ColorScheme::Forest), Color::Green);
        assert_eq!(accent_color(ColorScheme::Amber), Color::Yellow);
        assert_eq!(accent_color(ColorScheme::Violet), Color::Magenta);
        assert_eq!(accent_color(ColorScheme::Rose), Color::Red);
        assert_eq!(accent_color(ColorScheme::Teal), Color::Cyan);
    }

    // --- branch_entry_item rendering ---

    #[test]
    fn header_item_contains_label() {
        let entry = BranchEntry::Header {
            label: "auth stack".into(),
            expanded: Some(true),
        };
        let line = branch_entry_item(&entry, 40);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("auth stack"));
    }

    #[test]
    fn collapsed_header_item_shows_fold_marker() {
        let entry = BranchEntry::Header {
            label: "auth stack".into(),
            expanded: Some(false),
        };
        let line = branch_entry_item(&entry, 40);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("▸"));
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
            worktree_label: None,
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
            worktree_label: None,
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
            worktree_label: None,
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
            worktree_label: None,
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
            worktree_label: None,
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
            worktree_label: None,
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
            worktree_label: None,
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
            worktree_label: None,
        };
        let line = branch_entry_item(&entry, 24);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("~"));
    }

    #[test]
    fn branch_item_shows_worktree_label() {
        let entry = BranchEntry::Branch {
            branch_name: "feat".into(),
            is_head: false,
            commit_count: 0,
            ahead: 0,
            behind: 0,
            has_upstream: false,
            indent: 0,
            stale: false,
            worktree_label: Some("wt-feature".into()),
        };
        let line = branch_entry_item(&entry, 60);
        let text: String = line.spans.iter().map(|s| s.content.as_ref()).collect();
        assert!(text.contains("[wt-feature]"));
    }

    #[test]
    fn help_bar_shows_non_blocking_notice() {
        let line = help_bar_line(
            &AppConfig::default().keybindings,
            false,
            None,
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

    #[test]
    fn help_bar_shows_stack_view_hints() {
        let line = help_bar_line(
            &AppConfig::default().keybindings,
            true,
            None,
            FocusedPane::BranchList,
            None,
            None,
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains("Enter open diff"));
        assert!(text.contains("Esc close"));
        assert!(text.contains("h/l fold"));
    }

    #[test]
    fn help_bar_shows_status_shortcut_in_branch_list() {
        let line = help_bar_line(
            &AppConfig::default().keybindings,
            false,
            None,
            FocusedPane::BranchList,
            None,
            None,
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains("S status"));
        assert!(text.contains("h/l fold"));
    }

    #[test]
    fn help_bar_shows_diff_view_and_whitespace_hints() {
        let line = help_bar_line(
            &AppConfig::default().keybindings,
            false,
            Some(DetailKind::BranchDiff),
            FocusedPane::Diff,
            None,
            None,
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains("v view"));
        assert!(text.contains("w whitespace"));
    }

    #[test]
    fn help_bar_uses_remapped_keybindings() {
        let keybindings = KeyBindings {
            quit: 'x',
            help: 'H',
            refresh: 'r',
            command: ';',
            stack_view: 't',
            status_view: 'z',
            graph_view: '9',
        };
        let line = help_bar_line(
            &keybindings,
            false,
            None,
            FocusedPane::BranchList,
            None,
            None,
        );
        let text: String = line
            .spans
            .iter()
            .map(|span| span.content.as_ref())
            .collect();
        assert!(text.contains("z status"));
        assert!(text.contains("t stack"));
        assert!(text.contains("r refresh"));
        assert!(text.contains("x quit"));
        assert!(text.contains("H help"));
    }

    #[test]
    fn help_overlay_uses_remapped_keybindings() {
        let keybindings = KeyBindings {
            quit: 'x',
            help: 'H',
            refresh: 'r',
            command: ';',
            stack_view: 't',
            status_view: 'z',
            graph_view: '9',
        };
        let text: String = help_overlay_lines(&keybindings)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect();
        assert!(text.contains("x quit"));
        assert!(text.contains("H toggle help"));
        assert!(text.contains("r refresh"));
        assert!(text.contains("9 graph"));
        assert!(text.contains("; command"));
        assert!(text.contains("t stack"));
        assert!(text.contains("z status"));
    }

    #[test]
    fn stack_view_lines_render_trunk_parent_for_bottom_branch() {
        let stack_view = StackView {
            title: "auth stack".into(),
            selected_branch: "auth-base".into(),
            parent_branch: Some("main".into()),
            child_branch: Some("auth-ui".into()),
            base_ref: Some("main".into()),
            stale: false,
            branches: vec![],
        };

        let text: String = stack_view_lines(&stack_view)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect();
        assert!(text.contains("Parent   main"));
    }

    #[test]
    fn stack_view_lines_include_selected_relationships() {
        let stack_view = StackView {
            title: "auth stack".into(),
            selected_branch: "auth-ui".into(),
            parent_branch: Some("auth-base".into()),
            child_branch: None,
            base_ref: Some("auth-base".into()),
            stale: true,
            branches: vec![
                StackViewBranch {
                    name: "auth-base".into(),
                    is_selected: false,
                    is_head: false,
                    commit_count: 2,
                    ahead: 0,
                    behind: 0,
                    has_upstream: true,
                    stale: false,
                },
                StackViewBranch {
                    name: "auth-ui".into(),
                    is_selected: true,
                    is_head: true,
                    commit_count: 3,
                    ahead: 1,
                    behind: 0,
                    has_upstream: true,
                    stale: true,
                },
            ],
        };

        let text: String = stack_view_lines(&stack_view)
            .into_iter()
            .flat_map(|line| line.spans.into_iter())
            .map(|span| span.content.into_owned())
            .collect();
        assert!(text.contains("Selected: auth-ui"));
        assert!(text.contains("Parent   auth-base"));
        assert!(text.contains("Stale    yes"));
        assert!(text.contains("▶ "));
        assert!(text.contains("⚠ "));
    }

    #[test]
    fn side_by_side_rows_pair_deletions_with_additions() {
        let diff = BranchDiff {
            branch_name: "feature".into(),
            base_ref: Some("main".into()),
            title: None,
            ignore_whitespace: false,
            lines: vec![
                crate::git::DiffLine {
                    kind: DiffLineKind::Del,
                    text: "-old".into(),
                    file_path: Some("a.txt".into()),
                },
                crate::git::DiffLine {
                    kind: DiffLineKind::Add,
                    text: "+new".into(),
                    file_path: Some("a.txt".into()),
                },
            ],
            file_positions: vec![],
        };

        let rows = side_by_side_rows(&diff);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].left.text, "-old");
        assert_eq!(rows[0].right.text, "+new");
    }
}
