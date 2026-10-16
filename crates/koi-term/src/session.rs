//! A child process running in a pseudo-terminal, together with its screen state.
//!
//! A background thread reads the process output and feeds it into the
//! terminal state, then raises a [`Signal`]. Nothing runs while the process is
//! silent, so idle sessions cost no CPU.

use std::io::{self, Read, Write};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Config, Term};
use alacritty_terminal::vte::ansi::Processor;
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};

use crate::program_status::{ProgramStatus, Scanner};
use crate::{Palette, Signal};

/// Lines of scrollback kept per session.
const SCROLLBACK_LINES: usize = 10_000;
const READ_BUFFER_SIZE: usize = 64 * 1024;

/// Visible size of a terminal: grid cells plus the pixel size of one cell.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct GridSize {
    pub columns: u16,
    pub lines: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}

impl Dimensions for GridSize {
    fn total_lines(&self) -> usize {
        self.screen_lines()
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.lines)
    }

    fn columns(&self) -> usize {
        usize::from(self.columns)
    }
}

impl From<GridSize> for PtySize {
    fn from(size: GridSize) -> Self {
        Self {
            rows: size.lines,
            cols: size.columns,
            pixel_width: size.columns.saturating_mul(size.cell_width),
            pixel_height: size.lines.saturating_mul(size.cell_height),
        }
    }
}

impl From<GridSize> for WindowSize {
    fn from(size: GridSize) -> Self {
        Self {
            num_lines: size.lines,
            num_cols: size.columns,
            cell_width: size.cell_width,
            cell_height: size.cell_height,
        }
    }
}

/// Something the UI should react to, reported by a session.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SessionEvent {
    /// The application set the title, or reset it with `None`.
    Title(Option<String>),
    /// The application asked to copy text to the clipboard.
    Copy(String),
    /// The application reported its state with OSC 7501.
    ProgramStatus(ProgramStatus),
    /// The process exited, with its exit code when known.
    Exited(Option<u32>),
}

/// State shared by the session, its reader thread and the terminal's event listener.
struct Shared {
    writer: Mutex<Box<dyn Write + Send>>,
    events: Mutex<Vec<SessionEvent>>,
    size: Mutex<GridSize>,
    palette: Mutex<Palette>,
    signal: Signal,
}

impl Shared {
    fn write(&self, bytes: &[u8]) {
        let mut writer = self.writer.lock().expect("pty writer lock poisoned");
        // The process may already have exited; its input is then discarded.
        let _ = writer.write_all(bytes).and_then(|()| writer.flush());
    }

    fn push(&self, event: SessionEvent) {
        self.events.lock().expect("event lock poisoned").push(event);
        self.signal.notify();
    }
}

/// Receives requests from the terminal state machine.
pub struct EventProxy(Arc<Shared>);

impl EventListener for EventProxy {
    fn send_event(&self, event: Event) {
        let shared = &self.0;
        match event {
            Event::PtyWrite(text) => shared.write(text.as_bytes()),
            Event::ColorRequest(index, format) => {
                let color = shared
                    .palette
                    .lock()
                    .expect("palette lock poisoned")
                    .indexed(index);
                shared.write(format(color).as_bytes());
            }
            Event::TextAreaSizeRequest(format) => {
                let size = *shared.size.lock().expect("size lock poisoned");
                shared.write(format(size.into()).as_bytes());
            }
            Event::Title(title) => shared.push(SessionEvent::Title(Some(title))),
            Event::ResetTitle => shared.push(SessionEvent::Title(None)),
            Event::ClipboardStore(_, text) => shared.push(SessionEvent::Copy(text)),
            _ => {}
        }
    }
}

/// The terminal state type exposed for rendering and selection.
pub type Terminal = Term<EventProxy>;

/// A running terminal session. Dropping it kills the process.
pub struct Session {
    term: Arc<Mutex<Terminal>>,
    pid: Option<u32>,
    shared: Arc<Shared>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
}

