mod config;
mod git;
mod stack;
mod syntax;
mod ui;

use anyhow::{Context, Result};
use config::AppConfig;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use git::{load_branch_diff, open_repo, refresh_repo, BranchDiff, RepoState};
use ratatui::{backend::CrosstermBackend, text::Line, Terminal};
use stack::{detect_stacks, enrich_stacks, StackInfo};
use std::{
    env, io, mem,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender},
    thread,
    time::Duration,
};
use syntax::SyntaxHighlighter;
use ui::{draw, BranchEntry, FocusedPane};

fn main() -> Result<()> {
    let repo_arg = env::args().nth(1).map(PathBuf::from);
    let config = AppConfig::load();
    let repo = open_repo(repo_arg)?;
    let stack_info = detect_stacks(&repo.root);
    let mut app = App::new(config, repo, stack_info);
    app.run()
}

struct StackLoadResult {
    request_id: usize,
    stack_info: StackInfo,
}

struct App {
    config: AppConfig,
    repo: RepoState,
    stack_info: StackInfo,
    display_entries: Vec<BranchEntry>,
    selected_index: usize,
    branch_diff: Option<BranchDiff>,
    highlighted_diff: Option<Vec<Line<'static>>>,
    diff_scroll: usize,
    show_help: bool,
    focus: FocusedPane,
    search_mode: bool,
    search_input: String,
    search_matches: Vec<usize>,
    search_cursor: usize,
    syntax_highlighter: SyntaxHighlighter,
    stack_result_tx: Sender<StackLoadResult>,
    stack_result_rx: Receiver<StackLoadResult>,
    stack_request_id: usize,
}

impl App {
    fn new(config: AppConfig, repo: RepoState, stack_info: StackInfo) -> Self {
        let display_entries = build_display_entries(&repo, &stack_info);
        let (stack_result_tx, stack_result_rx) = mpsc::channel();
        let mut app = Self {
            config,
            repo,
            stack_info,
            display_entries,
            selected_index: 0,
            branch_diff: None,
            highlighted_diff: None,
            diff_scroll: 0,
            show_help: false,
            focus: FocusedPane::BranchList,
            search_mode: false,
            search_input: String::new(),
            search_matches: Vec::new(),
            search_cursor: 0,
            syntax_highlighter: SyntaxHighlighter::new(),
            stack_result_tx,
            stack_result_rx,
            stack_request_id: 0,
        };
        app.reload_stack_info_async();
        app
    }

    fn run(&mut self) -> Result<()> {
        let mut stdout = io::stdout();
        enable_raw_mode().context("failed to enable raw mode")?;
        execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend).context("failed to initialize terminal")?;
        terminal.clear().ok();

