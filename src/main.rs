mod config;
mod git;
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
use std::{env, io, mem, path::PathBuf, time::Duration};
use syntax::SyntaxHighlighter;
use ui::{draw, FocusedPane};

fn main() -> Result<()> {
    let repo_arg = env::args().nth(1).map(PathBuf::from);
    let config = AppConfig::load();
    let repo = open_repo(repo_arg)?;
    let mut app = App::new(config, repo);
    app.run()
}

struct App {
    config: AppConfig,
    repo: RepoState,
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
}

impl App {
    fn new(config: AppConfig, repo: RepoState) -> Self {
        Self {
            config,
            repo,
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
        }
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
        loop {
            terminal.draw(|frame| {
                draw(
                    frame,
                    &self.config,
                    &self.repo,
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

            if !event::poll(Duration::from_millis(100))? {
                continue;
            }

            let Event::Key(key) = event::read()? else {
                continue;
            };
            if key.kind != KeyEventKind::Press {
                continue;
            }

            if self.search_mode {
                self.handle_search_input(key);
                continue;
            }

            if self.show_help {
                self.show_help = false;
                continue;
            }

            if self.handle_global_keys(&key)? {
                break;
            }

            match self.branch_diff {
                Some(_) => self.handle_detail_keys(key)?,
                None => self.handle_branch_list_keys(key)?,
            }
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
        let len = self.repo.branches.len();
        if len == 0 {
            self.selected_index = 0;
            return;
        }

        let current = self.selected_index as isize + delta;
        self.selected_index = current.clamp(0, (len - 1) as isize) as usize;
    }

    fn open_selected_branch(&mut self) -> Result<()> {
        let Some(branch) = self.repo.branches.get(self.selected_index).cloned() else {
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
        if self.repo.branches.is_empty() {
            self.selected_index = 0;
        } else if self.selected_index >= self.repo.branches.len() {
            self.selected_index = self.repo.branches.len() - 1;
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
