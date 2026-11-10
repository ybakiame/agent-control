use agent_control_core::model::{RuleKind, SuggestionStatus};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Selectable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::console::Console;
use crate::ui;

impl Console {
    pub fn render_suggestions(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let bg = cx.theme().background;
        let muted_bg = cx.theme().muted;

        let filter = self.suggestion_filter;
        let filters = [
            (
                Some(SuggestionStatus::Pending),
                "sugg.pending",
                "sugg-filter-pending",
            ),
            (
                Some(SuggestionStatus::Approved),
                "sugg.approved",
                "sugg-filter-approved",
            ),
            (
                Some(SuggestionStatus::Rejected),
                "sugg.rejected",
                "sugg-filter-rejected",
            ),
            (None, "sugg.all", "sugg-filter-all"),
        ];

        let visible: Vec<_> = self
            .suggestions
            .iter()
            .filter(|s| filter.is_none_or(|f| s.status == f))
            .collect();

        let rows = visible.iter().map(|s| {
            let id = s.id;
            let selected = self.selected_suggestion == Some(id);
            let status_tag = match s.status {
                SuggestionStatus::Pending => Tag::warning().child(self.t("sugg.pending")),
                SuggestionStatus::Approved => Tag::success().child(self.t("sugg.approved")),
                SuggestionStatus::Rejected => Tag::secondary().child(self.t("sugg.rejected")),
            };
            let kind_key = match s.kind {
                RuleKind::Preference => "kind.preference",
                RuleKind::Contract => "kind.contract",
                RuleKind::Constraint => "kind.constraint",
            };
            v_flex()
                .id(format!("sugg-row-{id}"))
                .role(Role::Button)
                .aria_label(s.title.clone())
                .p_3()
                .gap_1()
                .border_1()
                .border_color(if selected { cx.theme().primary } else { border })
                .rounded_md()
                .bg(if selected { muted_bg } else { bg })
                .cursor_pointer()
                .on_click(cx.listener(move |this, _, window, cx| {
                    this.select_suggestion(id, window, cx);
                    cx.notify();
                }))
                .child(div().font_medium().text_sm().child(s.title.clone()))
                .child(ui::mono(cx, s.rule_id.clone()))
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(status_tag)
                        .child(Tag::secondary().child(self.t(kind_key))),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.tr("sugg.proposed_by", &[&s.agent_id])),
                )
                .into_any_element()
        });

        let list_body = if self.suggestions.is_empty() {
            ui::empty_state(
                cx,
                IconName::Inbox,
                self.t("sugg.queue"),
                self.t("sugg.empty"),
            )
            .into_any_element()
        } else if visible.is_empty() {
            ui::empty_state(
                cx,
                IconName::Search,
                self.t("sugg.queue"),
                self.t("sugg.empty_filter"),
            )
            .into_any_element()
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .gap_2()
                .id("sugg-list")
                .overflow_y_scroll()
                .children(rows)
                .into_any_element()
        };

        let detail = self
            .selected_suggestion
            .and_then(|id| self.suggestions.iter().find(|s| s.id == id).cloned())
            .map(|s| self.render_suggestion_detail(&s, cx))
            .unwrap_or_else(|| {
                ui::empty_state(
                    cx,
                    IconName::Inbox,
                    self.t("sugg.detail"),
                    self.t("sugg.select_hint"),
                )
                .into_any_element()
            });

        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .p_4()
            .gap_3()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .children(filters.into_iter().map(|(f, key, id)| {
                        Button::new(id)
                            .label(self.t(key))
                            .ghost()
                            .small()
                            .selected(filter == f)
                            .on_click(cx.listener(move |this, _, window, cx| {
                                this.suggestion_filter = f;
                                this.clear_suggestion_editor(window, cx);
                                cx.notify();
                            }))
                            .into_any_element()
                    }))
                    .child(div().flex_1())
                    .child(
                        Tag::warning()
                            .child(self.tr("sugg.count", &[&self.pending_suggestions.to_string()])),
                    ),
            )
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_3()
                    .child(
                        v_flex()
                            .w(px(340.))
                            .h_full()
                            .gap_2()
                            .child(ui::section_label(cx, self.t("sugg.queue")))
                            .child(list_body),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .gap_3()
                            .id("sugg-detail")
                            .overflow_y_scroll()
                            .child(detail),
                    ),
            )
    }

    fn render_suggestion_detail(
        &mut self,
        s: &agent_control_core::model::RuleSuggestion,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let created = s
            .created_at
            .with_timezone(&chrono::Local)
            .format("%Y-%m-%d %H:%M")
            .to_string();
        let kind_key = match s.kind {
            RuleKind::Preference => "kind.preference",
            RuleKind::Contract => "kind.contract",
            RuleKind::Constraint => "kind.constraint",
        };

        let header = v_flex()
            .gap_1()
            .child(ui::section_label(cx, self.t("sugg.detail")))
            .child(div().text_lg().font_bold().child(s.title.clone()))
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(ui::mono(cx, s.rule_id.clone()))
                    .child(Tag::secondary().child(self.t(kind_key)))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.tr("sugg.proposed_by", &[&s.agent_id])),
                    )
                    .child(div().text_xs().text_color(muted).child(created)),
            );

        let rationale = v_flex()
            .gap_1()
            .p_3()
            .border_1()
            .border_color(border)
            .rounded_md()
            .child(
                div()
                    .text_xs()
                    .font_medium()
                    .text_color(muted)
                    .child(self.t("sugg.rationale")),
            )
            .child(
                div().text_sm().child(
                    s.rationale
                        .clone()
                        .unwrap_or_else(|| self.t("sugg.no_rationale").to_string()),
                ),
            );

        if s.status == SuggestionStatus::Pending {
            v_flex()
                .gap_3()
                .child(header)
                .child(rationale)
                .child(ui::section_label(cx, self.t("sugg.edit_title")))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.t("sugg.edit_hint")),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().text_color(muted).child(self.t("rules.id")))
                        .child(Input::new(&self.sugg_rule_id_input).id("sugg-rule-id-input")),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(self.t("rules.title_label")),
                        )
                        .child(Input::new(&self.sugg_title_input).id("sugg-title-input")),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(self.t("rules.content")),
                        )
                        .child(Textarea::new(&self.sugg_content_input).h(px(180.))),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().text_color(muted).child(self.t("sugg.note")))
                        .child(Input::new(&self.sugg_note_input).id("sugg-note-input")),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("sugg-approve")
                                .label(self.t("sugg.approve"))
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.approve_selected_suggestion(window, cx);
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("sugg-reject")
                                .label(self.t("sugg.reject"))
                                .danger()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_reject_suggestion(window, cx);
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else {
            let status_tag = match s.status {
                SuggestionStatus::Approved => Tag::success().child(self.t("sugg.approved")),
                SuggestionStatus::Rejected => Tag::danger().child(self.t("sugg.rejected")),
                SuggestionStatus::Pending => Tag::warning().child(self.t("sugg.pending")),
            };
            let reviewed = s
                .reviewed_at
                .map(|t| {
                    self.tr(
                        "sugg.reviewed",
                        &[&t.with_timezone(&chrono::Local)
                            .format("%Y-%m-%d %H:%M")
                            .to_string()],
                    )
                })
                .unwrap_or_default();
            v_flex()
                .gap_3()
                .child(header)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(status_tag)
                        .child(div().text_xs().text_color(muted).child(reviewed)),
                )
                .child(rationale)
                .child(
                    v_flex()
                        .gap_1()
                        .p_3()
                        .border_1()
                        .border_color(border)
                        .rounded_md()
                        .child(
                            div()
                                .text_xs()
                                .font_medium()
                                .text_color(muted)
                                .child(self.t("rules.content")),
                        )
                        .child(div().text_sm().child(s.content.clone())),
                )
                .when_some(s.review_note.clone(), |this, note| {
                    this.child(ui::kv(cx, self.t("sugg.note"), note))
                })
                .when_some(s.approved_rule_id.clone(), |this, rule_id| {
                    this.child(
                        div()
                            .text_sm()
                            .text_color(cx.theme().success)
                            .child(self.tr("sugg.approved_rule", &[&rule_id])),
                    )
                })
                .into_any_element()
        }
    }
}
