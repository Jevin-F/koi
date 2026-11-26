//! The settings window: a sidebar of pages (general, appearance, shortcuts)
//! beside the chosen page. Changes are saved and applied as they are made.

use koi_core::settings::{Appearance, FONT_SIZES, Language, Settings};

use gpui_kit::assets::IconName;
use gpui_kit::base::{Disableable, Selectable};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::group_box::GroupBoxVariant;
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::notification::Notification;
use gpui_kit::component::setting::{
    SettingField, SettingGroup, SettingItem, SettingPage, Settings as SettingsPanel,
};
use gpui_kit::component::{ActiveTheme, Sizable, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use rust_i18n::t;

use crate::main_window::SECONDARY_CONTEXT;
use crate::shortcuts::{self, Shortcut};
use crate::{APP_NAME, settings};

const SIZE: Size<Pixels> = size(px(760.), px(600.));
const SIDEBAR_WIDTH: Pixels = px(200.);
/// Width of the text boxes for the font and shortcuts.
const INPUT_WIDTH: Pixels = px(200.);

/// The open settings window, if any.
struct SettingsWindow(AnyWindowHandle);

impl Global for SettingsWindow {}

/// Brings the settings window to the front, opening it if needed.
pub fn open(cx: &mut App) {
    // Also reached from the Dock menu while another app is in front.
    cx.activate(true);
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

/// Saves and applies a change, reporting a failure to save.
fn change(window: &mut Window, cx: &mut App, change: impl FnOnce(&mut Settings)) {
    if let Err(error) = settings::update(cx, change) {
        window.push_notification(
            Notification::error(t!("error.save_settings", error = error)),
            cx,
        );
    }
}

/// Buttons choosing one of `options`, with the current one selected.
fn choice<T: Copy + PartialEq + 'static>(
    id: &'static str,
    current: T,
    options: Vec<(T, SharedString)>,
    apply: fn(&mut Settings, T),
) -> impl IntoElement {
    h_flex()
        .gap_1()
        .children(options.into_iter().enumerate().map(|(ix, (value, label))| {
            Button::new((id, ix))
                .small()
                .outline()
                .label(label)
                .selected(value == current)
                .cursor_pointer()
                .on_click(move |_, window, cx| {
                    change(window, cx, |settings| apply(settings, value));
                })
        }))
}

/// A page holding `items` in one group.
fn page(
    title: SharedString,
    icon: IconName,
    items: impl IntoIterator<Item = SettingItem>,
) -> SettingPage {
    SettingPage::new(title)
        .icon(icon)
        .group(SettingGroup::new().items(items))
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
                cx.subscribe_in(&font_family, window, |_, input, event, window, cx| {
                    if matches!(event, InputEvent::PressEnter { .. } | InputEvent::Blur) {
                        let family = input.read(cx).value().trim().to_owned();
                        if !family.is_empty() {
                            change(window, cx, |settings| settings.font_family = family);
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

    /// Stores the shortcut typed in field `ix`; an empty box restores the default.
    fn save_shortcut(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        let field = &mut self.shortcuts[ix];
        let keys = field.input.read(cx).value().trim().to_owned();
        field.invalid = !keys.is_empty() && !shortcuts::is_valid(&keys);
        cx.notify();
        if field.invalid {
            return;
        }
        let name = field.shortcut.name.to_owned();
        let custom = (!keys.is_empty() && keys != field.shortcut.default).then_some(keys);
        change(window, cx, move |settings| match custom {
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

    fn general_page() -> SettingPage {
        page(
            t!("settings.general").into(),
            IconName::Settings,
            [SettingItem::new(
                t!("settings.language"),
                SettingField::render(|_, _, cx| {
                    // Each language is named in itself, so it can be found in either.
                    choice(
                        "language",
                        settings::get(cx).language,
                        vec![
                            (Language::Chinese, "中文".into()),
                            (Language::English, "English".into()),
                        ],
                        |settings, value| settings.language = value,
                    )
                }),
            )],
        )
    }

    fn appearance_page(&self) -> SettingPage {
        let font_family = self.font_family.clone();
        page(
            t!("settings.appearance").into(),
            IconName::Palette,
            [
                SettingItem::new(
                    t!("settings.theme"),
                    SettingField::render(|_, _, cx| {
                        choice(
                            "appearance",
                            settings::get(cx).appearance,
                            vec![
                                (Appearance::Dark, t!("settings.dark").into()),
                                (Appearance::Light, t!("settings.light").into()),
                                (Appearance::System, t!("settings.system").into()),
                            ],
                            |settings, value| settings.appearance = value,
                        )
                    }),
                ),
                SettingItem::new(
                    t!("settings.font"),
                    SettingField::render(move |_, _, _| {
                        div().w(INPUT_WIDTH).child(Input::new(&font_family).small())
                    }),
                ),
                SettingItem::new(
                    t!("settings.font_size"),
                    SettingField::render(|_, _, cx| {
                        font_size_stepper(settings::get(cx).font_size())
                    }),
                ),
            ],
        )
    }

    fn shortcuts_page(&self, cx: &mut Context<Self>) -> SettingPage {
        let view = cx.entity().downgrade();
        let items = self.shortcuts.iter().enumerate().map(|(ix, field)| {
            let (input, invalid, view) = (field.input.clone(), field.invalid, view.clone());
            SettingItem::new(
                t!(field.shortcut.label, app = APP_NAME),
                SettingField::render(move |_, _, cx| {
                    let view = view.clone();
                    v_flex()
                        .items_end()
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .child(div().w(INPUT_WIDTH).child(Input::new(&input).small()))
                                .child(
                                    Button::new(("reset-shortcut", ix))
                                        .ghost()
                                        .xsmall()
                                        .label(t!("settings.reset"))
                                        .cursor_pointer()
                                        .on_click(move |_, window, cx| {
                                            view.update(cx, |this, cx| {
                                                this.reset_shortcut(ix, window, cx)
                                            })
                                            .ok();
                                        }),
                                ),
                        )
                        .when(invalid, |field| {
                            field.child(
                                div()
                                    .text_xs()
                                    .text_color(cx.theme().danger)
                                    .child(t!("settings.invalid_shortcut")),
                            )
                        })
                }),
            )
        });
        page(
            t!("settings.shortcuts").into(),
            IconName::Keyboard,
            items.collect::<Vec<_>>(),
        )
    }
}

/// Buttons that make the font one point smaller or larger around `size`.
fn font_size_stepper(size: f32) -> impl IntoElement {
    let step = |delta: f32| {
        move |_: &ClickEvent, window: &mut Window, cx: &mut App| {
            change(window, cx, |settings| settings.set_font_size(size + delta));
        }
    };
    h_flex()
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
        )
}

impl Render for SettingsView {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .id("settings")
            .size_full()
            .key_context(SECONDARY_CONTEXT)
            .bg(cx.theme().background)
            .child(
                SettingsPanel::new("settings-pages")
                    .sidebar_width(SIDEBAR_WIDTH)
                    .with_group_variant(GroupBoxVariant::Outline)
                    .page(Self::general_page())
                    .page(self.appearance_page())
                    .page(self.shortcuts_page(cx)),
            )
    }
}