impl Session {
    /// Starts `command` in a new pseudo-terminal of the given size.
    pub fn spawn(
        mut command: CommandBuilder,
        size: GridSize,
        palette: Palette,
    ) -> io::Result<Self> {
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        // Apps launched from Finder have no locale; without UTF-8 wide characters break.
        if ["LC_ALL", "LC_CTYPE", "LANG"]
            .iter()
            .all(|key| command.get_env(key).is_none())
        {
            command.env("LANG", "en_US.UTF-8");
        }

        let pair = native_pty_system()
            .openpty(size.into())
            .map_err(io::Error::other)?;
        let mut child = pair
            .slave
            .spawn_command(command)
            .map_err(io::Error::other)?;
        // Close our copy of the child's end so reads see EOF once the process exits.
        drop(pair.slave);
        let killer = child.clone_killer();
        let pid = child.process_id();
        let reader = pair.master.try_clone_reader().map_err(io::Error::other)?;
        let writer = pair.master.take_writer().map_err(io::Error::other)?;

        let shared = Arc::new(Shared {
            writer: Mutex::new(writer),
            events: Mutex::default(),
            size: Mutex::new(size),
            palette: Mutex::new(palette),
            signal: Signal::default(),
        });
        let config = Config {
            scrolling_history: SCROLLBACK_LINES,
            kitty_keyboard: true,
            ..Config::default()
        };
        let term = Arc::new(Mutex::new(Term::new(
            config,
            &size,
            EventProxy(shared.clone()),
        )));

        let (thread_term, thread_shared) = (term.clone(), shared.clone());
        thread::Builder::new()
            .name("koi-pty-reader".into())
            .spawn(move || {
                read_output(reader, &thread_term, &thread_shared);
                let code = child.wait().ok().map(|status| status.exit_code());
                thread_shared.push(SessionEvent::Exited(code));
                thread_shared.signal.close();
            })?;

        Ok(Self {
            term,
            pid,
            shared,
            master: pair.master,
            killer,
        })
    }

    /// Process id of the program started in the terminal, if known.
    pub fn pid(&self) -> Option<u32> {
        self.pid
    }

    /// Updates the default colors returned to terminal color queries.
    pub fn set_palette(&self, palette: Palette) {
        *self.shared.palette.lock().expect("palette lock poisoned") = palette;
    }

    /// Locks the terminal state for reading or selection changes.
    pub fn term(&self) -> MutexGuard<'_, Terminal> {
        self.term.lock().expect("terminal lock poisoned")
    }

    /// Sends input to the process.
    pub fn write(&self, bytes: &[u8]) {
        self.shared.write(bytes);
    }

    /// Resizes the terminal grid and the pseudo-terminal.
    pub fn resize(&self, size: GridSize) {
        {
            let mut current = self.shared.size.lock().expect("size lock poisoned");
            if *current == size {
                return;
            }
            *current = size;
        }
        self.term().resize(size);
        // Fails only once the process has exited, when the size no longer matters.
        let _ = self.master.resize(size.into());
    }

    /// Raised whenever the screen changes or events are queued; closed after exit.
    pub fn signal(&self) -> Signal {
        self.shared.signal.clone()
    }

    /// Removes and returns the queued events.
    pub fn take_events(&self) -> Vec<SessionEvent> {
        std::mem::take(&mut *self.shared.events.lock().expect("event lock poisoned"))
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        // The process may have exited already.
        let _ = self.killer.kill();
    }
}

