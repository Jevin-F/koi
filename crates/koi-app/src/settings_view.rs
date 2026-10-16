//! The settings window: appearance, language, font and shortcuts. Changes
//! are saved and applied as they are made.

use koi_core::settings::{Appearance, FONT_SIZES, Language, Settings};

use gpui_kit::base::{Disableable, Selectable};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::{ActiveTheme, Sizable, StyledExt, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use crate::main_window::SECONDARY_CONTEXT;
use crate::shortcuts::{self, Shortcut};
use crate::{APP_NAME, settings};

const SIZE: Size<Pixels> = size(px(680.), px(720.));
const LABEL_WIDTH: Pixels = px(120.);

/// The open settings window, if any.
struct SettingsWindow(AnyWindowHandle);

impl Global for SettingsWindow {}

/// Brings the settings window to the front, opening it if needed.
pub fn open(cx: &mut App) {
    if let Some(handle) = cx.try_global::<SettingsWindow>().map(|window| window.0)
        && handle
            .update(cx, |_, window, _| window.activate_window())
            .is_ok()
    {
        return;
    }
    let options = WindowOptions {
        window_bounds: Some(WindowBounds::centered(SIZE, cx)),
        titlebar: Some(TitlebarOptions {
            title: Some(t!("settings.title", app = APP_NAME).into()),
            ..Default::default()
        }),
        ..Default::default()
    };
    match gpui_kit::open_window(options, cx, |window, cx| {
        cx.new(|cx| SettingsView::new(window, cx))
    }) {
        Ok((handle, _)) => cx.set_global(SettingsWindow(handle)),
        Err(error) => {
            if let Some(window) = cx.active_window() {
                window
                    .update(cx, |_, window, cx| {
                        window.push_notification(Notification::error(error.to_string()), cx);
                    })
                    .ok();
            }
        }
    }
}

/// A shortcut with the box where it is edited.
struct ShortcutField {
    shortcut: Shortcut,
    input: Entity<InputState>,
    /// The typed keys are not a valid shortcut.
    invalid: bool,
}

struct SettingsView {
    font_family: Entity<InputState>,
    shortcuts: Vec<ShortcutField>,
    _subscriptions: Vec<Subscription>,
}

impl SettingsView {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let current = settings::get(cx).clone();
        let font_family =
            cx.new(|cx| InputState::new(window, cx).default_value(current.font_family.clone()));
        let mut subscriptions =
            vec![
                cx.subscribe_in(&font_family, window, |this, input, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        let family = input.read(cx).value().trim().to_owned();
                        if !family.is_empty() {
                            this.change(window, cx, |settings| settings.font_family = family);
                        }
                    }
                }),
            ];
        let shortcuts: Vec<ShortcutField> = shortcuts::shortcuts()
            .into_iter()
            .map(|shortcut| {
                let keys = shortcut.keys(&current.shortcuts);
                let input = cx.new(|cx| {
                    InputState::new(window, cx)
                        .placeholder(shortcut.default)
                        .default_value(keys)
                });
                ShortcutField {
                    shortcut,
                    input,
                    invalid: false,
                }
            })
            .collect();
        for (ix, field) in shortcuts.iter().enumerate() {
            subscriptions.push(cx.subscribe_in(
                &field.input,
                window,
                move |this, _, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        this.save_shortcut(ix, window, cx);
                    }
                },
            ));
        }
        Self {
            font_family,
            shortcuts,
            _subscriptions: subscriptions,
        }
    }

    /// Saves and applies a change, reporting a failure to save.
    fn change(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
        change: impl FnOnce(&mut Settings),
    ) {
        if let Err(error) = settings::update(cx, change) {
            window.push_notification(
                Notification::error(t!("error.save_settings", error = error)),
                cx,
            );
        }
        cx.notify();
    }

    /// Stores the shortcut typed in field `ix`; an empty box restores the default.
    fn save_shortcut(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let field = &mut self.shortcuts[ix];
        let keys = field.input.read(cx).value().trim().to_owned();
        field.invalid = !keys.is_empty() && !shortcuts::is_valid(&keys);
        if field.invalid {
            cx.notify();
            return;
        }
        let name = field.shortcut.name.to_owned();
        let custom = (!keys.is_empty() && keys != field.shortcut.default).then_some(keys);
        self.change(window, cx, move |settings| match custom {
            Some(keys) => {
                settings.shortcuts.insert(name, keys);
            }
            None => {
                settings.shortcuts.remove(&name);
            }
        });
    }

    fn reset_shortcut(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let default = self.shortcuts[ix].shortcut.default;
        self.shortcuts[ix]
            .input
            .update(cx, |input, cx| input.set_value(default, window, cx));
        self.save_shortcut(ix, window, cx);
    }

    /// A labelled row of the form.
    fn row(label: SharedString, content: impl IntoElement) -> Div {
        h_flex()
            .gap_3()
            .child(div().w(LABEL_WIDTH).flex_none().text_sm().child(label))
            .child(content)
    }

    /// Buttons choosing one of `options`, with the current one selected.
    fn choice<T: Copy + PartialEq + 'static>(
        id: &'static str,
        current: T,
        options: Vec<(T, SharedString)>,
        apply: fn(&mut Settings, T),
        cx: &mut Context<Self>,
    ) -> impl IntoElement + use<T> {
        h_flex()
            .gap_1()
            .children(options.into_iter().enumerate().map(|(ix, (value, label))| {
                Button::new((id, ix))
                    .small()
                    .outline()
                    .label(label)
                    .selected(value == current)
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.change(window, cx, |settings| apply(settings, value));
                    }))
            }))
    }
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current = settings::get(cx).clone();
        let appearance = Self::choice(
            "appearance",
            current.appearance,
            vec![
                (Appearance::Dark, t!("settings.dark").into()),
                (Appearance::Light, t!("settings.light").into()),
                (Appearance::System, t!("settings.system").into()),
            ],
            |settings, value| settings.appearance = value,
            cx,
        );
        // Each language is named in itself, so it can be found in either.
        let language = Self::choice(
            "language",
            current.language,
            vec![
                (Language::Chinese, "中文".into()),
                (Language::English, "English".into()),
            ],
            |settings, value| settings.language = value,
            cx,
        );
        let size = current.font_size();
        let step = |delta: f32| {
            cx.listener(move |this: &mut Self, _: &ClickEvent, window, cx| {
                this.change(window, cx, |settings| settings.set_font_size(size + delta));
            })
        };
        let font_size = h_flex()
            .gap_2()
            .child(
                Button::new("smaller")
                    .small()
                    .outline()
                    .label("−")
                    .disabled(size <= *FONT_SIZES.start())
                    .when(size > *FONT_SIZES.start(), |button| button.cursor_pointer())
                    .on_click(step(-1.)),
            )
            .child(div().w_8().text_center().text_sm().child(format!("{size}")))
            .child(
                Button::new("larger")
                    .small()
                    .outline()
                    .label("+")
                    .disabled(size >= *FONT_SIZES.end())
                    .when(size < *FONT_SIZES.end(), |button| button.cursor_pointer())
                    .on_click(step(1.)),
            );

        let theme = cx.theme();
        let shortcut_rows = self.shortcuts.iter().enumerate().map(|(ix, field)| {
            h_flex()
                .gap_3()
                .child(
                    div()
                        .w(px(200.))
                        .flex_none()
                        .text_sm()
                        .child(t!(field.shortcut.label, app = APP_NAME)),
                )
                .child(div().w(px(200.)).child(Input::new(&field.input).small()))
                .child(
                    Button::new(("reset-shortcut", ix))
                        .ghost()
                        .xsmall()
                        .label(t!("settings.reset"))
                        .cursor_pointer()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.reset_shortcut(ix, window, cx)
                        })),
                )
                .children(field.invalid.then(|| {
                    div()
                        .text_xs()
                        .text_color(theme.danger)
                        .child(t!("settings.invalid_shortcut"))
                }))
        });
        div()
            .id("settings")
            .size_full()
            .key_context(SECONDARY_CONTEXT)
            .overflow_y_scroll()
            .bg(theme.background)
            .child(
                v_flex()
                    .p_6()
                    .gap_4()
                    .child(Self::row(t!("settings.appearance").into(), appearance))
                    .child(Self::row(t!("settings.language").into(), language))
                    .child(Self::row(
                        t!("settings.font").into(),
                        div()
                            .w(px(240.))
                            .child(Input::new(&self.font_family).small()),
                    ))
                    .child(Self::row(t!("settings.font_size").into(), font_size))
                    .child(
                        div()
                            .pt_2()
                            .text_sm()
                            .font_semibold()
                            .child(t!("settings.shortcuts")),
                    )
                    .child(v_flex().gap_2().children(shortcut_rows)),
            )
    }
}
