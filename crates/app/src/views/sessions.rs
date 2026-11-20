use agent_control_core::model::SessionStatus;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use uuid::Uuid;

use crate::console::Console;
use crate::ui::{self, SessionKind};

impl Console {
    pub fn render_sessions(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let bg = cx.theme().background;
        let list_hover = cx.theme().list_hover;
        let list_active = cx.theme().list_active;
        let primary = cx.theme().primary;

        let weak = cx.weak_entity();
        let visible_sessions: Vec<_> = self
            .sessions
            .iter()
            .filter(|s| {
                (self.session_show_completed
                    || !matches!(s.status, SessionStatus::Completed | SessionStatus::Failed))
                    && (self.session_kind_filter == SessionKind::All
                        || ui::session_kind(s) == self.session_kind_filter)
            })
            .collect();
        let mut groups: Vec<(std::path::PathBuf, Vec<&agent_control_core::model::Session>)> =
            Vec::new();
        for s in &visible_sessions {
            match groups.iter_mut().find(|(p, _)| p == &s.project_path) {
                Some((_, rows)) => rows.push(s),
                None => groups.push((s.project_path.clone(), vec![s])),
            }
        }
        let grouped_rows: Vec<AnyElement> = groups
            .iter()
            .map(|(path, rows)| {
                let name = self.project_display_name(path);
                v_flex()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(div().text_xs().text_color(muted).child(name.clone()))
                            .child(Tag::secondary().child(rows.len().to_string())),
                    )
                    .children(rows.iter().map(|s| {
                        let id = s.id;
                        let selected = self.selected_session == Some(id);
                        let row_weak = weak.clone();
                        let delete_label = self.t("sess.delete").to_string();
                        let rename_label = self.t("sess.rename").to_string();
                        let display_title = ui::display_title(s);
                        let kind = ui::session_kind(s);
                        let kind_tag = match kind {
                            SessionKind::Telemetry => {
                                Tag::secondary().child(self.t("sess.kind_telemetry"))
                            }
                            SessionKind::Continuity => {
                                Tag::info().child(self.t("sess.kind_continuity"))
                            }
                            SessionKind::Work | SessionKind::All => {
                                Tag::primary().child(self.t("sess.kind_work"))
                            }
                        };
                        let owners = self
                            .session_owners
                            .get(&id)
                            .filter(|o| o.len() > 1)
                            .map(|o| o.join(" → "))
                            .unwrap_or_else(|| s.agent_id.clone());
                        let updated = s
                            .updated_at
                            .with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string();
                        v_flex()
                            .id(format!("session-row-{id}"))
                            .role(Role::Button)
                            .aria_label(display_title.clone())
                            .p_2()
                            .gap_1()
                            .border_1()
                            .border_color(if selected { primary } else { border })
                            .rounded_md()
                            .bg(if selected { list_active } else { bg })
                            .hover(move |s| {
                                if selected {
                                    s.bg(list_active)
                                } else {
                                    s.bg(list_hover)
                                }
                            })
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.select_session(id, window, cx);
                                this.open_session_sheet(id, window, cx);
                            }))
                            .context_menu(move |menu, _window, _cx| {
                                session_row_menu(
                                    menu,
                                    row_weak.clone(),
                                    id,
                                    delete_label.clone(),
                                    rename_label.clone(),
                                )
                            })
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .child(
                                        div()
                                            .flex_1()
                                            .min_w_0()
                                            .font_medium()
                                            .text_sm()
                                            .child(display_title),
                                    )
                                    .child(ui::status_tag(s.status, self.locale))
                                    .child(kind_tag),
                            )
                            .child(div().text_xs().text_color(muted).child(self.tr(
                                "sess.card_meta",
                                &[
                                    &owners,
                                    &name,
                                    &s.constraint_snapshot.rules.len().to_string(),
                                    &updated,
                                ],
                            )))
                            .children(s.predecessor.map(|p| {
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(self.tr("sess.continues", &[&p.to_string()[..8]]))
                            }))
                            .into_any_element()
                    }))
                    .into_any_element()
            })
            .collect();

        let kind_filters = [
            (SessionKind::All, "sess.filter_all"),
            (SessionKind::Work, "sess.kind_work"),
            (SessionKind::Continuity, "sess.kind_continuity"),
            (SessionKind::Telemetry, "sess.kind_telemetry"),
        ]
        .into_iter()
        .map(|(kind, key)| {
            Button::new(format!("sess-filter-{key}"))
                .label(self.t(key))
                .ghost()
                .xsmall()
                .selected(self.session_kind_filter == kind)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.session_kind_filter = kind;
                    cx.notify();
                }))
                .into_any_element()
        });

        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .p_4()
            .gap_2()
            .child(
                h_flex()
                    .items_center()
                    .gap_2()
                    .child(ui::section_label(cx, self.t("sess.title")))
                    .child(h_flex().gap_1().items_center().children(kind_filters))
                    .child(div().flex_1())
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(self.t("sess.show_completed")),
                            )
                            .child(
                                Switch::new("sessions-show-completed")
                                    .checked(self.session_show_completed)
                                    .accessibility_label(self.t("sess.show_completed"))
                                    .on_click(cx.listener(|this, next, _, cx| {
                                        this.session_show_completed = *next;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(Tag::secondary().child(visible_sessions.len().to_string())),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .id("sessions-list")
                    .overflow_y_scroll()
                    .children(grouped_rows)
                    .when(visible_sessions.is_empty(), |this| {
                        this.child(ui::empty_state(
                            cx,
                            IconName::SquareTerminal,
                            self.t("sess.title"),
                            self.t("sess.empty"),
                        ))
                    }),
            )
    }
}

/// Right-click menu for a session card: rename (title only) or delete (cascade).
fn session_row_menu(
    menu: PopupMenu,
    weak: WeakEntity<Console>,
    id: Uuid,
    delete_label: String,
    rename_label: String,
) -> PopupMenu {
    let rename = weak.clone();
    let menu = menu.item(
        PopupMenuItem::new(rename_label)
            .icon(Icon::new(IconName::FileText))
            .on_click(move |_, window, cx| {
                if let Some(entity) = rename.upgrade() {
                    entity.update(cx, |this, ccx| {
                        this.open_session_rename_dialog(id, window, ccx);
                        ccx.notify();
                    });
                }
            }),
    );
    let delete = weak.clone();
    menu.item(
        PopupMenuItem::new(delete_label)
            .icon(Icon::new(IconName::Close))
            .on_click(move |_, window, cx| {
                if let Some(entity) = delete.upgrade() {
                    entity.update(cx, |this, ccx| {
                        this.selected_session = Some(id);
                        this.confirm_delete_session(window, ccx);
                        ccx.notify();
                    });
                }
            }),
    )
}
