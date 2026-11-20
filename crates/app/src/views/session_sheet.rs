use agent_control_core::DataLayout;
use agent_control_core::model::{
    AssuranceTier, EvidenceEntry, EvidenceKind, OwnershipSpan, ProvenanceRecord, Scope, Session,
    SessionStatus, VerificationReport,
};
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Textarea;
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{
    ActiveTheme as _, Icon, Selectable, Sizable, StyledExt, WindowExt as _, h_flex, v_flex,
};
use gpui_kit::prelude::*;
use gpui_kit::*;
use uuid::Uuid;

use crate::console::{Console, SheetTab};
use crate::ui;

/// Everything the per-session Sheet renders, loaded once when the sheet opens (and on refresh),
/// never at render time.
pub struct SheetData {
    pub session: Session,
    pub evidence: Vec<EvidenceEntry>,
    pub provenance: Vec<ProvenanceRecord>,
    pub ownership: Vec<OwnershipSpan>,
    pub chain: VerificationReport,
}

fn local_time(t: chrono::DateTime<chrono::Utc>) -> String {
    t.with_timezone(&chrono::Local)
        .format("%Y-%m-%d %H:%M")
        .to_string()
}

impl Console {
    pub fn load_sheet_data(&mut self) {
        let Some(id) = self.sheet_session else {
            self.sheet_data = None;
            return;
        };
        let loaded = (|| -> anyhow::Result<SheetData> {
            let session = self
                .store
                .get_session(id)?
                .ok_or_else(|| anyhow::anyhow!("session not found"))?;
            Ok(SheetData {
                evidence: self.store.get_evidence(id)?,
                provenance: self.store.provenance_for_session(&session)?,
                ownership: self.store.ownership_timeline(id)?,
                chain: self.store.verify_chain(id)?,
                session,
            })
        })();
        match loaded {
            Ok(data) => self.sheet_data = Some(data),
            Err(err) => {
                self.sheet_data = None;
                self.log(self.tr("sheet.load_failed", &[&err.to_string()]));
            }
        }
    }

    pub fn open_session_sheet(&mut self, id: Uuid, window: &mut Window, cx: &mut Context<Self>) {
        self.sheet_session = Some(id);
        self.selected_session = Some(id);
        self.sheet_payload = None;
        self.sheet_tab = SheetTab::Overview;
        self.sheet_cap = 100;
        let telemetry = self
            .sessions
            .iter()
            .find(|s| s.id == id)
            .map(|s| s.is_telemetry())
            .unwrap_or(false);
        self.sheet_kind_filter = if telemetry { Some("gate") } else { None };
        for g in ["gate", "tool", "test", "handoff", "ack", "status", "work"] {
            self.open_sections.insert(format!("tl-group-{g}"));
        }
        self.load_sheet_data();
        self.load_sheet_live_rules();
        self.load_session_work_inputs(window, cx);
        let weak = cx.weak_entity();
        let close_weak = weak.clone();
        window.open_sheet(cx, move |sheet, _window, cx| {
            let Some(console) = weak.upgrade() else {
                return sheet;
            };
            let (title, body) = console.update(cx, |this, cx| this.render_sheet_chrome(cx));
            let on_close_weak = close_weak.clone();
            sheet
                .title(title)
                .size(px(620.))
                .child(body)
                .on_close(move |_, _, cx| {
                    if let Some(c) = on_close_weak.upgrade() {
                        c.update(cx, |this, cx| this.close_session_sheet(cx));
                    }
                })
        });
    }

    pub fn close_session_sheet(&mut self, cx: &mut Context<Self>) {
        self.sheet_session = None;
        self.sheet_data = None;
        self.sheet_payload = None;
        cx.notify();
    }

