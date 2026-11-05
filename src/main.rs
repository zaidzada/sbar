mod tmux;
mod tree;

use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use ratatui::crossterm::event::{
    self, DisableMouseCapture, EnableMouseCapture, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers,
    MouseButton, MouseEvent, MouseEventKind,
};
use ratatui::crossterm::execute;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph};
use ratatui::{DefaultTerminal, Frame};

use tree::{Node, Row};

const KEYS: [(&str, &str); 20] = [
    ("j / ↓", "move down"),
    ("k / ↑", "move up"),
    ("g / Home", "go to top"),
    ("G / End", "go to bottom"),
    ("^d / ^u", "down / up 10 rows"),
    ("h / ←", "fold / go to parent"),
    ("l / →", "unfold / step in"),
    ("Space/Tab", "toggle fold"),
    ("Enter", "toggle dir / open"),
    ("o", "open in vim pane"),
    ("O", "open in new window"),
    ("E", "expand all 1 level"),
    ("C", "collapse all"),
    ("r", "refresh"),
    (".", "toggle dotfiles"),
    ("click", "select, toggle dir"),
    ("dbl-click", "open file"),
    ("wheel", "scroll"),
    ("?", "toggle this help"),
    ("q / Esc", "quit"),
];

struct App {
    root: Node,
    rows: Vec<Row>,
    cursor: usize,
    show_hidden: bool,
    list: ListState,
    list_height: u16,
    last_click: Option<(Instant, usize)>,
    help: bool,
    quit: bool,
}

impl App {
    fn new(dir: PathBuf) -> Self {
        let mut root = Node::new(dir, true);
        root.expand();
        let mut app = App { root, rows: Vec::new(), cursor: 0, show_hidden: false, list: ListState::default(), list_height: 0, last_click: None, help: false, quit: false };
        app.rebuild();
        app
    }

    fn rebuild(&mut self) {
        self.rows = self.root.rows(self.show_hidden);
        self.cursor = self.cursor.min(self.rows.len() - 1);
    }

    fn current(&self) -> &Row {
        &self.rows[self.cursor]
    }

    fn select_path(&mut self, path: &std::path::Path) {
        if let Some(i) = self.rows.iter().position(|r| r.path == path) {
            self.cursor = i;
        }
    }

    fn move_by(&mut self, delta: isize) {
        self.cursor = self.cursor.saturating_add_signed(delta).min(self.rows.len() - 1);
    }

    fn set_expanded(&mut self, expanded: bool) {
        let path = self.current().path.clone();
        if let Some(n) = self.root.find_mut(&path) {
            if expanded { n.expand() } else { n.collapse() }
        }
        self.rebuild();
    }

    fn goto_parent(&mut self) {
        if let Some(parent) = self.current().path.parent().map(|p| p.to_path_buf()) {
            self.select_path(&parent);
        }
    }

    fn fold(&mut self) {
        let row = self.current();
        if row.is_dir && row.expanded && self.cursor != 0 {
            self.set_expanded(false);
        } else {
            self.goto_parent();
        }
    }

    fn unfold(&mut self) {
        let row = self.current();
        if row.is_dir && row.expanded {
            self.move_by(1);
        } else if row.is_dir {
            self.set_expanded(true);
        }
    }

    fn expand_level(&mut self) {
        let path = self.current().path.clone();
        self.root.expand_next_level(self.show_hidden);
        self.rebuild();
        self.select_path(&path);
    }

    fn collapse_all(&mut self) {
        let path = self.current().path.clone();
        self.root.collapse_all();
        self.rebuild();
        // keep the cursor on the top-level ancestor of where it was
        let top = match path.strip_prefix(&self.root.path).ok().and_then(|p| p.components().next()) {
            Some(first) => self.root.path.join(first),
            None => self.root.path.clone(),
        };
        self.select_path(&top);
    }

    fn toggle(&mut self) {
        let row = self.current();
        if row.is_dir && self.cursor != 0 {
            self.set_expanded(!row.expanded);
        }
    }

    fn activate(&mut self) {
        if self.current().is_dir {
            self.toggle();
        } else {
            tmux::open(&self.current().path);
        }
    }

    fn open_file(&mut self, in_window: bool) {
        let row = self.current();
        if row.is_dir {
            return;
        }
        if in_window {
            tmux::open_in_window(&row.path);
        } else {
            tmux::open(&row.path);
        }
    }

    fn refresh(&mut self) {
        let path = self.current().path.clone();
        self.root.load();
        self.rebuild();
        self.select_path(&path);
    }

