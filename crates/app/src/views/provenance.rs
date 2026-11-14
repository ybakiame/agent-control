use agent_control_core::model::{Decision, MatchReason, Scope};
use gpui_kit::assets::IconName;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::console::Console;
use crate::i18n::{self, Locale};
use crate::ui;

fn scope_name(scope: Scope, locale: Locale) -> &'static str {
    match scope {
        Scope::User => i18n::t(locale, "scope.user.tag"),
        Scope::Project => i18n::t(locale, "scope.project.tag"),
        Scope::Session => i18n::t(locale, "scope.session.tag"),
        Scope::Environment => i18n::t(locale, "scope.environment.tag"),
    }
}

fn describe_reason(reason: &MatchReason, locale: Locale) -> String {
    match reason {
        MatchReason::AgentMatch { pattern, agent } => {
            i18n::tr(locale, "reason.agent", &[agent, pattern])
        }
        MatchReason::PathMatch { pattern, path } => {
            i18n::tr(locale, "reason.path", &[path, pattern])
        }
        MatchReason::LanguageMatch { language } => i18n::tr(locale, "reason.language", &[language]),
        MatchReason::TaskTagMatch { tag } => i18n::tr(locale, "reason.tag", &[tag]),
        MatchReason::Universal => i18n::t(locale, "reason.universal").into(),
        MatchReason::ScopePrecedence { winner, over } => i18n::tr(
            locale,
            "reason.scope",
            &[scope_name(*winner, locale), scope_name(*over, locale)],
        ),
        MatchReason::PriorityWin { winner, over } => i18n::tr(
            locale,
            "reason.priority",
            &[&winner.to_string(), &over.to_string()],
        ),
        MatchReason::RecencyWin => i18n::t(locale, "reason.recency").into(),
        MatchReason::ExplicitConflict { other_rule } => {
            i18n::tr(locale, "reason.conflict", &[other_rule])
        }
        MatchReason::CodeCheckFailed {
            file,
            key,
            expected,
            actual,
        } => i18n::tr(locale, "reason.codecheck", &[file, key, expected, actual]),
        MatchReason::Excluded { reason } => i18n::tr(locale, "reason.excluded", &[reason]),
    }
}

fn decision_tag(decision: Decision, locale: Locale) -> Tag {
    match decision {
        Decision::Applied => Tag::success().child(i18n::t(locale, "decision.applied")),
        Decision::Superseded => Tag::secondary().child(i18n::t(locale, "decision.superseded")),
        Decision::ConflictFlagged => Tag::danger().child(i18n::t(locale, "decision.conflict")),
        Decision::FilteredOut => Tag::secondary().child(i18n::t(locale, "decision.filtered")),
        Decision::Disabled => Tag::secondary().child(i18n::t(locale, "decision.disabled")),
    }
}

impl Console {
    pub fn render_provenance(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let bg = cx.theme().background;
        let list_hover = cx.theme().list_hover;
        let list_active = cx.theme().list_active;
        let primary = cx.theme().primary;
        let accent = cx.theme().accent;
        let warning = cx.theme().warning;

        let record_rows = self.provenance.iter().enumerate().map(|(index, record)| {
            let selected = self.selected_provenance == Some(index);
            v_flex()
                .id(format!("prov-row-{index}"))
                .role(Role::Button)
                .aria_label(record.agent_id.clone())
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
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected_provenance = Some(index);
                    cx.notify();
                }))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().font_medium().text_sm().child(record.agent_id.clone()))
                        .child(Tag::secondary().child(
                            self.tr("prov.rules_count", &[&record.trace.len().to_string()]),
                        )),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(record.timestamp.format("%Y-%m-%d %H:%M:%S").to_string()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(record.context.project_path.display().to_string()),
                )
                .into_any_element()
        });

        let detail = if let Some(record) = self
            .selected_provenance
            .and_then(|i| self.provenance.get(i))
        {
            let trace_rows = record.trace.iter().map(|m| {
                let reasons = m.reasons.iter().map(|r| {
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(format!("• {}", describe_reason(r, self.locale)))
                        .into_any_element()
                });
                ui::card(cx)
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(decision_tag(m.decision, self.locale))
                            .child(div().font_medium().text_sm().child(m.rule.title.clone()))
                            .child(Tag::secondary().child(scope_name(m.rule.scope, self.locale))),
                    )
                    .child(ui::mono(cx, m.rule.id.clone()))
                    .children(reasons)
                    .when_some(m.superseded_by.clone(), |this, by| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(accent)
                                .child(self.tr("prov.superseded_by", &[&by])),
                        )
                    })
                    .children(m.warnings.iter().map(|w| {
                        div()
                            .text_xs()
                            .text_color(warning)
                            .child(format!("⚠ {w}"))
                            .into_any_element()
                    }))
                    .into_any_element()
            });

            v_flex()
                .gap_3()
                .child(
                    ui::card(cx)
                        .gap_1()
                        .child(ui::section_label(cx, self.t("prov.context")))
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(self.tr("prov.agent", &[&record.agent_id])),
                        )
                        .child(div().text_sm().text_color(muted).child(self.tr(
                            "prov.project",
                            &[&record.context.project_path.display().to_string()],
                        )))
                        .child(div().text_sm().text_color(muted).child(
                            self.tr("prov.languages", &[&record.context.languages.join(", ")]),
                        ))
                        .child(
                            div().text_sm().text_color(muted).child(
                                self.tr("prov.tags", &[&record.context.task_tags.join(", ")]),
                            ),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(self.tr("prov.id", &[&record.id.to_string()])),
                        ),
                )
                .child(ui::section_label(cx, self.t("prov.trace")))
                .children(trace_rows)
                .into_any_element()
        } else {
            ui::empty_state(
                cx,
                IconName::Network,
                self.t("prov.title"),
                self.t("prov.empty"),
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
                        .w(px(320.))
                        .h_full()
                        .gap_2()
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .child(ui::section_label(cx, self.t("prov.title")))
                                .child(Tag::secondary().child(self.provenance.len().to_string())),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_h_0()
                                .gap_2()
                                .id("provenance-list")
                                .overflow_y_scroll()
                                .children(record_rows),
                        ),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .id("provenance-detail")
                        .overflow_y_scroll()
                        .child(detail),
                ),
        )
    }
}
