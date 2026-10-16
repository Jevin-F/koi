//! A fixed sidebar beside the workspace's flexible content area.

use gpui_kit::base::{HandleEdge, resize_handle};
use gpui_kit::component::h_flex;
use gpui_kit::component::resizable::resize_handle_appearance;
use gpui_kit::*;

use super::WorkspaceView;

#[derive(Clone, Copy)]
struct SidebarDrag;

impl Render for SidebarDrag {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        Empty
    }
}

struct ResizeListener(WeakEntity<WorkspaceView>);

impl IntoElement for ResizeListener {
    type Element = Self;

    fn into_element(self) -> Self::Element {
        self
    }
}

impl Element for ResizeListener {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static std::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (window.request_layout(Style::default(), None, cx), ())
    }

    fn prepaint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut Window,
        _: &mut App,
    ) {
    }

    fn paint(
        &mut self,
        _: Option<&GlobalElementId>,
        _: Option<&InspectorElementId>,
        _: Bounds<Pixels>,
        _: &mut (),
        _: &mut (),
        window: &mut Window,
        _: &mut App,
    ) {
        let view = self.0.clone();
        window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }
            view.update(cx, |this, cx| {
                let Some((width, pointer)) = this.sidebar_drag else {
                    return;
                };
                let available = window.viewport_size().width - px(300.);
                let next = width + event.position.x - pointer;
                this.workspace
                    .sidebar_width
                    .resize(f32::from(next.min(available)).round() as u16);
                this.sidebar_resizing = true;
                cx.notify();
            })
            .ok();
        });

        let view = self.0.clone();
        window.on_mouse_event(move |_: &MouseUpEvent, phase, window, cx| {
            if !phase.bubble() {
                return;
            }
            view.update(cx, |this, cx| {
                this.sidebar_drag = None;
                if std::mem::take(&mut this.sidebar_resizing) {
                    this.persist(window, cx);
                }
            })
            .ok();
        });
    }
}

impl WorkspaceView {
    pub(super) fn render_body(&mut self, window: &Window, cx: &mut Context<Self>) -> AnyElement {
        let width = px(f32::from(
            self.workspace
                .sidebar_width
                .in_window(f32::from(window.viewport_size().width) as u16),
        ));
        let handle = resize_handle("sidebar-resize", Axis::Horizontal)
            .inside(HandleEdge::Trailing)
            .with_appearance(resize_handle_appearance())
            .on_drag(SidebarDrag, {
                let view = cx.entity().downgrade();
                move |drag, _, window, cx| {
                    cx.stop_propagation();
                    view.update(cx, |this, _| {
                        this.sidebar_drag = Some((width, window.mouse_position().x));
                    })
                    .ok();
                    cx.new(|_| *drag)
                }
            });
        h_flex()
            .id("workspace-body")
            .size_full()
            .child(ResizeListener(cx.entity().downgrade()))
            .child(
                div()
                    .relative()
                    .w(width)
                    .h_full()
                    .flex_none()
                    .child(self.render_sidebar(cx))
                    .child(handle),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .h_full()
                    .child(self.render_main(cx)),
            )
            .into_any_element()
    }
}
