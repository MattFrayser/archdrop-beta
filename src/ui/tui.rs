use crossterm::{
    event::{self, Event, KeyCode},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout},
    widgets::{Block, Borders, Gauge, Paragraph},
    Frame, Terminal,
};
use std::{io, time::Duration};
use tokio::sync::watch;

pub struct TransferUI {
    progress: watch::Receiver<f64>,
    file_name: String,
    qr_code: String,
    is_recieving: bool,
    status_message: watch::Receiver<Option<String>>,
}

impl TransferUI {
    pub fn new(
        progress: watch::Receiver<f64>,
        file_name: String,
        qr_code: String,
        is_recieving: bool,
        status_message: watch::Receiver<Option<String>>,
    ) -> Self {
        Self {
            progress,
            file_name,
            qr_code,
            is_recieving,
            status_message,
        }
    }

    pub async fn run(&mut self) -> Result<(), io::Error> {
        enable_raw_mode()?;
        let mut stdout = io::stdout();
        execute!(stdout, EnterAlternateScreen)?;

        let backend = CrosstermBackend::new(stdout);
        let mut terminal = Terminal::new(backend)?;

        loop {
            let progress = *self.progress.borrow();
            let status_msg = self.status_message.borrow().clone();

            terminal.draw(|f| {
                self.render_layout(f, progress, status_msg.as_deref());
            })?;

            // Check for keypresses
            if event::poll(Duration::from_millis(100))? {
                if let Event::Key(key) = event::read()? {
                    match key.code {
                        KeyCode::Char('q') | KeyCode::Char('c') | KeyCode::Esc => break,
                        _ => {}
                    }
                }
            }

            if progress >= 100.0 {
                break;
            }
        }

        disable_raw_mode()?;
        execute!(terminal.backend_mut(), LeaveAlternateScreen)?;

        Ok(())
    }

    // Render layout with correct format for size of terminal
    fn render_layout(&self, f: &mut Frame, progress: f64, status_msg: Option<&str>) {
        let width = f.size().width;

        match width {
            w if w >= 112 => self.render_wide(f, progress, status_msg),
            w if w >= 65 => self.render_medium(f, progress, status_msg),
            _ => self.render_compact(f, progress, status_msg),
        }
    }

