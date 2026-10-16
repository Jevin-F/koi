//! A search box over a list, shown in a dialog: fuzzy filtering, keyboard
//! selection, and optionally an extra entry built from the query itself.

use gpui_kit::component::input::{Enter, Input, InputEvent, InputState, MoveDown, MoveUp};
use gpui_kit::component::{ActiveTheme, WindowExt, h_flex, v_flex};
use gpui_kit::prelude::FluentBuilder;
use gpui_kit::*;
use koi_core::{FuzzyMatch, fuzzy_match};
use rust_i18n::t;

const WIDTH: Pixels = px(600.);
const LIST_HEIGHT: Pixels = px(360.);
/// Matches shown at most; more would only slow the list down, and narrowing
/// the query brings the wanted entry up.
const MAX_SHOWN: usize = 200;

/// One entry of the list.
pub struct PickerItem<T> {
    /// Short label of the entry's kind.
    pub kind: SharedString,
    /// The text searched and shown.
    pub text: String,
    pub value: T,
}

type QueryItem<T> = Box<dyn Fn(&str) -> Option<PickerItem<T>>>;
type OnConfirm<T> = Box<dyn Fn(T, &mut Window, &mut App)>;

pub struct Picker<T: 'static> {
    input: Entity<InputState>,
    items: Vec<PickerItem<T>>,
    make_query_item: Option<QueryItem<T>>,
    /// Entry built from the current query, listed first.
    query_item: Option<PickerItem<T>>,
    /// Matching items, best first, as indices into `items`.
    matches: Vec<(usize, FuzzyMatch)>,
    selected: usize,
    scroll: ScrollHandle,
    on_confirm: OnConfirm<T>,
    _input_events: Subscription,
}

impl<T: Clone + 'static> Picker<T> {
    pub fn new(
        items: Vec<PickerItem<T>>,
        placeholder: SharedString,
        on_confirm: impl Fn(T, &mut Window, &mut App) + 'static,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let input_events = cx.subscribe_in(&input, window, |this, input, event, _, cx| {
            if let InputEvent::Change = event {
                let query = input.read(cx).value();
                this.filter(&query);
                cx.notify();
            }
        });
        let mut this = Self {
            input,
            items,
            make_query_item: None,
            query_item: None,
            matches: Vec::new(),
            selected: 0,
            scroll: ScrollHandle::new(),
            on_confirm: Box::new(on_confirm),
            _input_events: input_events,
        };
        this.filter("");
        this
    }

    /// Adds an entry built from the query, such as "create this name".
    pub fn with_query_item(
        mut self,
        make: impl Fn(&str) -> Option<PickerItem<T>> + 'static,
    ) -> Self {
        self.make_query_item = Some(Box::new(make));
        self
    }

    /// Shows `picker` in a dialog with the search box focused. Focus moves
    /// after opening, since opening a dialog focuses the dialog itself.
    pub fn open(
        picker: Entity<Self>,
        title: Option<SharedString>,
        window: &mut Window,
        cx: &mut App,
    ) {
        window.open_dialog(cx, {
            let picker = picker.clone();
            move |dialog, _, _| {
                let dialog = dialog.w(WIDTH).close_button(false).child(picker.clone());
                match &title {
                    Some(title) => dialog.title(title.clone()),
                    None => dialog,
                }
            }
        });
        let input = picker.read(cx).input.clone();
        input.update(cx, |input, cx| input.focus(window, cx));
    }

    fn filter(&mut self, query: &str) {
        self.query_item = self.make_query_item.as_ref().and_then(|make| make(query));
        self.matches = self
            .items
            .iter()
            .enumerate()
            .filter_map(|(ix, item)| Some((ix, fuzzy_match(query, &item.text)?)))
            .collect();
        // Stable, so equally good matches keep the list order.
        self.matches
            .sort_by_key(|(_, found)| std::cmp::Reverse(found.score));
        self.matches.truncate(MAX_SHOWN);
        self.selected = 0;
        self.scroll.scroll_to_item(0);
    }

    /// Entries in display order, with their match when they come from the list.
    fn entries(&self) -> impl Iterator<Item = (&PickerItem<T>, Option<&FuzzyMatch>)> {
        self.query_item.iter().map(|item| (item, None)).chain(
            self.matches
                .iter()
                .map(|(ix, found)| (&self.items[*ix], Some(found))),
        )
    }

    fn move_selection(&mut self, step: isize, cx: &mut Context<Self>) {
        let len = self.entries().count() as isize;
        if len == 0 {
            return;
        }
        self.selected = (self.selected as isize + step).rem_euclid(len) as usize;
        self.scroll.scroll_to_item(self.selected);
        cx.stop_propagation();
        cx.notify();
    }

    fn confirm(&mut self, position: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(value) = self
            .entries()
            .nth(position)
            .map(|(item, _)| item.value.clone())
        else {
            return;
        };
        window.close_dialog(cx);
        (self.on_confirm)(value, window, cx);
    }
}

impl<T: Clone + 'static> Render for Picker<T> {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let highlight = HighlightStyle {
            color: Some(theme.primary),
            font_weight: Some(FontWeight::BOLD),
            ..HighlightStyle::default()
        };
        let rows: Vec<AnyElement> =
            self.entries()
                .enumerate()
                .map(|(position, (item, found))| {
                    let highlights =
                        found
                            .into_iter()
                            .flat_map(|found| &found.positions)
                            .map(|&at| {
                                let len = item.text[at..].chars().next().map_or(1, char::len_utf8);
                                (at..at + len, highlight)
                            });
                    h_flex()
                        .id(("picker-item", position))
                        .px_2()
                        .py_1()
                        .gap_3()
                        .rounded_md()
                        .cursor_pointer()
                        .when(position == self.selected, |row| row.bg(theme.list_active))
                        .when(position != self.selected, |row| {
                            row.hover(|row| row.bg(theme.list_hover))
                        })
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.confirm(position, window, cx)
                        }))
                        .child(
                            div()
                                .w_12()
                                .flex_none()
                                .text_xs()
                                .text_color(theme.muted_foreground)
                                .child(item.kind.clone()),
                        )
                        .child(
                            div().flex_1().min_w_0().truncate().text_sm().child(
                                StyledText::new(item.text.clone()).with_highlights(highlights),
                            ),
                        )
                        .into_any_element()
                })
                .collect();
        let empty = rows.is_empty();
        // Up, down and enter belong to the list, not to the one-line input or
        // the dialog around it, so their actions are taken before they arrive.
        v_flex()
            .gap_2()
            .capture_action(cx.listener(|this, _: &MoveUp, _, cx| this.move_selection(-1, cx)))
            .capture_action(cx.listener(|this, _: &MoveDown, _, cx| this.move_selection(1, cx)))
            .capture_action(cx.listener(|this, _: &Enter, window, cx| {
                cx.stop_propagation();
                this.confirm(this.selected, window, cx);
            }))
            .child(Input::new(&self.input).id("picker-input"))
            .child(
                v_flex()
                    .id("picker-list")
                    .max_h(LIST_HEIGHT)
                    .overflow_y_scroll()
                    .track_scroll(&self.scroll)
                    .children(rows),
            )
            .when(empty, |list| {
                list.child(
                    div()
                        .py_2()
                        .text_sm()
                        .text_color(theme.muted_foreground)
                        .child(t!("picker.no_match")),
                )
            })
    }
}