    fn on_key(&mut self, key: KeyEvent) {
        if self.help {
            if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
                self.quit = true;
            }
            self.help = false;
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('c') => self.quit = true,
                KeyCode::Char('d') => self.move_by(10),
                KeyCode::Char('u') => self.move_by(-10),
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Char('j') | KeyCode::Down => self.move_by(1),
            KeyCode::Char('k') | KeyCode::Up => self.move_by(-1),
            KeyCode::Char('g') | KeyCode::Home => self.cursor = 0,
            KeyCode::Char('G') | KeyCode::End => self.cursor = self.rows.len() - 1,
            KeyCode::Char('h') | KeyCode::Left => self.fold(),
            KeyCode::Char('l') | KeyCode::Right => self.unfold(),
            KeyCode::Char(' ') | KeyCode::Tab => self.toggle(),
            KeyCode::Enter => self.activate(),
            KeyCode::Char('E') => self.expand_level(),
            KeyCode::Char('C') => self.collapse_all(),
            KeyCode::Char('?') => self.help = true,
            KeyCode::Char('o') => self.open_file(false),
            KeyCode::Char('O') => self.open_file(true),
            KeyCode::Char('r') => self.refresh(),
            KeyCode::Char('.') => {
                let path = self.current().path.clone();
                self.show_hidden = !self.show_hidden;
                self.rebuild();
                self.select_path(&path);
            }
            _ => {}
        }
    }

    fn on_mouse(&mut self, m: MouseEvent) {
        if self.help {
            if matches!(m.kind, MouseEventKind::Down(_)) {
                self.help = false;
            }
            return;
        }
        match m.kind {
            MouseEventKind::ScrollDown => self.move_by(3),
            MouseEventKind::ScrollUp => self.move_by(-3),
            MouseEventKind::Down(MouseButton::Left) => self.click(m.row),
            _ => {}
        }
    }

    fn click(&mut self, y: u16) {
        if y >= self.list_height {
            return;
        }
        let idx = self.list.offset() + y as usize;
        if idx >= self.rows.len() {
            return;
        }
        let double = matches!(self.last_click, Some((t, i)) if i == idx && t.elapsed() < Duration::from_millis(400));
        self.last_click = if double { None } else { Some((Instant::now(), idx)) };
        self.cursor = idx;
        if self.current().is_dir {
            if !double {
                self.toggle();
            }
        } else if double {
            tmux::open(&self.current().path);
        }
    }

    fn draw(&mut self, frame: &mut Frame) {
        let items: Vec<ListItem> = self
            .rows
            .iter()
            .map(|r| {
                let (marker, style) = if r.is_dir {
                    (if r.expanded { "▾ " } else { "▸ " }, Style::new().fg(Color::Blue).add_modifier(Modifier::BOLD))
                } else {
                    ("", Style::new())
                };
                ListItem::new(Line::from(vec![
                    Span::styled(r.prefix.clone(), Style::new().fg(Color::DarkGray)),
                    Span::styled(format!("{marker}{}", r.name), style),
                ]))
            })
            .collect();
        self.list.select(Some(self.cursor));
        let list = List::new(items).highlight_style(Style::new().add_modifier(Modifier::REVERSED));
        let area: Rect = frame.area();
        let [list_area, footer_area] = Layout::vertical([Constraint::Min(0), Constraint::Length(1)]).areas(area);
        self.list_height = list_area.height;
        frame.render_stateful_widget(list, list_area, &mut self.list);
        frame.render_widget(Paragraph::new("? help").style(Style::new().fg(Color::DarkGray)), footer_area);
        if self.help {
            draw_help(frame, area);
        }
    }
}

fn draw_help(frame: &mut Frame, area: Rect) {
    let key_w = KEYS.iter().map(|(k, _)| k.chars().count()).max().unwrap_or(0);
    let lines: Vec<Line> = KEYS
        .iter()
        .map(|(k, d)| {
            Line::from(vec![
                Span::styled(format!("{k:<key_w$}  "), Style::new().fg(Color::Yellow)),
                Span::raw(*d),
            ])
        })
        .collect();
    let w = (lines.iter().map(|l| l.width()).max().unwrap_or(0) as u16 + 4).min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let popup = Rect::new(area.x + (area.width - w) / 2, area.y + (area.height - h) / 2, w, h);
    let block = Block::new().borders(Borders::ALL).title(" help ");
    frame.render_widget(Clear, popup);
    frame.render_widget(Paragraph::new(lines).block(block), popup);
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> io::Result<()> {
    while !app.quit {
        terminal.draw(|f| app.draw(f))?;
        match event::read()? {
            Event::Key(key) if key.kind == KeyEventKind::Press => app.on_key(key),
            Event::Mouse(m) => app.on_mouse(m),
            _ => {}
        }
    }
    Ok(())
}

fn main() -> io::Result<()> {
    let arg = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("."));
    let dir = arg.canonicalize()?;
    let mut app = App::new(dir);
    let mut terminal = ratatui::init();
    // ratatui's panic hook restores the terminal but knows nothing about mouse capture
    let prev_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = execute!(io::stdout(), DisableMouseCapture);
        prev_hook(info);
    }));
    execute!(io::stdout(), EnableMouseCapture)?;
    let result = run(&mut terminal, &mut app);
    let _ = execute!(io::stdout(), DisableMouseCapture);
    ratatui::restore();
    result
}
