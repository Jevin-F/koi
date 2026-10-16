//! A terminal view: renders a [`Session`] and forwards keyboard, mouse and
//! clipboard input to it.

mod keyboard;
mod mouse;
mod paint;

use std::io;

use gpui_kit::component::button::Button;
use gpui_kit::component::{ActiveTheme, Sizable, Theme, h_flex};
use gpui_kit::*;
use koi_term::state::grid::Scroll;
use koi_term::state::term::TermMode;
use koi_term::{
    CommandBuilder, GridSize, Palette, ProgramStatus, Session, SessionEvent, encode_paste,
    recent_line, submitted_line,
};
use rust_i18n::t;

use mouse::CellHit;
use paint::Geometry;

pub(crate) const CONTEXT: &str = "Terminal";

actions!(terminal, [Copy, Paste]);

/// Keeps keys that the window root or debug tools bind for the terminal.
/// Copy and paste are bound with the other changeable shortcuts.
pub fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("tab", NoAction, Some(CONTEXT)),
        KeyBinding::new("shift-tab", NoAction, Some(CONTEXT)),
        KeyBinding::new("ctrl-shift-i", NoAction, Some(CONTEXT)),
    ]);
}

/// Emitted when something outside the view should react.
pub enum TerminalEvent {
    TitleChanged(Option<String>),
    /// The user pressed enter, usually submitting a prompt.
    Submitted(Option<String>),
    /// A program reported its state with OSC 7501.
    ProgramStatus(ProgramStatus),
    Exited,
    /// The user asked to start the exited program again.
    RestartRequested,
}

pub struct TerminalView {
    session: Session,
    palette: Palette,
    focus: FocusHandle,
    /// Where the grid was last painted; `None` before the first paint.
    geometry: Option<Geometry>,
    /// Text being composed by an input method, not yet sent.
    marked_text: Option<String>,
    /// Set once the process exits, with its exit code when known.
    exit_code: Option<Option<u32>>,
    selecting: bool,
    /// The cell of the last mouse event reported to the application.
    last_reported: Option<CellHit>,
    /// Scroll-wheel movement not yet amounting to a whole line.
    scroll_remainder: f32,
    _pump: Task<()>,
}

impl EventEmitter<TerminalEvent> for TerminalView {}

impl TerminalView {
    /// Starts `command` in a new terminal view. The grid is resized to fit on first paint.
    pub fn spawn(command: CommandBuilder, cx: &mut App) -> io::Result<Entity<Self>> {
        const INITIAL_SIZE: GridSize = GridSize {
            columns: 80,
            lines: 24,
            cell_width: 8,
            cell_height: 16,
        };
        let palette = theme_palette(cx);
        let session = Session::spawn(command, INITIAL_SIZE, palette.clone())?;
        Ok(cx.new(|cx| Self::new(session, palette, cx)))
    }

    fn new(session: Session, palette: Palette, cx: &mut Context<Self>) -> Self {
        cx.observe_global::<Theme>(|this, cx| {
            let palette = theme_palette(cx);
            if this.palette != palette {
                this.session.set_palette(palette.clone());
                this.palette = palette;
                cx.notify();
            }
        })
        .detach();
        let signal = session.signal();
        let pump = cx.spawn(async move |this, cx| {
            while signal.wait().await {
                if this.update(cx, |view, cx| view.drain_events(cx)).is_err() {
                    break;
                }
            }
        });
        Self {
            session,
            palette,
            focus: cx.focus_handle(),
            geometry: None,
            marked_text: None,
            exit_code: None,
            selecting: false,
            last_reported: None,
            scroll_remainder: 0.,
            _pump: pump,
        }
    }

    pub fn focus_handle(&self) -> &FocusHandle {
        &self.focus
    }

    /// Set once the process exits, with its exit code when known.
    pub fn exit_code(&self) -> Option<Option<u32>> {
        self.exit_code
    }

    /// Process id of the program in the terminal, if known.
    pub fn pid(&self) -> Option<u32> {
        self.session.pid()
    }

    /// The latest line of output, for summaries.
    pub fn recent_line(&self) -> Option<String> {
        recent_line(&self.session.term())
    }