        let result = self.event_loop(&mut terminal);
        self.restore_terminal(&mut terminal)?;
        result
    }

    fn event_loop(&mut self, terminal: &mut Terminal<CrosstermBackend<io::Stdout>>) -> Result<()> {
        let mut needs_redraw = true;
        loop {
            if self.apply_pending_stack_updates() {
                needs_redraw = true;
            }

            if needs_redraw {
                terminal.draw(|frame| {
                    draw(
                        frame,
                        &self.config,
                        &self.repo,
                        &self.display_entries,
                        self.selected_index,
                        self.branch_diff.as_ref(),
                        self.highlighted_diff.as_deref(),
                        self.diff_scroll,
                        self.show_help,
                        self.focus,
                        if self.search_mode {
                            Some(self.search_input.as_str())
                        } else {
                            None
                        },
                    );
                })?;
                needs_redraw = false;
            }

            if !event::poll(Duration::from_millis(50))? {
                continue;
            }

            let event = event::read()?;
            let Event::Key(key) = event else {
                if matches!(event, Event::Resize(_, _)) {
                    needs_redraw = true;
                }
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            if self.search_mode {
                self.handle_search_input(key);
                needs_redraw = true;
                continue;
            }

            if self.show_help {
                self.show_help = false;
                needs_redraw = true;
                continue;
            }

            if self.handle_global_keys(&key)? {
                break;
            }

            match self.branch_diff {
                Some(_) => self.handle_detail_keys(key)?,
                None => self.handle_branch_list_keys(key)?,
            }
            needs_redraw = true;
        }
        Ok(())
    }

    fn handle_global_keys(&mut self, key: &KeyEvent) -> Result<bool> {
        match key.code {
            KeyCode::Char('q') => return Ok(true),
            KeyCode::Char('?') => {
                self.show_help = true;
            }
            KeyCode::Char('R') => {
                self.refresh_repo()?;
            }
            _ => {}
        }
        Ok(false)
    }

    fn handle_branch_list_keys(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.move_selection(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_selection(-1),
            KeyCode::Char('J') => self.jump_stack_group(1),
            KeyCode::Char('K') => self.jump_stack_group(-1),
            KeyCode::Char('g') if key.modifiers.is_empty() => self.jump_to_first_branch(),
            KeyCode::Char('G') => self.jump_to_last_branch(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(10)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.move_selection(-10)
            }
            KeyCode::Enter => self.open_selected_branch()?,
            _ => {}
        }
        Ok(())
    }

    fn handle_detail_keys(&mut self, key: KeyEvent) -> Result<()> {
        match key.code {
            KeyCode::Esc => self.close_detail(),
            KeyCode::Tab => {
                self.focus = match self.focus {
                    FocusedPane::BranchList => FocusedPane::Diff,
                    FocusedPane::Diff => FocusedPane::BranchList,
                };
            }
            _ => match self.focus {
                FocusedPane::BranchList => self.handle_branch_list_keys(key)?,
                FocusedPane::Diff => self.handle_diff_keys(key),
            },
        }
        Ok(())
    }

    fn handle_diff_keys(&mut self, key: KeyEvent) {
        let visible = 15usize;
        match key.code {
            KeyCode::Char('j') | KeyCode::Down => self.scroll_diff(1),
            KeyCode::Char('k') | KeyCode::Up => self.scroll_diff(-1),
            KeyCode::Char('J') => self.jump_file(1),
            KeyCode::Char('K') => self.jump_file(-1),
            KeyCode::Char('g') => {
                if key.modifiers.is_empty() {
                    self.scroll_diff_to_top();
                }
            }
            KeyCode::Char('G') => self.scroll_diff_to_bottom(),
            KeyCode::Char('d') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_diff(visible as isize / 2)
            }
            KeyCode::Char('u') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                self.scroll_diff(-(visible as isize / 2))
            }
            KeyCode::Char('/') => {
                self.search_mode = true;
                self.search_input.clear();
            }
            KeyCode::Char('n') => self.advance_match(1),
            KeyCode::Char('N') => self.advance_match(-1),
            _ => {}
        }
    }

    fn handle_search_input(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.search_mode = false;
                self.search_input.clear();
            }
            KeyCode::Enter => {
                self.search_mode = false;
                self.refresh_search_matches();
                self.advance_match(1);
            }
            KeyCode::Backspace => {
                self.search_input.pop();
            }
            KeyCode::Char(ch) => {
                if !key.modifiers.contains(KeyModifiers::CONTROL) {
                    self.search_input.push(ch);
                }
            }
            _ => {}
        }
    }

    fn move_selection(&mut self, delta: isize) {
        let selectable: Vec<usize> = self
            .display_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_header())
            .map(|(i, _)| i)
            .collect();

        if selectable.is_empty() {
            self.selected_index = 0;
            return;
        }

        let current_pos = selectable
            .iter()
            .position(|&i| i == self.selected_index)
            .unwrap_or(0);
        let next_pos = (current_pos as isize + delta).clamp(0, (selectable.len() - 1) as isize);
        self.selected_index = selectable[next_pos as usize];
    }

    fn jump_stack_group(&mut self, direction: isize) {
        // Find header positions
        let headers: Vec<usize> = self
            .display_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_header())
            .map(|(i, _)| i)
            .collect();

        if headers.is_empty() {
            return;
        }

        // Find the first selectable entry after each header
        let group_starts: Vec<usize> = headers
            .iter()
            .filter_map(|&h| {
                self.display_entries
                    .iter()
                    .enumerate()
                    .skip(h + 1)
                    .find(|(_, e)| !e.is_header())
                    .map(|(i, _)| i)
            })
            .collect();

        if group_starts.is_empty() {
            return;
        }

        let current_group = group_starts
            .iter()
            .rposition(|&s| s <= self.selected_index)
            .unwrap_or(0);
        let next_group =
            (current_group as isize + direction).clamp(0, (group_starts.len() - 1) as isize);
        self.selected_index = group_starts[next_group as usize];
    }

    fn jump_to_first_branch(&mut self) {
        if let Some(pos) = self.display_entries.iter().position(|e| !e.is_header()) {
            self.selected_index = pos;
        }
    }

    fn jump_to_last_branch(&mut self) {
        if let Some(pos) = self.display_entries.iter().rposition(|e| !e.is_header()) {
            self.selected_index = pos;
        }
    }

    fn open_selected_branch(&mut self) -> Result<()> {
        let entry = match self.display_entries.get(self.selected_index) {
            Some(e) if !e.is_header() => e.clone(),
            _ => return Ok(()),
        };

        let Some(branch) = self
            .repo
            .branches
            .iter()
            .find(|b| b.name == entry.branch_name())
            .cloned()
        else {
            return Ok(());
        };

        let diff = load_branch_diff(&self.repo.root, &branch)?;
        self.highlighted_diff = Some(self.syntax_highlighter.highlight_diff(&diff)?);
        self.branch_diff = Some(diff);
        self.diff_scroll = 0;
        self.focus = FocusedPane::Diff;
        self.search_matches.clear();
        self.search_cursor = 0;
        Ok(())
    }

    fn close_detail(&mut self) {
        self.branch_diff = None;
        self.highlighted_diff = None;
        self.diff_scroll = 0;
        self.focus = FocusedPane::BranchList;
        self.search_input.clear();
        self.search_matches.clear();
        self.search_cursor = 0;
    }

    fn refresh_repo(&mut self) -> Result<()> {
        self.repo = refresh_repo(&self.repo.root)?;
        self.stack_info = detect_stacks(&self.repo.root);
        self.rebuild_display_entries_preserve_selection(None);
        self.reload_stack_info_async();

        // Clamp selected_index to a valid selectable entry
        let selectable: Vec<usize> = self
            .display_entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_header())
            .map(|(i, _)| i)
            .collect();
        if selectable.is_empty() {
            self.selected_index = 0;
        } else if !selectable.contains(&self.selected_index) {
            self.selected_index = *selectable.last().unwrap();
        }

        if let Some(current_diff) = &self.branch_diff {
            if let Some(branch) = self
                .repo
                .branches
                .iter()
                .find(|branch| branch.name == current_diff.branch_name)
                .cloned()
            {
                let diff = load_branch_diff(&self.repo.root, &branch)?;
                self.highlighted_diff = Some(self.syntax_highlighter.highlight_diff(&diff)?);
                self.branch_diff = Some(diff);
                self.refresh_search_matches();
            } else {
                self.close_detail();
            }
        }

        Ok(())
    }

    fn reload_stack_info_async(&mut self) {
        self.stack_request_id += 1;
        let request_id = self.stack_request_id;
        let root = self.repo.root.clone();
        let stack_info = self.stack_info.clone();
        let tx = self.stack_result_tx.clone();
        thread::spawn(move || {
            let stack_info = enrich_stacks(&root, &stack_info);
            let _ = tx.send(StackLoadResult {
                request_id,
                stack_info,
            });
        });
    }

    fn apply_pending_stack_updates(&mut self) -> bool {
        let mut changed = false;
        while let Ok(result) = self.stack_result_rx.try_recv() {
            if result.request_id != self.stack_request_id {
                continue;
            }

            let selected_branch = self.selected_branch_name().map(ToOwned::to_owned);
            self.stack_info = result.stack_info;
            self.rebuild_display_entries_preserve_selection(selected_branch.as_deref());
            changed = true;
        }
        changed
    }

    fn rebuild_display_entries_preserve_selection(&mut self, branch_name: Option<&str>) {
        let selected_branch = branch_name
            .map(ToOwned::to_owned)
            .or_else(|| self.selected_branch_name().map(ToOwned::to_owned));
        self.display_entries = build_display_entries(&self.repo, &self.stack_info);

        if let Some(branch_name) = selected_branch {
            if let Some(index) = self
                .display_entries
                .iter()
                .position(|entry| !entry.is_header() && entry.branch_name() == branch_name)
            {
                self.selected_index = index;
                return;
            }
        }

        self.selected_index = self
            .display_entries
            .iter()
            .position(|entry| !entry.is_header())
            .unwrap_or(0);
    }

    fn selected_branch_name(&self) -> Option<&str> {
        self.display_entries
            .get(self.selected_index)
            .and_then(|entry| {
                if entry.is_header() {
                    None
                } else {
                    Some(entry.branch_name())
                }
            })
    }

    fn scroll_diff(&mut self, delta: isize) {
        let max_scroll = self
            .branch_diff
            .as_ref()
            .map(|diff| diff.lines.len().saturating_sub(1))
            .unwrap_or(0) as isize;
        let next = self.diff_scroll as isize + delta;
        self.diff_scroll = next.clamp(0, max_scroll) as usize;
    }

    fn scroll_diff_to_top(&mut self) {
        self.diff_scroll = 0;
    }

    fn scroll_diff_to_bottom(&mut self) {
        if let Some(diff) = &self.branch_diff {
            self.diff_scroll = diff.lines.len().saturating_sub(1);
        }
    }

    fn jump_file(&mut self, direction: isize) {
        let Some(diff) = &self.branch_diff else {
            return;
        };
        if diff.file_positions.is_empty() {
            return;
        }

        let current = self.diff_scroll;
        let next = if direction > 0 {
            diff.file_positions
                .iter()
                .copied()
                .find(|position| *position > current)
                .or_else(|| diff.file_positions.last().copied())
        } else {
            diff.file_positions
                .iter()
                .rev()
                .copied()
                .find(|position| *position < current)
                .or_else(|| diff.file_positions.first().copied())
        };

        if let Some(next) = next {
            self.diff_scroll = next;
        }
    }

    fn refresh_search_matches(&mut self) {
        let Some(diff) = &self.branch_diff else {
            self.search_matches.clear();
            return;
        };
        if self.search_input.is_empty() {
            self.search_matches.clear();
            return;
        }
        let needle = self.search_input.to_lowercase();
        self.search_matches = diff
            .lines
            .iter()
            .enumerate()
            .filter_map(|(idx, line)| line.text.to_lowercase().contains(&needle).then_some(idx))
            .collect();
        self.search_cursor = 0;
    }

    fn advance_match(&mut self, direction: isize) {
        if self.search_matches.is_empty() {
            return;
        }
        let len = self.search_matches.len() as isize;
        let next = (self.search_cursor as isize + direction).rem_euclid(len);
        self.search_cursor = next as usize;
        if let Some(position) = self.search_matches.get(self.search_cursor).copied() {
            self.diff_scroll = position;
        }
    }

    fn restore_terminal(
        &mut self,
        terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    ) -> Result<()> {
        disable_raw_mode().context("failed to disable raw mode")?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)
            .context("failed to leave alternate screen")?;
        terminal.show_cursor().ok();
        mem::take(&mut self.search_input);
        Ok(())
    }
}

