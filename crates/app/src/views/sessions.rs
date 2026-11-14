use agent_control_core::model::{AssuranceTier, Scope, SessionStatus, WorkItemStatus};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use uuid::Uuid;

use crate::console::Console;
use crate::i18n::{self, Locale};
use crate::ui;

fn status_tag(status: SessionStatus, locale: Locale) -> Tag {
    match status {
        SessionStatus::Active => Tag::success().child(i18n::t(locale, "sess.status.active")),
        SessionStatus::HandedOff => Tag::info().child(i18n::t(locale, "sess.status.handed_off")),
        SessionStatus::Completed => {
            Tag::secondary().child(i18n::t(locale, "sess.status.completed"))
        }
        SessionStatus::Failed => Tag::danger().child(i18n::t(locale, "sess.status.failed")),
    }
}

fn work_status_tag(status: WorkItemStatus, locale: Locale) -> Tag {
    match status {
        WorkItemStatus::Done => Tag::success().child(i18n::t(locale, "work.done")),
        WorkItemStatus::Doing => Tag::primary().child(i18n::t(locale, "work.doing")),
        WorkItemStatus::Todo => Tag::secondary().child(i18n::t(locale, "work.todo")),
    }
}

impl Console {
    pub fn render_sessions(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let bg = cx.theme().background;
        let list_hover = cx.theme().list_hover;
        let list_active = cx.theme().list_active;
        let primary = cx.theme().primary;

        let weak = cx.weak_entity();
        let session_rows = self.sessions.iter().map(|s| {
            let id = s.id;
            let selected = self.selected_session == Some(id);
            let row_weak = weak.clone();
            let delete_label = self.t("sess.delete").to_string();
            v_flex()
                .id(format!("session-row-{id}"))
                .role(Role::Button)
                .aria_label(s.task.clone())
                .p_3()
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
                }))
                .context_menu(move |menu, _window, _cx| {
                    session_row_menu(menu, row_weak.clone(), id, delete_label.clone())
                })
                .child(div().font_medium().text_sm().child(s.task.clone()))
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(status_tag(s.status, self.locale))
                        .child(Tag::secondary().child(s.agent_id.clone())),
                )
                .child(div().text_xs().text_color(muted).child(self.tr(
                    "sess.binding",
                    &[
                        &s.constraint_snapshot.rules.len().to_string(),
                        &id.to_string()[..8],
                    ],
                )))
                .into_any_element()
        });

        let agent_buttons = self.agents.iter().enumerate().map(|(index, agent)| {
            let selected = self.selected_agent == index;
            Button::new(format!("agent-{index}"))
                .label(agent.name.clone())
                .ghost()
                .small()
                .selected(selected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected_agent = index;
                    cx.notify();
                }))
                .into_any_element()
        });

        let detail = if let Some(session) = self
            .selected_session
            .and_then(|id| self.sessions.iter().find(|s| s.id == id))
        {
            let snapshot_short = session.constraint_snapshot.snapshot_hash
                [..16.min(session.constraint_snapshot.snapshot_hash.len())]
                .to_string();
            let binding_rules = session.constraint_snapshot.rules.iter().map(|rule| {
                let scope_tag = match rule.scope {
                    Scope::User => Tag::primary().child(self.t("scope.user.tag")),
                    Scope::Project => Tag::warning().child(self.t("scope.project.tag")),
                    Scope::Session => Tag::info().child(self.t("scope.session.tag")),
                    Scope::Environment => Tag::secondary().child(self.t("scope.environment.tag")),
                };
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(scope_tag)
                    .child(div().text_sm().child(rule.title.clone()))
                    .into_any_element()
            });
            let work_rows = session.work_items.iter().map(|item| {
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(work_status_tag(item.status, self.locale))
                    .child(div().text_sm().child(item.title.clone()))
                    .into_any_element()
            });
            let assurance_rows = session.assurance.iter().map(|a| {
                let tier_tag = match a.achieved {
                    AssuranceTier::Advisory => Tag::secondary().child(self.t("tier.advisory")),
                    AssuranceTier::Verified => Tag::success().child(self.t("tier.verified")),
                    AssuranceTier::Enforced => Tag::primary().child(self.t("tier.enforced")),
                };
                v_flex()
                    .p_2()
                    .gap_1()
                    .border_1()
                    .border_color(border)
                    .rounded_sm()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(tier_tag)
                            .child(div().text_sm().child(a.rule_id.clone())),
                    )
                    .child(div().text_xs().text_color(muted).child(a.basis.clone()))
                    .into_any_element()
            });
            let handoff_rows = session.handoffs.iter().map(|h| {
                let ack = if h.acknowledged {
                    Tag::success().child(self.t("sess.ack"))
                } else {
                    Tag::warning().child(self.t("sess.pending_ack"))
                };
                v_flex()
                    .p_2()
                    .gap_1()
                    .border_1()
                    .border_color(border)
                    .rounded_sm()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                div()
                                    .text_sm()
                                    .child(format!("{} → {}", h.from_agent, h.to_agent)),
                            )
                            .child(ack),
                    )
                    .child(div().text_xs().text_color(muted).child(self.tr(
                        "sess.bundle",
                        &[
                            &h.bundle_hash[..12.min(h.bundle_hash.len())],
                            &h.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                        ],
                    )))
                    .into_any_element()
            });

            let work_editor = v_flex()
                .w_full()
                .gap_2()
                .child(
                    Textarea::new(&self.session_goal_input)
                        .w_full()
                        .h(px(56.))
                        .aria_label(self.t("sess.goal_label")),
                )
                .child(
                    Textarea::new(&self.session_work_input)
                        .w_full()
                        .h(px(120.))
                        .aria_label(self.t("sess.work_title")),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.t("sess.work_hint")),
                )
                .child(
                    Button::new("session-work-save")
                        .label(self.t("sess.work_save"))
                        .primary()
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.save_session_work_state(window, cx) {
                                let msg = this.t("sess.work_saved_toast").to_string();
                                this.notify(window, cx, &msg);
                            }
                            cx.notify();
                        })),
                );

            let verification = v_flex()
                .gap_1()
                .child(ui::mono(
                    cx,
                    self.tr(
                        "sess.snapshot",
                        &[
                            &snapshot_short,
                            &session.constraint_snapshot.provenance_id.to_string(),
                        ],
                    ),
                ))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.t("sess.verification_hint")),
                );

            v_flex()
                .gap_3()
                .child(ui::section_label(cx, self.t("sess.selected")))
                .child(
                    ui::card(cx)
                        .gap_1()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(status_tag(session.status, self.locale))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .font_medium()
                                        .text_sm()
                                        .child(session.task.clone()),
                                )
                                .child(
                                    Button::new("session-delete")
                                        .label(self.t("sess.delete"))
                                        .danger()
                                        .small()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.confirm_delete_session(window, cx);
                                        })),
                                ),
                        )
                        .child(div().text_sm().text_color(muted).child(self.tr(
                            "sess.agent_project",
                            &[
                                &session.agent_id,
                                &session.project_path.display().to_string(),
                            ],
                        ))),
                )
                .child(ui::section_label(cx, self.t("sess.binding_title")))
                .child(v_flex().gap_1().children(binding_rules))
                .child(
                    v_flex()
                        .gap_2()
                        .child(ui::section_label(cx, self.t("sess.work_title")))
                        .child(if session.goal.is_empty() {
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(self.t("sess.goal_empty"))
                                .into_any_element()
                        } else {
                            v_flex()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(self.t("sess.goal_label")),
                                )
                                .child(div().text_sm().child(session.goal.clone()))
                                .into_any_element()
                        })
                        .child(if session.work_items.is_empty() {
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(self.t("sess.work_empty"))
                                .into_any_element()
                        } else {
                            v_flex().gap_1().children(work_rows).into_any_element()
                        })
                        .child(ui::disclosure(
                            "sess-work-edit",
                            self.t("sess.work_edit"),
                            self.open_sections.contains("sess-work-edit"),
                            work_editor,
                            cx.listener(|this, _, _, cx| {
                                this.toggle_section("sess-work-edit", cx);
                            }),
                        )),
                )
                .child(
                    v_flex()
                        .gap_2()
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(ui::section_label(cx, self.t("sess.assurance_title")))
                                .child(div().flex_1())
                                .child(
                                    Button::new("session-verify-constraints")
                                        .label(self.t("sess.verify_constraints"))
                                        .small()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if let Some((verified, advisory)) =
                                                this.verify_selected_session_constraints(cx)
                                            {
                                                let msg = this.tr(
                                                    "sess.verified_toast",
                                                    &[&verified.to_string(), &advisory.to_string()],
                                                );
                                                this.notify(window, cx, &msg);
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(if session.assurance.is_empty() {
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(self.t("sess.assurance_empty"))
                                .into_any_element()
                        } else {
                            v_flex().gap_1().children(assurance_rows).into_any_element()
                        }),
                )
                .child(ui::disclosure(
                    "sess-verification",
                    self.t("sess.verification_title"),
                    self.open_sections.contains("sess-verification"),
                    verification,
                    cx.listener(|this, _, _, cx| {
                        this.toggle_section("sess-verification", cx);
                    }),
                ))
                .child(
                    v_flex()
                        .gap_2()
                        .child(ui::section_label(cx, self.t("sess.handoff_title")))
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div().flex_1().min_w_0().child(
                                        Input::new(&self.handoff_target_input)
                                            .id("handoff-target-input"),
                                    ),
                                )
                                .child(
                                    Button::new("handoff-export")
                                        .label(self.t("sess.export"))
                                        .small()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if this.export_handoff(cx) {
                                                this.notify(window, cx, this.t("sess.exported"));
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .children(handoff_rows),
                )
                .into_any_element()
        } else {
            ui::empty_state(
                cx,
                IconName::SquareTerminal,
                self.t("sess.title"),
                self.t("sess.empty"),
            )
            .into_any_element()
        };

        v_flex().flex_1().min_h_0().w_full().p_4().gap_3().child(
            h_flex()
                .flex_1()
                .min_h_0()
                .gap_3()
                .child(
                    v_flex()
                        .w(px(340.))
                        .h_full()
                        .gap_2()
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .child(ui::section_label(cx, self.t("sess.title")))
                                .child(Tag::secondary().child(self.sessions.len().to_string())),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_h_0()
                                .gap_2()
                                .id("sessions-list")
                                .overflow_y_scroll()
                                .children(session_rows),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .gap_3()
                        .id("sessions-detail")
                        .overflow_y_scroll()
                        .child(
                            ui::card(cx)
                                .child(ui::section_label(cx, self.t("sess.new")))
                                .child(
                                    Input::new(&self.session_task_input).id("session-task-input"),
                                )
                                .child(match self.current_project() {
                                    Some(p) => div()
                                        .text_sm()
                                        .text_color(muted)
                                        .child(self.tr("sess.current_project", &[&p.name]))
                                        .into_any_element(),
                                    None => div()
                                        .text_sm()
                                        .text_color(muted)
                                        .child(self.t("sess.no_project"))
                                        .into_any_element(),
                                })
                                .child(h_flex().gap_1().flex_wrap().children(agent_buttons))
                                .child(
                                    Button::new("session-create")
                                        .label(self.t("sess.create"))
                                        .primary()
                                        .small()
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            if this.create_session_from_inputs(window, cx) {
                                                this.notify(window, cx, this.t("sess.created"));
                                            }
                                            cx.notify();
                                        })),
                                ),
                        )
                        .child(detail),
                ),
        )
    }
}

/// Right-click menu for a session row: delete (cascade) the session it belongs to.
fn session_row_menu(
    menu: PopupMenu,
    weak: WeakEntity<Console>,
    id: Uuid,
    delete_label: String,
) -> PopupMenu {
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
