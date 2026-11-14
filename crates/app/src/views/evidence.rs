use agent_control_core::model::EvidenceKind;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Selectable, Sizable, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::console::Console;
use crate::ui;

fn kind_tag(kind: EvidenceKind) -> Tag {
    match kind {
        EvidenceKind::FileChange => Tag::warning().child("file_change"),
        EvidenceKind::ConstraintViolation => Tag::danger().child("violation"),
        EvidenceKind::TestResult => Tag::success().child("test_result"),
        EvidenceKind::HandoffOut => Tag::info().child("handoff_out"),
        EvidenceKind::HandoffIn => Tag::info().child("handoff_in"),
        EvidenceKind::RuleAcknowledgment => Tag::primary().child("rule_ack"),
        EvidenceKind::ToolCall => Tag::secondary().child("tool_call"),
        EvidenceKind::ToolGate => Tag::warning().child("tool_gate"),
        EvidenceKind::SessionStart => Tag::secondary().child("session_start"),
        EvidenceKind::SessionEnd => Tag::secondary().child("session_end"),
    }
}

impl Console {
    pub fn render_evidence(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let danger = cx.theme().danger;
        let success = cx.theme().success;

        let session_buttons = self.sessions.iter().map(|s| {
            let id = s.id;
            let selected = self.selected_session == Some(id);
            let short = id.to_string()[..8].to_string();
            Button::new(format!("ev-session-{id}"))
                .label(format!(
                    "{short} · {}",
                    s.task.chars().take(24).collect::<String>()
                ))
                .ghost()
                .small()
                .selected(selected)
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected_session = Some(id);
                    this.refresh_evidence();
                    cx.notify();
                }))
                .into_any_element()
        });

        let verification_panel = self.verification.as_ref().map(|report| {
            let status_tag = if report.valid {
                Tag::success().child(self.t("ev.valid"))
            } else {
                Tag::danger().child(self.t("ev.broken"))
            };
            let broken = if report.broken_at.is_empty() {
                self.t("ev.none").to_string()
            } else {
                report
                    .broken_at
                    .iter()
                    .map(|s| s.to_string())
                    .collect::<Vec<_>>()
                    .join(", ")
            };
            ui::card(cx)
                .gap_1()
                .border_color(if report.valid { success } else { danger })
                .child(
                    h_flex().gap_2().items_center().child(status_tag).child(
                        div()
                            .text_sm()
                            .child(self.tr("ev.entries", &[&report.entries_checked.to_string()])),
                    ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.tr("ev.broken_at", &[&broken])),
                )
                .child(ui::mono(cx, self.tr("ev.head", &[&report.head_hash])))
                .into_any_element()
        });

        let broken_seqs: Vec<u64> = self
            .verification
            .as_ref()
            .map(|r| r.broken_at.clone())
            .unwrap_or_default();

        let entry_rows = self.evidence.iter().map(|entry| {
            let is_broken = broken_seqs.contains(&entry.seq);
            let payload = serde_json::to_string(&entry.payload).unwrap_or_default();
            let payload_short: String = payload.chars().take(100).collect();
            let prev_short = entry.prev_hash[..12.min(entry.prev_hash.len())].to_string();
            let hash_short = entry.entry_hash[..12.min(entry.entry_hash.len())].to_string();
            ui::card(cx)
                .gap_1()
                .border_color(if is_broken { danger } else { border })
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(format!("#{}", entry.seq)),
                        )
                        .child(kind_tag(entry.kind))
                        .child(div().text_sm().child(entry.agent_id.clone()))
                        .child(div().flex_1())
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(entry.timestamp.format("%H:%M:%S").to_string()),
                        ),
                )
                .child(div().text_xs().text_color(muted).child(payload_short))
                .child(
                    h_flex()
                        .gap_2()
                        .child(ui::mono(cx, self.tr("ev.prev", &[&prev_short])))
                        .child(ui::mono(cx, self.tr("ev.hash", &[&hash_short]))),
                )
                .into_any_element()
        });

        let empty = self.evidence.is_empty();
        let entries_area = if empty {
            ui::empty_state(
                cx,
                IconName::FileText,
                self.t("nav.evidence"),
                self.t("ev.empty"),
            )
            .into_any_element()
        } else {
            v_flex()
                .flex_1()
                .min_h_0()
                .gap_2()
                .id("evidence-entries")
                .overflow_y_scroll()
                .children(entry_rows)
                .into_any_element()
        };

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
                    .child(ui::section_label(cx, self.t("ev.session")))
                    .child(
                        h_flex()
                            .flex_1()
                            .min_w_0()
                            .gap_1()
                            .id("evidence-session-row")
                            .overflow_x_scroll()
                            .children(session_buttons),
                    )
                    .child(
                        Button::new("ev-verify")
                            .label(self.t("ev.verify"))
                            .primary()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.verify_selected_chain();
                                cx.notify();
                            })),
                    ),
            )
            .children(verification_panel)
            .child(entries_area)
    }
}