    fn drain_events(&mut self, cx: &mut Context<Self>) {
        for event in self.session.take_events() {
            match event {
                SessionEvent::Title(title) => cx.emit(TerminalEvent::TitleChanged(title)),
                SessionEvent::Copy(text) => cx.write_to_clipboard(ClipboardItem::new_string(text)),
                SessionEvent::ProgramStatus(status) => {
                    cx.emit(TerminalEvent::ProgramStatus(status))
                }
                SessionEvent::Exited(code) => {
                    self.exit_code = Some(code);
                    cx.emit(TerminalEvent::Exited);
                }
            }
        }
        cx.notify();
    }

    fn mode(&self) -> TermMode {
        *self.session.term().mode()
    }

    /// Sends input typed or pasted by the user, jumping back to the live screen.
    fn send_input(&mut self, bytes: &[u8], cx: &mut Context<Self>) {
        {
            let mut term = self.session.term();
            term.scroll_display(Scroll::Bottom);
            term.selection = None;
        }
        self.session.write(bytes);
        cx.notify();
    }

    fn copy(&mut self, _: &Copy, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(text) = self.session.term().selection_to_string() {
            cx.write_to_clipboard(ClipboardItem::new_string(text));
        }
    }

    fn paste(&mut self, _: &Paste, _: &mut Window, cx: &mut Context<Self>) {
        let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) else {
            return;
        };
        let bracketed = self.mode().contains(TermMode::BRACKETED_PASTE);
        self.send_input(&encode_paste(&text, bracketed), cx);
    }
}

/// The modifiers a terminal application can see; command is kept by the app.
fn term_modifiers(modifiers: &Modifiers) -> koi_term::Modifiers {
    koi_term::Modifiers {
        shift: modifiers.shift,
        alt: modifiers.alt,
        control: modifiers.control,
    }
}

/// Default terminal colors follow the app; explicit application colors still win.
fn theme_palette(cx: &App) -> Palette {
    use koi_term::state::vte::ansi::Rgb;

    let theme = cx.theme();
    let mut palette = if theme.is_dark() {
        Palette::default()
    } else {
        Palette::light()
    };
    let rgb = |color: Hsla| {
        let color = color.to_rgb();
        Rgb {
            r: (color.r * 255.).round() as u8,
            g: (color.g * 255.).round() as u8,
            b: (color.b * 255.).round() as u8,
        }
    };
    palette.foreground = rgb(theme.foreground);
    palette.background = rgb(theme.background);
    palette.cursor = palette.foreground;
    palette
}

impl Render for TerminalView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let view = cx.entity();
        let exit_notice = self.exit_code.map(|code| match code {
            Some(code) => t!("terminal.exited_with_code", code = code),
            None => t!("terminal.exited"),
        });
        let mut root = div()
            .relative()
            .size_full()
            .track_focus(&self.focus)
            .key_context(CONTEXT)
            .cursor(CursorStyle::IBeam)
            .on_action(cx.listener(Self::copy))
            .on_action(cx.listener(Self::paste))
            .on_key_down(cx.listener(Self::on_key_down))
            .on_mouse_move(cx.listener(Self::on_mouse_move))
            .on_scroll_wheel(cx.listener(Self::on_scroll_wheel));
        for button in [MouseButton::Left, MouseButton::Middle, MouseButton::Right] {
            root = root
                .on_mouse_down(button, cx.listener(Self::on_mouse_down))
                .on_mouse_up(button, cx.listener(Self::on_mouse_up));
        }
        root.child(paint::grid(view).size_full())
            .children(exit_notice.map(|notice| {
                h_flex()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .gap_3()
                    .px_3()
                    .py_1()
                    .bg(cx.theme().secondary)
                    .text_color(paint::to_hsla(self.palette.foreground))
                    .text_sm()
                    .child(notice)
                    .child(
                        Button::new("restart")
                            .xsmall()
                            .label(t!("action.restart"))
                            .cursor_pointer()
                            .on_click(cx.listener(|_, _, _, cx| {
                                cx.emit(TerminalEvent::RestartRequested);
                            })),
                    )
            }))
    }
}