fn read_output(mut reader: Box<dyn Read + Send>, term: &Mutex<Terminal>, shared: &Shared) {
    let mut parser: Processor = Processor::new();
    let mut scanner = Scanner::default();
    let mut buffer = vec![0; READ_BUFFER_SIZE];
    loop {
        match reader.read(&mut buffer) {
            Ok(0) => return,
            Ok(count) => {
                scanner.feed(&buffer[..count], |status| {
                    shared.push(SessionEvent::ProgramStatus(status));
                });
                let mut term = term.lock().expect("terminal lock poisoned");
                parser.advance(&mut *term, &buffer[..count]);
                drop(term);
                shared.signal.notify();
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(_) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::index::{Column, Line, Point};
    use std::time::{Duration, Instant};

    const SIZE: GridSize = GridSize {
        columns: 40,
        lines: 10,
        cell_width: 8,
        cell_height: 16,
    };

    fn shell(script: &str) -> CommandBuilder {
        let mut command = CommandBuilder::new("/bin/sh");
        command.args(["-c", script]);
        command
    }

    /// Polls until the process exits, collecting every event.
    fn run_to_exit(session: &Session) -> Vec<SessionEvent> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut events = Vec::new();
        while !events.iter().any(|e| matches!(e, SessionEvent::Exited(_))) {
            assert!(Instant::now() < deadline, "process did not exit in time");
            events.extend(session.take_events());
            thread::sleep(Duration::from_millis(10));
        }
        events
    }

    fn line_text(term: &Terminal, line: i32) -> String {
        let row = &term.grid()[Line(line)];
        (0..term.columns())
            .map(|c| row[Column(c)].c)
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn output_reaches_the_grid() {
        let session = Session::spawn(shell("printf 'hello'"), SIZE, Palette::default()).unwrap();
        let events = run_to_exit(&session);
        assert!(events.contains(&SessionEvent::Exited(Some(0))));
        assert_eq!(line_text(&session.term(), 0), "hello");
    }

    #[test]
    fn knows_the_process_id() {
        let session = Session::spawn(shell("sleep 5"), SIZE, Palette::default()).unwrap();
        assert!(session.pid().is_some_and(|pid| pid > 0));
    }

    #[test]
    fn exit_code_is_reported() {
        let session = Session::spawn(shell("exit 3"), SIZE, Palette::default()).unwrap();
        assert!(run_to_exit(&session).contains(&SessionEvent::Exited(Some(3))));
    }

    #[test]
    fn input_is_delivered_to_the_process() {
        let session = Session::spawn(
            shell("read line; printf \"got:$line\""),
            SIZE,
            Palette::default(),
        )
        .unwrap();
        session.write(b"ping\r");
        run_to_exit(&session);
        let term = session.term();
        let text: Vec<String> = (0..3).map(|line| line_text(&term, line)).collect();
        assert!(
            text.iter().any(|line| line == "got:ping"),
            "screen: {text:?}"
        );
    }

    #[test]
    fn title_and_wide_characters() {
        let script = "printf '\\033]0;agent\\007中文'";
        let session = Session::spawn(shell(script), SIZE, Palette::default()).unwrap();
        let events = run_to_exit(&session);
        assert!(events.contains(&SessionEvent::Title(Some("agent".into()))));
        let term = session.term();
        assert_eq!(term.grid()[Point::new(Line(0), Column(0))].c, '中');
        assert_eq!(term.grid()[Point::new(Line(0), Column(2))].c, '文');
    }

    #[test]
    fn program_status_reports_are_events() {
        let script = "printf '\\033]7501;state=working:app=pi\\033\\\\done'";
        let session = Session::spawn(shell(script), SIZE, Palette::default()).unwrap();
        let events = run_to_exit(&session);
        assert!(events.contains(&SessionEvent::ProgramStatus(ProgramStatus {
            state: "working".into(),
            app: Some("pi".into()),
        })));
        assert_eq!(line_text(&session.term(), 0), "done");
    }

    #[test]
    fn resize_updates_grid() {
        let session = Session::spawn(shell("sleep 5"), SIZE, Palette::default()).unwrap();
        let bigger = GridSize {
            columns: 100,
            lines: 30,
            ..SIZE
        };
        session.resize(bigger);
        assert_eq!(session.term().columns(), 100);
        assert_eq!(session.term().screen_lines(), 30);
    }
}
