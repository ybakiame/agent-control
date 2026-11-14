use agent_control_core::model::Scope;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::console::{Console, Nav};
use crate::ui;

impl Console {
    pub fn render_dashboard(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let accent = cx.theme().accent_foreground;

        let user_rules = self.rules.iter().filter(|r| r.scope == Scope::User).count();
        let project_rules = self
            .rules
            .iter()
            .filter(|r| r.scope == Scope::Project)
            .count();

        let project_tag = match self.current_project() {
            Some(p) => Tag::info().child(self.tr("dash.current_project", &[&p.name])),
            None => Tag::secondary().child(self.t("dash.no_project")),
        };

        let mcp_tag = if self.mcp_running {
            Tag::success().child(self.t("dash.mcp.running"))
        } else {
            Tag::danger().child(self.t("dash.mcp.stopped"))
        };

        let suggestions_banner = if self.pending_suggestions > 0 {
            ui::card(cx)
                .child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .child(
                            h_flex()
                                .size(px(36.))
                                .rounded(cx.theme().radius)
                                .bg(cx.theme().warning.opacity(0.16))
                                .items_center()
                                .justify_center()
                                .child(
                                    Icon::new(IconName::Inbox)
                                        .size_4()
                                        .text_color(cx.theme().warning),
                                ),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .gap_0p5()
                                .child(div().font_medium().text_sm().child(
                                    self.tr("sugg.count", &[&self.pending_suggestions.to_string()]),
                                ))
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(muted)
                                        .child(self.t("dash.suggestions_hint")),
                                ),
                        )
                        .child(
                            Button::new("dash-review-suggestions")
                                .label(self.t("dash.review"))
                                .primary()
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.nav = Nav::Suggestions;
                                    this.refresh_data(cx);
                                    cx.notify();
                                })),
                        ),
                )
                .into_any_element()
        } else {
            div().into_any_element()
        };

        let conflict_rows = self.conflicts.iter().map(|c| {
            let kind = match c.kind {
                agent_control_core::model::ConflictKind::RuleVsRule => {
                    self.t("dash.conflict.rule_vs_rule")
                }
                agent_control_core::model::ConflictKind::RuleVsCode => {
                    self.t("dash.conflict.rule_vs_code")
                }
            };
            ui::card(cx)
                .p_3()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(Tag::danger().child(kind))
                        .child(div().text_sm().child(c.rule_ids.join(", "))),
                )
                .child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(c.description.clone()),
                )
                .when_some(c.resolution_hint.clone(), |this, hint| {
                    this.child(
                        div()
                            .text_xs()
                            .text_color(accent)
                            .child(self.tr("dash.hint", &[&hint])),
                    )
                })
                .into_any_element()
        });

        let log_rows = self.event_log.iter().rev().map(|entry| {
            h_flex()
                .gap_2()
                .text_sm()
                .child(ui::mono(cx, entry.time.clone()))
                .child(div().flex_1().min_w_0().child(entry.text.clone()))
                .into_any_element()
        });

        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .p_4()
            .gap_3()
            .id("dashboard-scroll")
            .overflow_y_scroll()
            .overflow_x_hidden()
            .child(
                h_flex()
                    .gap_3()
                    .flex_wrap()
                    .child(ui::stat_card(
                        cx,
                        IconName::User,
                        self.t("dash.stat.user_rules"),
                        user_rules.to_string(),
                        cx.theme().primary,
                    ))
                    .child(ui::stat_card(
                        cx,
                        IconName::BookOpen,
                        self.t("dash.stat.project_rules"),
                        project_rules.to_string(),
                        cx.theme().info,
                    ))
                    .child(ui::stat_card(
                        cx,
                        IconName::SquareTerminal,
                        self.t("dash.stat.sessions"),
                        self.sessions.len().to_string(),
                        cx.theme().success,
                    ))
                    .child(ui::stat_card(
                        cx,
                        IconName::Network,
                        self.t("dash.stat.provenance"),
                        self.provenance.len().to_string(),
                        cx.theme().info,
                    ))
                    .child(ui::stat_card(
                        cx,
                        IconName::TriangleAlert,
                        self.t("dash.stat.conflicts"),
                        self.conflicts.len().to_string(),
                        cx.theme().danger,
                    ))
                    .child(ui::stat_card(
                        cx,
                        IconName::Inbox,
                        self.t("dash.stat.suggestions"),
                        self.pending_suggestions.to_string(),
                        cx.theme().warning,
                    )),
            )
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(mcp_tag)
                    .child(project_tag)
                    .child(div().flex_1())
                    .child(
                        Button::new("dash-check-conflicts")
                            .label(self.t("dash.check_conflicts"))
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.resolve_conflicts_for_project(cx);
                                cx.notify();
                            })),
                    ),
            )
            .child(suggestions_banner)
            .when(!self.conflicts.is_empty(), |this| {
                this.child(
                    v_flex()
                        .gap_2()
                        .child(ui::section_label(cx, self.t("dash.conflicts")))
                        .children(conflict_rows),
                )
            })
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_2()
                    .child(ui::section_label(cx, self.t("dash.activity")))
                    .child(
                        ui::card(cx)
                            .flex_1()
                            .min_h_0()
                            .p_3()
                            .gap_1()
                            .id("dashboard-log")
                            .overflow_y_scroll()
                            .children(log_rows),
                    ),
            )
    }
}