    fn render_sheet_chrome(&mut self, cx: &mut Context<Self>) -> (AnyElement, AnyElement) {
        let title = h_flex()
            .gap_2()
            .items_center()
            .child(
                div().font_medium().text_sm().child(
                    self.sheet_data
                        .as_ref()
                        .map(|d| ui::display_title(&d.session))
                        .unwrap_or_else(|| self.t("sheet.title").to_string()),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(cx.theme().muted_foreground)
                    .child(
                        self.sheet_data
                            .as_ref()
                            .map(|d| d.session.id.to_string()[..8].to_string())
                            .unwrap_or_default(),
                    ),
            )
            .child(div().flex_1())
            .child(
                Button::new("sheet-rename")
                    .label(self.t("sess.rename"))
                    .ghost()
                    .xsmall()
                    .on_click(cx.listener(|this, _, window, cx| {
                        if let Some(id) = this.sheet_session {
                            this.open_session_rename_dialog(id, window, cx);
                        }
                    })),
            )
            .into_any_element();
        (title, self.render_sheet_body(cx).into_any_element())
    }

    fn render_sheet_body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let Some(data) = self.sheet_data.as_ref() else {
            return v_flex().p_4().child(ui::empty_state(
                cx,
                gpui_kit::assets::IconName::SquareTerminal,
                self.t("sheet.empty"),
                self.t("sheet.empty_desc"),
            ));
        };
        let muted = cx.theme().muted_foreground;
        let session = &data.session;
        let is_telemetry = session.is_telemetry();
        let ownership = data
            .ownership
            .iter()
            .map(|s| s.agent_id.clone())
            .collect::<Vec<_>>()
            .join(" → ");
        let overview = v_flex()
            .gap_1()
            .child(ui::section_label(cx, self.t("sheet.overview")))
            .child(ui::kv(cx, self.t("sheet.task"), session.task.clone()))
            .child(
                h_flex()
                    .gap_1()
                    .items_center()
                    .child(ui::status_tag(session.status, self.locale))
                    .child(Tag::secondary().child(session.agent_id.clone())),
            )
            .child(ui::kv(cx, self.t("sheet.ownership"), ownership))
            .child(ui::kv(
                cx,
                self.t("sheet.project"),
                session.project_path.to_string_lossy().to_string(),
            ))
            .child(ui::kv(
                cx,
                self.t("sheet.created"),
                local_time(session.created_at),
            ))
            .child(ui::kv(
                cx,
                self.t("sheet.updated"),
                local_time(session.updated_at),
            ))
            .child(ui::kv(
                cx,
                self.t("sess.snapshot"),
                session.constraint_snapshot.snapshot_hash.clone(),
            ))
            .when(is_telemetry, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.t("sess.telemetry_note")),
                )
            });

        let border = cx.theme().border;
        let actions = h_flex()
            .gap_1()
            .flex_wrap()
            .items_center()
            .child(
                if matches!(
                    session.status,
                    SessionStatus::Completed | SessionStatus::Failed
                ) {
                    Button::new("session-reopen")
                        .label(self.t("sess.reopen"))
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.set_selected_session_status(SessionStatus::Active, cx) {
                                let msg = this.t("sess.reopened_toast").to_string();
                                this.notify(window, cx, &msg);
                            }
                            cx.notify();
                        }))
                } else {
                    Button::new("session-complete")
                        .label(self.t("sess.complete"))
                        .small()
                        .on_click(cx.listener(|this, _, window, cx| {
                            if this.set_selected_session_status(SessionStatus::Completed, cx) {
                                let msg = this.t("sess.completed_toast").to_string();
                                this.notify(window, cx, &msg);
                            }
                            cx.notify();
                        }))
                },
            )
            .when(!is_telemetry, |this| {
                this.child(
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
                )
            })
            .child(div().flex_1())
            .child(
                Button::new("session-delete")
                    .label(self.t("sess.delete"))
                    .danger()
                    .small()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.confirm_delete_session(window, cx);
                    })),
            );

        let live_rules = self.sheet_live_rules.clone().unwrap_or_default();
        let snap_ids: Vec<&str> = session
            .constraint_snapshot
            .rules
            .iter()
            .map(|r| r.id.as_str())
            .collect();
        let live_ids: Vec<&str> = live_rules.iter().map(|r| r.id.as_str()).collect();
        let added = live_rules
            .iter()
            .filter(|r| !snap_ids.contains(&r.id.as_str()))
            .count();
        let removed = session
            .constraint_snapshot
            .rules
            .iter()
            .filter(|r| !live_ids.contains(&r.id.as_str()))
            .count();
        let changed = live_rules
            .iter()
            .filter(|r| {
                session
                    .constraint_snapshot
                    .rules
                    .iter()
                    .any(|s| s.id == r.id && (s.version != r.version || s.content != r.content))
            })
            .count();
        let binding = v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(ui::section_label(cx, self.t("sess.binding_live")))
                    .when(added + removed + changed > 0, |this| {
                        this.child(Tag::warning().child(self.tr(
                            "sess.delta",
                            &[
                                &added.to_string(),
                                &removed.to_string(),
                                &changed.to_string(),
                            ],
                        )))
                    })
                    .child(div().flex_1())
                    .when(!is_telemetry, |this| {
                        this.child(
                            Button::new("session-refresh-snapshot")
                                .label(self.t("sess.refresh_snapshot"))
                                .ghost()
                                .xsmall()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.refresh_session_snapshot(cx);
                                })),
                        )
                    }),
            )
            .children(live_rules.iter().map(|r| self.binding_rule_row(r)))
            .when(live_rules.is_empty(), |this| {
                this.child(
                    div()
                        .text_sm()
                        .text_color(muted)
                        .child(self.t("sess.assurance_empty")),
                )
            })
            .child(ui::disclosure(
                "sess-binding-frozen",
                self.t("sess.binding_frozen"),
                self.open_sections.contains("sess-binding-frozen"),
                v_flex()
                    .gap_1()
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.tr("sess.frozen_at", &[&local_time(session.created_at)])),
                    )
                    .child(ui::mono(
                        cx,
                        session.constraint_snapshot.snapshot_hash.clone(),
                    ))
                    .children(
                        session
                            .constraint_snapshot
                            .rules
                            .iter()
                            .map(|r| self.binding_rule_row(r)),
                    ),
                cx.listener(|this, _, _, cx| {
                    this.toggle_section("sess-binding-frozen", cx);
                }),
            ));
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
        let work = v_flex()
            .gap_2()
            .child(ui::section_label(cx, self.t("sess.work_title")))
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(self.tr("sess.work_updated", &[&local_time(session.updated_at)])),
            )
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
                v_flex()
                    .gap_1()
                    .children(session.work_items.iter().map(|item| {
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(ui::work_status_tag(item.status, self.locale))
                            .child(div().text_sm().child(item.title.clone()))
                            .into_any_element()
                    }))
                    .into_any_element()
            })
            .child(ui::disclosure(
                "sess-work-edit",
                self.t("sess.work_edit"),
                self.open_sections.contains("sess-work-edit"),
                work_editor,
                cx.listener(|this, _, _, cx| {
                    this.toggle_section("sess-work-edit", cx);
                }),
            ));

        let assurance = v_flex()
            .gap_2()
            .child(ui::section_label(cx, self.t("sess.assurance_title")))
            .child(if session.assurance.is_empty() {
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.t("sess.assurance_empty"))
                    .into_any_element()
            } else {
                v_flex()
                    .gap_1()
                    .children(session.assurance.iter().map(|a| {
                        let tier_tag = match a.achieved {
                            AssuranceTier::Advisory => {
                                Tag::secondary().child(self.t("tier.advisory"))
                            }
                            AssuranceTier::Verified => {
                                Tag::success().child(self.t("tier.verified"))
                            }
                            AssuranceTier::Enforced => {
                                Tag::primary().child(self.t("tier.enforced"))
                            }
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
                    }))
                    .into_any_element()
            });

        let snapshot_short = session.constraint_snapshot.snapshot_hash
            [..16.min(session.constraint_snapshot.snapshot_hash.len())]
            .to_string();
        let verification = ui::disclosure(
            "sess-verification",
            self.t("sess.verification_title"),
            self.open_sections.contains("sess-verification"),
            v_flex()
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
                ),
            cx.listener(|this, _, _, cx| {
                this.toggle_section("sess-verification", cx);
            }),
        );

        let chain_badge = || {
            if data.chain.valid {
                Tag::success().child(self.t("ev.valid"))
            } else {
                Tag::danger().child(self.t("ev.broken"))
            }
        };
        fn group_of(k: EvidenceKind) -> &'static str {
            match k {
                EvidenceKind::ToolGate | EvidenceKind::ConstraintViolation => "gate",
                EvidenceKind::ToolCall | EvidenceKind::FileChange => "tool",
                EvidenceKind::TestResult => "test",
                EvidenceKind::HandoffOut | EvidenceKind::HandoffIn => "handoff",
                EvidenceKind::RuleAcknowledgment => "ack",
                EvidenceKind::WorkStateUpdate => "work",
                _ => "status",
            }
        }
        fn group_label(g: &str) -> &'static str {
            match g {
                "gate" => "sheet.tl_gate",
                "tool" => "sheet.tl_tool",
                "test" => "sheet.tl_test",
                "handoff" => "sheet.tl_handoff",
                "ack" => "sheet.tl_ack",
                "work" => "sheet.tl_work",
                _ => "sheet.tl_status",
            }
        }
        let telemetry_switch = Switch::new("sheet-telemetry")
            .checked(self.sheet_show_telemetry)
            .accessibility_label(self.t("sheet.telemetry"))
            .xsmall()
            .on_click(cx.listener(|this, checked, _, cx| {
                this.sheet_show_telemetry = *checked;
                cx.notify();
            }));
        let chips = h_flex().gap_1().flex_wrap().items_center().children(
            [
                "all", "gate", "tool", "test", "handoff", "ack", "status", "work",
            ]
            .into_iter()
            .map(|g| {
                let key = if g == "all" {
                    "sheet.tl_all"
                } else {
                    group_label(g)
                };
                let selected = match self.sheet_kind_filter {
                    None => g == "all",
                    Some(f) => f == g,
                };
                let filter = if g == "all" { None } else { Some(g) };
                Button::new(format!("sheet-tl-{g}"))
                    .label(self.t(key))
                    .ghost()
                    .xsmall()
                    .selected(selected)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sheet_kind_filter = filter;
                        this.sheet_cap = 100;
                        cx.notify();
                    }))
                    .into_any_element()
            }),
        );
        let filtered: Vec<&EvidenceEntry> = data
            .evidence
            .iter()
            .filter(|e| match self.sheet_kind_filter {
                Some(f) => group_of(e.kind) == f,
                None => {
                    is_telemetry || self.sheet_show_telemetry || e.kind != EvidenceKind::ToolGate
                }
            })
            .collect();
        let total = filtered.len();
        let capped: Vec<&EvidenceEntry> = filtered
            .iter()
            .rev()
            .take(self.sheet_cap)
            .copied()
            .collect();
        let mut activity_nodes: Vec<AnyElement> = Vec::new();
        if self.sheet_kind_filter.is_none() {
            for g in ["gate", "tool", "test", "handoff", "ack", "status", "work"] {
                let rows: Vec<AnyElement> = capped
                    .iter()
                    .filter(|e| group_of(e.kind) == g)
                    .map(|e| self.evidence_row(cx, e))
                    .collect();
                if rows.is_empty() {
                    continue;
                }
                let key = format!("tl-group-{g}");
                let open = self.open_sections.contains(&key);
                let label = format!("{} ({})", self.t(group_label(g)), rows.len());
                let toggle_key = key.clone();
                activity_nodes.push(
                    ui::disclosure(
                        &key,
                        label,
                        open,
                        v_flex().gap_1().children(rows),
                        cx.listener(move |this, _, _, cx| {
                            this.toggle_section(&toggle_key, cx);
                        }),
                    )
                    .into_any_element(),
                );
            }
        } else {
            for e in &capped {
                activity_nodes.push(self.evidence_row(cx, e));
            }
        }
        let activity =
            v_flex()
                .flex_1()
                .min_h_0()
                .gap_2()
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(ui::section_label(cx, self.t("sheet.timeline")))
                        .child(chain_badge())
                        .child(div().text_xs().text_color(muted).child(
                            self.tr("ev.entries", &[&data.chain.entries_checked.to_string()]),
                        ))
                        .child(div().flex_1())
                        .when(!is_telemetry, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(self.t("sheet.telemetry")),
                            )
                            .child(telemetry_switch)
                        }),
                )
                .child(chips)
                .child(
                    v_flex()
                        .flex_1()
                        .min_h_0()
                        .gap_1()
                        .id("sheet-activity-list")
                        .overflow_y_scroll()
                        .children(activity_nodes)
                        .when(total == 0, |this| {
                            this.child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(self.t("sheet.no_events")),
                            )
                        }),
                )
                .when(total > self.sheet_cap, |this| {
                    this.child(
                        Button::new("sheet-tl-more")
                            .label(self.t("sheet.load_more"))
                            .ghost()
                            .small()
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.sheet_cap += 100;
                                cx.notify();
                            })),
                    )
                });
        let handoff_row = |h: &agent_control_core::model::HandoffRecord, highlight: bool| {
            let tag = if h.acknowledged {
                Tag::success().child(self.t("sess.ack"))
            } else if h.superseded {
                Tag::secondary().child(self.t("sess.superseded"))
            } else {
                Tag::warning().child(self.t("sess.pending_ack"))
            };
            v_flex()
                .p_2()
                .gap_1()
                .border_1()
                .border_color(if highlight {
                    cx.theme().primary
                } else {
                    border
                })
                .rounded_sm()
                .child(
                    h_flex()
                        .gap_1()
                        .items_center()
                        .child(tag)
                        .child(div().text_sm().child(format!(
                            "{} → {}",
                            h.from_agent,
                            h.to_agent.as_deref().unwrap_or(self.t("sess.unspecified"))
                        )))
                        .child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(local_time(h.timestamp)),
                        ),
                )
                .child(ui::mono(
                    cx,
                    self.tr(
                        "sess.bundle",
                        &[
                            &h.bundle_hash[..12.min(h.bundle_hash.len())],
                            &h.timestamp.format("%Y-%m-%d %H:%M:%S").to_string(),
                        ],
                    ),
                ))
                .into_any_element()
        };
        let latest_handoff = session.handoffs.last();
        let superseded: Vec<_> = session.handoffs.iter().filter(|h| h.superseded).collect();
        let other_handoffs = session
            .handoffs
            .iter()
            .filter(|h| !h.superseded && Some(h.id) != latest_handoff.map(|l| l.id))
            .map(|h| handoff_row(h, false));
        let bundle_path = self
            .store
            .layout
            .bundles_dir()
            .join(format!("{}.json", session.id));
        let latest_path = DataLayout::project_handoff_latest(&session.project_path);
        let sheet_id = session.id;
        let handoffs = v_flex()
            .gap_1()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(ui::section_label(cx, self.t("sheet.handoffs")))
                    .child(div().flex_1())
                    .child(
                        Button::new("sheet-handoff-export")
                            .label(self.t("sess.export"))
                            .ghost()
                            .xsmall()
                            .on_click(cx.listener(move |this, _, window, cx| {
                                if this.export_handoff(sheet_id, cx) {
                                    this.notify(window, cx, this.t("sess.exported"));
                                }
                                cx.notify();
                            })),
                    ),
            )
            .child(ui::mono(
                cx,
                self.tr("sess.bundle_path", &[&bundle_path.display().to_string()]),
            ))
            .child(ui::mono(
                cx,
                self.tr("sess.latest_path", &[&latest_path.display().to_string()]),
            ))
            .children(latest_handoff.map(|h| handoff_row(h, true)))
            .children(other_handoffs)
            .when(!superseded.is_empty(), |this| {
                this.child(ui::disclosure(
                    "sess-handoff-history",
                    self.tr("sess.handoff_history", &[&superseded.len().to_string()]),
                    self.open_sections.contains("sess-handoff-history"),
                    v_flex()
                        .gap_1()
                        .children(superseded.iter().map(|h| handoff_row(h, false))),
                    cx.listener(|this, _, _, cx| {
                        this.toggle_section("sess-handoff-history", cx);
                    }),
                ))
            })
            .when(session.handoffs.is_empty(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.t("sheet.no_handoffs")),
                )
            });

        let legacy = data.provenance.iter().any(|p| p.session_id.is_none());
        let prov_rows = data.provenance.iter().map(|p| {
            h_flex()
                .gap_1()
                .items_center()
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(local_time(p.timestamp)),
                )
                .child(Tag::secondary().child(p.agent_id.clone()))
                .child(div().text_sm().child(self.tr(
                    "sheet.prov_row",
                    &[&p.trace.len().to_string(), &p.conflicts.len().to_string()],
                )))
                .when(p.session_id.is_none(), |this| {
                    this.child(Tag::warning().child(self.t("sheet.legacy")))
                })
                .into_any_element()
        });
        let provenance = v_flex()
            .gap_1()
            .child(ui::section_label(cx, self.t("sheet.provenance")))
            .children(prov_rows)
            .when(data.provenance.is_empty(), |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.t("sheet.no_provenance")),
                )
            })
            .when(legacy, |this| {
                this.child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.t("sheet.legacy_note")),
                )
            });

        let history: Vec<&EvidenceEntry> = data
            .evidence
            .iter()
            .filter(|e| {
                matches!(
                    e.kind,
                    EvidenceKind::SessionStart
                        | EvidenceKind::SnapshotRefresh
                        | EvidenceKind::HandoffOut
                        | EvidenceKind::HandoffIn
                )
            })
            .collect();
        let history_pane = v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .child(
                h_flex()
                    .gap_2()
                    .items_center()
                    .child(ui::section_label(cx, self.t("sheet.tab_history")))
                    .child(Tag::secondary().child(history.len().to_string())),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_h_0()
                    .gap_1()
                    .id("sheet-history-list")
                    .overflow_y_scroll()
                    .children(history.iter().rev().map(|e| self.evidence_row(cx, e)))
                    .when(history.is_empty(), |this| {
                        this.child(
                            div()
                                .text_xs()
                                .text_color(muted)
                                .child(self.t("sheet.history_empty")),
                        )
                    }),
            );

        let tab_bar = h_flex().gap_1().items_center().children(
            [
                (
                    SheetTab::Overview,
                    "sheet-tab-overview",
                    "sheet.tab_overview",
                ),
                (
                    SheetTab::Activity,
                    "sheet-tab-activity",
                    "sheet.tab_activity",
                ),
                (SheetTab::History, "sheet-tab-history", "sheet.tab_history"),
            ]
            .into_iter()
            .map(|(tab, id, key)| {
                Button::new(id)
                    .label(self.t(key))
                    .ghost()
                    .xsmall()
                    .selected(self.sheet_tab == tab)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.sheet_tab = tab;
                        cx.notify();
                    }))
                    .into_any_element()
            }),
        );
        let pane = match self.sheet_tab {
            SheetTab::Overview => v_flex()
                .gap_4()
                .child(overview)
                .child(actions)
                .child(binding)
                .when(!is_telemetry, |this| this.child(work))
                .child(assurance)
                .when(!is_telemetry, |this| this.child(handoffs))
                .child(provenance)
                .child(verification)
                .into_any_element(),
            SheetTab::Activity => activity.into_any_element(),
            SheetTab::History => history_pane.into_any_element(),
        };
        v_flex()
            .flex_1()
            .min_h_0()
            .gap_2()
            .p_4()
            .child(tab_bar)
            .child(pane)
    }
    /// SNAP1 binding row: scope tag + title, shared by the live set and the frozen reference.
    fn binding_rule_row(&self, rule: &agent_control_core::model::Rule) -> AnyElement {
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
    }

    /// UX12 evidence row (descending timeline + constraint history): kind/agent/flip/time header
    /// with click-to-expand payload; no ascending spine.
    fn evidence_row(&self, cx: &mut Context<Self>, e: &EvidenceEntry) -> AnyElement {
        let muted = cx.theme().muted_foreground;
        let border = cx.theme().border;
        let (icon, tint) = ui::kind_visual(cx, e.kind);
        let eid = e.id;
        let expanded = self.sheet_payload == Some(eid);
        let flip = match e.kind {
            EvidenceKind::OwnershipChange | EvidenceKind::HandoffIn => {
                let from = e
                    .payload
                    .get("from")
                    .or_else(|| e.payload.get("from_agent"))
                    .and_then(|v| v.as_str());
                let to = e
                    .payload
                    .get("to")
                    .and_then(|v| v.as_str())
                    .unwrap_or(&e.agent_id);
                from.map(|f| format!("{f} → {to}"))
            }
            _ => None,
        };
        let kind_label = serde_json::to_value(e.kind)
            .ok()
            .and_then(|v| v.as_str().map(|s| s.to_string()))
            .unwrap_or_else(|| "event".to_string());
        let row = h_flex()
            .gap_2()
            .items_start()
            .child(Icon::new(icon).size(px(14.)).text_color(tint))
            .child(
                v_flex()
                    .flex_1()
                    .gap_1()
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .flex_wrap()
                            .child(ui::kind_tag(e.kind))
                            .child(Tag::secondary().child(e.agent_id.clone()))
                            .children(flip.map(|f| div().text_xs().text_color(tint).child(f)))
                            .child(
                                div()
                                    .text_xs()
                                    .text_color(muted)
                                    .child(local_time(e.timestamp)),
                            ),
                    )
                    .when(expanded, |this| {
                        this.child(
                            div()
                                .p_2()
                                .border_1()
                                .border_color(border)
                                .rounded_md()
                                .bg(cx.theme().group_box)
                                .child(ui::mono(
                                    cx,
                                    serde_json::to_string_pretty(&e.payload).unwrap_or_default(),
                                )),
                        )
                    }),
            );
        div()
            .id(format!("tl-{eid}"))
            .role(Role::Button)
            .aria_label(format!("#{seq} {kind_label}", seq = e.seq))
            .cursor_pointer()
            .on_click(cx.listener(move |this, _, _, cx| {
                this.sheet_payload = if this.sheet_payload == Some(eid) {
                    None
                } else {
                    Some(eid)
                };
                cx.notify();
            }))
            .child(row)
            .into_any_element()
    }
}
