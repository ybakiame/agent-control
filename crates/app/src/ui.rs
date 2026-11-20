use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::collapsible::Collapsible;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

/// Standard card surface used across views.
pub fn card(cx: &App) -> Div {
    v_flex()
        .p_4()
        .gap_2()
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().group_box)
}

/// Section heading with a brand accent bar.
pub fn section_label(cx: &App, text: impl Into<SharedString>) -> Div {
    h_flex()
        .gap_2()
        .items_center()
        .child(
            div()
                .w(px(3.))
                .h(px(14.))
                .rounded_full()
                .bg(cx.theme().primary),
        )
        .child(div().text_sm().font_semibold().child(text.into()))
}

/// A controlled disclosure section: a ghost trigger button (chevron + label) that the caller wires
/// to toggle state, wrapping a collapsible content panel with a spring reveal. Keeps verbose help
/// text out of the primary view while leaving the trigger keyboard- and UIA-operable.
pub fn disclosure(
    key: &str,
    label: impl Into<SharedString>,
    open: bool,
    content: impl IntoElement,
    on_toggle: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
) -> Collapsible {
    let label = label.into();
    let trigger = Button::new(format!("disclosure-{key}"))
        .ghost()
        .small()
        .icon(Icon::new(if open {
            IconName::ChevronDown
        } else {
            IconName::ChevronRight
        }))
        .label(label.clone())
        .accessibility_label(label)
        .on_click(on_toggle);
    // No motion_id: gpui's MotionReveal keeps closed content mounted AND prepainted, which leaks
    // it into the accessibility tree. Unmounting when closed keeps screen readers on the summary.
    Collapsible::new()
        .open(open)
        .w_full()
        .content(div().w_full().child(content))
        .child(trigger)
}

/// Centered empty-state placeholder with icon, title and description.
pub fn empty_state(
    cx: &App,
    icon: IconName,
    title: impl Into<SharedString>,
    desc: impl Into<SharedString>,
) -> Div {
    v_flex()
        .flex_1()
        .min_h(px(160.))
        .items_center()
        .justify_center()
        .gap_2()
        .p_8()
        .child(
            h_flex()
                .size(px(44.))
                .rounded_full()
                .bg(cx.theme().muted)
                .items_center()
                .justify_center()
                .child(
                    Icon::new(icon)
                        .size_5()
                        .text_color(cx.theme().muted_foreground),
                ),
        )
        .child(div().text_sm().font_medium().child(title.into()))
        .child(
            div()
                .text_xs()
                .text_color(cx.theme().muted_foreground)
                .max_w(px(380.))
                .text_center()
                .child(desc.into()),
        )
}

/// Dashboard metric card: tinted icon, big value, muted label.
pub fn stat_card(cx: &App, icon: IconName, label: &str, value: String, tint: Hsla) -> Div {
    h_flex()
        .p_4()
        .gap_3()
        .items_center()
        .flex_1()
        .min_w(px(170.))
        .border_1()
        .border_color(cx.theme().border)
        .rounded(cx.theme().radius_lg)
        .bg(cx.theme().group_box)
        .child(
            h_flex()
                .size(px(36.))
                .rounded(cx.theme().radius)
                .bg(tint.opacity(0.16))
                .items_center()
                .justify_center()
                .child(Icon::new(icon).size_4().text_color(tint)),
        )
        .child(
            v_flex()
                .gap_0p5()
                .child(div().text_xl().font_bold().child(value))
                .child(
                    div()
                        .text_xs()
                        .text_color(cx.theme().muted_foreground)
                        .child(label.to_string()),
                ),
        )
}

/// Key/value row for detail panels.
pub fn kv(cx: &App, key: &str, value: impl Into<SharedString>) -> Div {
    h_flex()
        .gap_2()
        .items_start()
        .justify_between()
        .text_sm()
        .child(
            div()
                .flex_shrink_0()
                .text_color(cx.theme().muted_foreground)
                .child(key.to_string()),
        )
        .child(div().min_w_0().text_right().child(value.into()))
}

/// Small colored status dot.
pub fn dot(color: Hsla) -> Div {
    div().size(px(7.)).rounded_full().bg(color)
}

/// Monospace hash/id snippet.
pub fn mono(cx: &App, text: impl Into<SharedString>) -> Div {
    div()
        .font_family(cx.theme().mono_font_family.clone())
        .text_xs()
        .child(text.into())
}

