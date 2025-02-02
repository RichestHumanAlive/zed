use std::collections::HashSet;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::OnceLock;

use db::kvp::KEY_VALUE_STORE;
use gpui::{AppContext, Empty, EntityId, EventEmitter};
use ui::{prelude::*, ButtonLike, IconButtonShape, Tooltip};
use workspace::item::ItemHandle;
use workspace::{ToolbarItemEvent, ToolbarItemLocation, ToolbarItemView};

pub struct MultibufferHint {
    shown_on: HashSet<EntityId>,
    active_item: Option<Box<dyn ItemHandle>>,
}

const NUMBER_OF_HINTS: usize = 10;

const SHOWN_COUNT_KEY: &str = "MULTIBUFFER_HINT_SHOWN_COUNT";

impl MultibufferHint {
    pub fn new() -> Self {
        Self {
            shown_on: Default::default(),
            active_item: None,
        }
    }
}

impl MultibufferHint {
    fn counter() -> &'static AtomicUsize {
        static SHOWN_COUNT: OnceLock<AtomicUsize> = OnceLock::new();
        SHOWN_COUNT.get_or_init(|| {
            let value: usize = KEY_VALUE_STORE
                .read_kvp(SHOWN_COUNT_KEY)
                .ok()
                .flatten()
                .and_then(|v| v.parse().ok())
                .unwrap_or(0);

            AtomicUsize::new(value)
        })
    }

    fn shown_count() -> usize {
        Self::counter().load(Ordering::Relaxed)
    }

    fn increment_count(cx: &mut AppContext) {
        Self::set_count(Self::shown_count() + 1, cx)
    }

    pub(crate) fn set_count(count: usize, cx: &mut AppContext) {
        Self::counter().store(count, Ordering::Relaxed);

        db::write_and_log(cx, move || {
            KEY_VALUE_STORE.write_kvp(SHOWN_COUNT_KEY.to_string(), format!("{}", count))
        });
    }

    fn dismiss(&mut self, cx: &mut AppContext) {
        Self::set_count(NUMBER_OF_HINTS, cx)
    }
}

impl EventEmitter<ToolbarItemEvent> for MultibufferHint {}

impl ToolbarItemView for MultibufferHint {
    fn set_active_pane_item(
        &mut self,
        active_pane_item: Option<&dyn ItemHandle>,
        cx: &mut ViewContext<Self>,
    ) -> ToolbarItemLocation {
        if Self::shown_count() > NUMBER_OF_HINTS {
            return ToolbarItemLocation::Hidden;
        }

        let Some(active_pane_item) = active_pane_item else {
            return ToolbarItemLocation::Hidden;
        };

        if active_pane_item.is_singleton(cx) {
            return ToolbarItemLocation::Hidden;
        }

        if self.shown_on.insert(active_pane_item.item_id()) {
            Self::increment_count(cx)
        }

        self.active_item = Some(active_pane_item.boxed_clone());
        ToolbarItemLocation::Secondary
    }
}

impl Render for MultibufferHint {
    fn render(&mut self, cx: &mut ViewContext<Self>) -> impl IntoElement {
        let Some(active_item) = self.active_item.as_ref() else {
            return Empty.into_any_element();
        };

        if active_item.breadcrumbs(cx.theme(), cx).is_none() {
            return Empty.into_any_element();
        }

        h_flex()
            .px_2()
            .justify_between()
            .bg(cx.theme().status().info_background)
            .rounded_md()
            .child(
                h_flex()
                    .gap_2()
                    .child(Label::new("You can edit results inline in multibuffers!"))
                    .child(
                        ButtonLike::new("open_docs")
                            .style(ButtonStyle::Transparent)
                            .child(
                                h_flex()
                                    .gap_1()
                                    .child(Label::new("Read more…"))
                                    .child(Icon::new(IconName::ArrowUpRight).size(IconSize::Small)),
                            )
                            .on_click(move |_event, cx| {
                                cx.open_url("https://zed.dev/docs/multibuffers")
                            }),
                    ),
            )
            .child(
                IconButton::new("dismiss", IconName::Close)
                    .style(ButtonStyle::Transparent)
                    .shape(IconButtonShape::Square)
                    .icon_size(IconSize::Small)
                    .on_click(cx.listener(|this, _event, cx| {
                        this.dismiss(cx);
                        cx.emit(ToolbarItemEvent::ChangeLocation(
                            ToolbarItemLocation::Hidden,
                        ))
                    }))
                    .tooltip(move |cx| Tooltip::text("Dismiss this hint", cx)),
            )
            .into_any_element()
    }
}
