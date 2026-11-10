use gpui_kit::assets::IconName;
use gpui_kit::component::{ActiveTheme as _, Icon, StyledExt, h_flex, v_flex};
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