/// Lifecycle status tag shared by the sessions list and the session sheet.
pub fn status_tag(
    status: agent_control_core::model::SessionStatus,
    locale: crate::i18n::Locale,
) -> gpui_kit::component::tag::Tag {
    use agent_control_core::model::SessionStatus;
    use gpui_kit::component::tag::Tag;
    match status {
        SessionStatus::Active => Tag::success().child(crate::i18n::t(locale, "sess.status.active")),
        SessionStatus::HandedOff => {
            Tag::info().child(crate::i18n::t(locale, "sess.status.handed_off"))
        }
        SessionStatus::Completed => {
            Tag::secondary().child(crate::i18n::t(locale, "sess.status.completed"))
        }
        SessionStatus::Failed => Tag::danger().child(crate::i18n::t(locale, "sess.status.failed")),
    }
}

/// Work-item status tag shared by the sessions list and the session sheet.
pub fn work_status_tag(
    status: agent_control_core::model::WorkItemStatus,
    locale: crate::i18n::Locale,
) -> gpui_kit::component::tag::Tag {
    use agent_control_core::model::WorkItemStatus;
    use gpui_kit::component::tag::Tag;
    match status {
        WorkItemStatus::Done => Tag::success().child(crate::i18n::t(locale, "work.done")),
        WorkItemStatus::Doing => Tag::primary().child(crate::i18n::t(locale, "work.doing")),
        WorkItemStatus::Todo => Tag::secondary().child(crate::i18n::t(locale, "work.todo")),
    }
}

/// Evidence-kind tag (protocol kind labels, monospace semantics) shared by the sheet timeline
/// and evidence list.
pub fn kind_tag(kind: agent_control_core::model::EvidenceKind) -> gpui_kit::component::tag::Tag {
    use agent_control_core::model::EvidenceKind;
    use gpui_kit::component::tag::Tag;
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
        EvidenceKind::WorkStateUpdate => Tag::primary().child("work_state_update"),
        EvidenceKind::StatusChange => Tag::secondary().child("status_change"),
        EvidenceKind::OwnershipChange => Tag::info().child("ownership_change"),
        EvidenceKind::SessionRename => Tag::primary().child("session_rename"),
        EvidenceKind::SnapshotRefresh => Tag::info().child("snapshot_refresh"),
    }
}

/// Human-facing session label: title when set, else the machine task (UX7).
pub fn display_title(session: &agent_control_core::model::Session) -> String {
    if session.title.is_empty() {
        session.task.clone()
    } else {
        session.title.clone()
    }
}

/// Session list kind (UX1/UX8): hook telemetry vs continuity threads vs plain work.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum SessionKind {
    All,
    Work,
    Continuity,
    Telemetry,
}

pub fn session_kind(session: &agent_control_core::model::Session) -> SessionKind {
    match session.kind() {
        agent_control_core::model::SessionKind::Telemetry => SessionKind::Telemetry,
        agent_control_core::model::SessionKind::Continuity => SessionKind::Continuity,
        agent_control_core::model::SessionKind::Work => SessionKind::Work,
    }
}

/// Timeline icon + tint per evidence kind (session Sheet spine).
pub fn kind_visual(cx: &App, kind: agent_control_core::model::EvidenceKind) -> (IconName, Hsla) {
    use agent_control_core::model::EvidenceKind;
    let t = cx.theme();
    match kind {
        EvidenceKind::SessionStart => (IconName::Play, t.success),
        EvidenceKind::SessionEnd => (IconName::Square, t.muted_foreground),
        EvidenceKind::RuleAcknowledgment => (IconName::Check, t.primary),
        EvidenceKind::ToolCall => (IconName::SquareTerminal, t.muted_foreground),
        EvidenceKind::ToolGate => (IconName::Square, t.warning),
        EvidenceKind::FileChange => (IconName::File, t.warning),
        EvidenceKind::TestResult => (IconName::Check, t.success),
        EvidenceKind::HandoffOut => (IconName::ExternalLink, t.info),
        EvidenceKind::HandoffIn => (IconName::Link, t.info),
        EvidenceKind::ConstraintViolation => (IconName::TriangleAlert, t.danger),
        EvidenceKind::WorkStateUpdate => (IconName::FileText, t.primary),
        EvidenceKind::StatusChange => (IconName::RefreshCw, t.info),
        EvidenceKind::OwnershipChange => (IconName::User, t.info),
        EvidenceKind::SessionRename => (IconName::FileText, t.primary),
        EvidenceKind::SnapshotRefresh => (IconName::RefreshCw, t.info),
    }
}