fn build_display_entries(repo: &RepoState, stack_info: &StackInfo) -> Vec<BranchEntry> {
    let mut entries = Vec::new();
    let mut used_branches = std::collections::HashSet::new();
    let branch_map: std::collections::HashMap<_, _> = repo
        .branches
        .iter()
        .map(|branch| (&branch.name, branch))
        .collect();

    // Stacked branches
    for stack in &stack_info.stacks {
        entries.push(BranchEntry::Header {
            label: stack.name.clone(),
        });
        for (depth, branch_name) in stack.branches.iter().enumerate() {
            if let Some(branch) = branch_map.get(branch_name) {
                let stale = stack_info.is_stale(branch_name);
                entries.push(BranchEntry::Branch {
                    branch_name: branch_name.clone(),
                    is_head: branch.is_head,
                    commit_count: branch.commit_count,
                    ahead: branch.ahead,
                    behind: branch.behind,
                    has_upstream: branch.upstream.is_some(),
                    indent: depth + 1,
                    stale,
                });
                used_branches.insert(branch_name.clone());
            }
        }
    }

    let mut standalone_names = Vec::new();
    let mut seen_standalone = std::collections::HashSet::new();
    if !stack_info.standalone.is_empty() {
        for name in &stack_info.standalone {
            if branch_map.contains_key(name) && !used_branches.contains(name) {
                standalone_names.push(name.clone());
                seen_standalone.insert(name.clone());
            }
        }
    }
    standalone_names.extend(
        repo.branches
            .iter()
            .filter(|branch| !used_branches.contains(&branch.name))
            .map(|branch| branch.name.clone())
            .filter(|name| !seen_standalone.contains(name)),
    );

    if !standalone_names.is_empty() {
        if !stack_info.stacks.is_empty() {
            entries.push(BranchEntry::Header {
                label: "standalone".to_string(),
            });
        }
        for branch_name in standalone_names {
            let Some(branch) = branch_map.get(&branch_name) else {
                continue;
            };
            entries.push(BranchEntry::Branch {
                branch_name,
                is_head: branch.is_head,
                commit_count: branch.commit_count,
                ahead: branch.ahead,
                behind: branch.behind,
                has_upstream: branch.upstream.is_some(),
                indent: 0,
                stale: false,
            });
        }
    }

    entries
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::git::BranchInfo;
    use crate::stack::{Stack, StackInfo};
    use std::collections::HashMap;
    use std::path::PathBuf;

    fn make_branch(name: &str) -> BranchInfo {
        BranchInfo {
            name: name.to_string(),
            is_head: false,
            upstream: None,
            ahead: 0,
            behind: 0,
            commit_count: 1,
            base_ref: Some("main".to_string()),
        }
    }

    fn make_repo(branch_names: &[&str]) -> RepoState {
        RepoState {
            root: PathBuf::from("/tmp/fake"),
            branches: branch_names.iter().map(|n| make_branch(n)).collect(),
        }
    }

    fn empty_stacks() -> StackInfo {
        StackInfo {
            stacks: vec![],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
        }
    }

    // --- build_display_entries ---

    #[test]
    fn display_entries_no_stacks_flat_list() {
        let repo = make_repo(&["feat-a", "feat-b", "main"]);
        let stacks = empty_stacks();
        let entries = build_display_entries(&repo, &stacks);

        // No headers when no stacks
        assert!(entries.iter().all(|e| !e.is_header()));
        assert_eq!(entries.len(), 3);
    }

    #[test]
    fn display_entries_with_stack_adds_headers() {
        let repo = make_repo(&["auth-base", "auth-ui", "main"]);
        let stacks = StackInfo {
            stacks: vec![Stack {
                name: "auth stack".into(),
                branches: vec!["auth-base".into(), "auth-ui".into()],
            }],
            standalone: vec!["main".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
        };

        let entries = build_display_entries(&repo, &stacks);

        // Should have: header, auth-base, auth-ui, header(standalone), main
        let headers: Vec<_> = entries.iter().filter(|e| e.is_header()).collect();
        assert_eq!(headers.len(), 2); // "auth stack" + "standalone"

        let branches: Vec<_> = entries.iter().filter(|e| !e.is_header()).collect();
        assert_eq!(branches.len(), 3);
    }

    #[test]
    fn display_entries_stack_branches_indented() {
        let repo = make_repo(&["base", "mid", "top", "main"]);
        let stacks = StackInfo {
            stacks: vec![Stack {
                name: "my stack".into(),
                branches: vec!["base".into(), "mid".into(), "top".into()],
            }],
            standalone: vec![],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
        };

        let entries = build_display_entries(&repo, &stacks);

        // Check indentation increases
        let branch_entries: Vec<_> = entries
            .iter()
            .filter_map(|e| match e {
                BranchEntry::Branch { indent, .. } => Some(*indent),
                _ => None,
            })
            .collect();
        // Stack branches get indent 1, 2, 3; standalone (main) gets 0
        assert_eq!(branch_entries[0], 1); // base
        assert_eq!(branch_entries[1], 2); // mid
        assert_eq!(branch_entries[2], 3); // top
    }

    #[test]
    fn display_entries_standalone_no_header_when_no_stacks() {
        let repo = make_repo(&["main", "fix"]);
        let stacks = empty_stacks();
        let entries = build_display_entries(&repo, &stacks);
        assert!(entries.iter().all(|e| !e.is_header()));
    }

    #[test]
    fn display_entries_use_stack_standalone_order() {
        let repo = make_repo(&["main", "fix", "topic"]);
        let stacks = StackInfo {
            stacks: vec![],
            standalone: vec!["topic".into(), "main".into(), "fix".into()],
            branch_to_parent: HashMap::new(),
            stale_branches: std::collections::HashSet::new(),
        };

        let entries = build_display_entries(&repo, &stacks);
        let names: Vec<_> = entries.iter().map(BranchEntry::branch_name).collect();
        assert_eq!(names, vec!["topic", "main", "fix"]);
    }

    // --- selection navigation helpers ---

    fn make_test_entries() -> Vec<BranchEntry> {
        vec![
            BranchEntry::Header {
                label: "stack A".into(),
            },
            BranchEntry::Branch {
                branch_name: "a1".into(),
                is_head: false,
                commit_count: 1,
                ahead: 0,
                behind: 0,
                has_upstream: false,
                indent: 1,
                stale: false,
            },
            BranchEntry::Branch {
                branch_name: "a2".into(),
                is_head: false,
                commit_count: 1,
                ahead: 0,
                behind: 0,
                has_upstream: false,
                indent: 2,
                stale: false,
            },
            BranchEntry::Header {
                label: "stack B".into(),
            },
            BranchEntry::Branch {
                branch_name: "b1".into(),
                is_head: false,
                commit_count: 1,
                ahead: 0,
                behind: 0,
                has_upstream: false,
                indent: 1,
                stale: false,
            },
            BranchEntry::Header {
                label: "standalone".into(),
            },
            BranchEntry::Branch {
                branch_name: "main".into(),
                is_head: true,
                commit_count: 0,
                ahead: 0,
                behind: 0,
                has_upstream: false,
                indent: 0,
                stale: false,
            },
        ]
    }

    #[test]
    fn move_selection_skips_headers() {
        let entries = make_test_entries();
        let selectable: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_header())
            .map(|(i, _)| i)
            .collect();

        // Selectable indices should be 1, 2, 4, 6
        assert_eq!(selectable, vec![1, 2, 4, 6]);
    }

    #[test]
    fn move_selection_clamps_at_start() {
        let entries = make_test_entries();
        let selectable: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_header())
            .map(|(i, _)| i)
            .collect();

        // Moving up from first selectable should stay at first
        let current_pos = 0;
        let next_pos = (current_pos as isize - 1).clamp(0, (selectable.len() - 1) as isize);
        assert_eq!(selectable[next_pos as usize], 1);
    }

    #[test]
    fn move_selection_clamps_at_end() {
        let entries = make_test_entries();
        let selectable: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| !e.is_header())
            .map(|(i, _)| i)
            .collect();

        let current_pos = selectable.len() - 1;
        let next_pos =
            (current_pos as isize + 1).clamp(0, (selectable.len() - 1) as isize) as usize;
        assert_eq!(selectable[next_pos], 6);
    }

    #[test]
    fn jump_stack_group_finds_first_entry_after_header() {
        let entries = make_test_entries();
        let headers: Vec<usize> = entries
            .iter()
            .enumerate()
            .filter(|(_, e)| e.is_header())
            .map(|(i, _)| i)
            .collect();

        assert_eq!(headers, vec![0, 3, 5]);

        let group_starts: Vec<usize> = headers
            .iter()
            .filter_map(|&h| {
                entries
                    .iter()
                    .enumerate()
                    .skip(h + 1)
                    .find(|(_, e)| !e.is_header())
                    .map(|(i, _)| i)
            })
            .collect();

        assert_eq!(group_starts, vec![1, 4, 6]);
    }

    #[test]
    fn first_selectable_entry() {
        let entries = make_test_entries();
        let first = entries.iter().position(|e| !e.is_header());
        assert_eq!(first, Some(1));
    }

    #[test]
    fn last_selectable_entry() {
        let entries = make_test_entries();
        let last = entries.iter().rposition(|e| !e.is_header());
        assert_eq!(last, Some(6));
    }
}