    fn render_wide(&self, f: &mut Frame, progress: f64, status_msg: Option<&str>) {
        let main_layout = if status_msg.is_some() {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Min(0), Constraint::Length(3)])
                .split(f.size())
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Percentage(100)])
                .split(f.size())
        };

        let content_area = main_layout[0];

        // Original layout: side-by-side with full ASCII logo
        let sides = Layout::default()
            .direction(Direction::Horizontal)
            .margin(2)
            .constraints(vec![Constraint::Percentage(40), Constraint::Percentage(60)])
            .split(content_area);

        let left_sections = Layout::default()
            .direction(Direction::Vertical)
            .constraints(vec![Constraint::Length(15), Constraint::Min(0)])
            .split(sides[0]);

        let left_content = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(vec![
                Constraint::Length(3), // File
                Constraint::Length(3), // Progress
                Constraint::Min(5),    // URL
            ])
            .split(left_sections[1]);

        // Full ASCII logo
        let logo = Paragraph::new(
            r#"
   _____               .__
  /  _  \______   ____ |  |__
 /  /_\  \  __ \_/ ___\|  |  \
/    |    \ | \/\  \___|   Y  \
\____|__  /_|    \___  >___|  /
        \/           \/     \/
             ________
             \______ \_______  ____ ______
              |    |  \_  __ \/  _ \\____ \
              |    `   \  | \(  <_> )  |_> |
              L______  /__|   \____/|   __/
                     \/             |__|

            "#,
        )
        .block(Block::default());
        f.render_widget(logo, left_sections[0]);

        self.render_file_widget(f, left_content[0]);
        self.render_progress_widget(f, progress, left_content[1]);
        self.render_qr_widget(f, sides[1]);

        // Render status message at bottom if present
        if let Some(msg) = status_msg {
            self.render_status_widget(f, msg, main_layout[1]);
        }
    }

    fn render_medium(&self, f: &mut Frame, progress: f64, status_msg: Option<&str>) {
        let main_layout = if status_msg.is_some() {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Min(0), Constraint::Length(3)])
                .split(f.size())
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Percentage(100)])
                .split(f.size())
        };

        // Full vertical stack - single column
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(vec![
                Constraint::Length(10), // Title
                Constraint::Length(3),  // File
                Constraint::Length(3),  // Progress
                Constraint::Min(15),    // QR (goes at bottom)
            ])
            .split(main_layout[0]);

        // Minimal title
        // Full ASCII logo
        let logo = Paragraph::new(
            r#"
   _____               .__    ________                        
  /  _  \______   ____ |  |__ \______ \_______  ____ ______  
 /  /_\  \  __ \_/ ___\|  |  \ |    |  \_  __ \/  _ \\____ \   
/    |    \ | \/\  \___|   Y  \|    `   \  | \(  <_> )  |_> | 
\____|__  /_|    \___  >___|  /_______  /__|   \____/|   __/  
        \/           \/     \/        \/             |__|      
            "#,
        )
        .block(Block::default());
        f.render_widget(logo, chunks[0]);

        self.render_file_widget(f, chunks[1]);
        self.render_progress_widget(f, progress, chunks[2]);
        self.render_qr_widget(f, chunks[3]);

        // Render status message at bottom if present
        if let Some(msg) = status_msg {
            self.render_status_widget(f, msg, main_layout[1]);
        }
    }

    fn render_compact(&self, f: &mut Frame, progress: f64, status_msg: Option<&str>) {
        let main_layout = if status_msg.is_some() {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Min(0), Constraint::Length(3)])
                .split(f.size())
        } else {
            Layout::default()
                .direction(Direction::Vertical)
                .constraints(vec![Constraint::Percentage(100)])
                .split(f.size())
        };

        // Full vertical stack - single column
        let chunks = Layout::default()
            .direction(Direction::Vertical)
            .margin(1)
            .constraints(vec![
                Constraint::Length(4), // Title
                Constraint::Length(3), // File
                Constraint::Length(3), // Progress
                Constraint::Min(15),   // QR (goes at bottom)
            ])
            .split(main_layout[0]);

        // Minimal title
        let title = Paragraph::new("ArchDrop")
            .block(Block::default().borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(title, chunks[0]);

        self.render_file_widget(f, chunks[1]);
        self.render_progress_widget(f, progress, chunks[2]);
        self.render_qr_widget(f, chunks[3]);

        // Render status message at bottom if present
        if let Some(msg) = status_msg {
            self.render_status_widget(f, msg, main_layout[1]);
        }
    }

    //------------
    // Widgets
    //------------
    fn render_file_widget(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let title = if self.is_recieving {
            "Destination"
        } else {
            "Sending"
        };
        let widget = Paragraph::new(self.file_name.clone())
            .block(Block::default().title(title).borders(Borders::ALL));
        f.render_widget(widget, area);
    }

    fn render_progress_widget(&self, f: &mut Frame, progress: f64, area: ratatui::layout::Rect) {
        let widget = Gauge::default()
            .block(Block::default().title("Progress").borders(Borders::ALL))
            .gauge_style(ratatui::style::Style::default().fg(ratatui::style::Color::Green))
            .percent(progress as u16);
        f.render_widget(widget, area);
    }

    fn render_qr_widget(&self, f: &mut Frame, area: ratatui::layout::Rect) {
        let widget = Paragraph::new(self.qr_code.clone())
            .block(Block::default().title("Scan").borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(widget, area);
    }

    fn render_status_widget(&self, f: &mut Frame, msg: &str, area: ratatui::layout::Rect) {
        use ratatui::style::{Color, Modifier, Style};
        let widget = Paragraph::new(msg)
            .style(
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )
            .block(Block::default().borders(Borders::ALL))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(widget, area);
    }
}
