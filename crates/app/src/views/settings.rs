use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Selectable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::console::Console;
use crate::i18n::Locale;
use crate::ui;

impl Console {
    pub fn render_settings(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let bg = cx.theme().background;

        let mcp_status = if self.mcp_running && self.mcp_external {
            Tag::success().child(self.t("mcp.daemon"))
        } else if self.mcp_running {
            Tag::success().child(self.t("mcp.running"))
        } else {
            Tag::danger().child(self.t("mcp.stopped"))
        };
        let mcp_button = if self.mcp_running {
            Button::new("mcp-stop")
                .label(self.t("set.mcp.stop"))
                .danger()
                .small()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.stop_mcp_http();
                    this.log(this.t("log.mcp_stopped"));
                    cx.notify();
                }))
        } else {
            Button::new("mcp-start")
                .label(self.t("set.mcp.start"))
                .primary()
                .small()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.start_mcp_http();
                    this.log(this.t("log.mcp_starting"));
                    cx.notify();
                }))
        };

        let agent_rows = self.agents.iter().map(|agent| {
            let command = if agent.args.is_empty() {
                agent.command.clone()
            } else {
                format!("{} {}", agent.command, agent.args.join(" "))
            };
            v_flex()
                .p_3()
                .gap_1()
                .border_1()
                .border_color(border)
                .rounded_md()
                .bg(bg)
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(div().font_bold().text_sm().child(agent.name.clone()))
                        .child(div().text_xs().text_color(muted).child(agent.id.clone())),
                )
                .child(div().text_xs().text_color(muted).child(command))
                .into_any_element()
        });

        let layout = &self.store.layout;
        let paths = [
            (self.t("set.path.root"), layout.root.display().to_string()),
            (
                self.t("set.path.db"),
                layout.db_path().display().to_string(),
            ),
            (
                self.t("set.path.user_rules"),
                layout.user_rules_dir().display().to_string(),
            ),
            (
                self.t("set.path.agents"),
                layout.agents_file().display().to_string(),
            ),
            (
                self.t("set.path.bundles"),
                layout.bundles_dir().display().to_string(),
            ),
            (
                self.t("set.path.logs"),
                layout.logs_dir().display().to_string(),
            ),
        ];
        let path_rows = paths.iter().map(|(label, value)| {
            v_flex()
                .gap_0p5()
                .child(div().text_xs().text_color(muted).child(*label))
                .child(ui::mono(cx, value.clone()))
                .into_any_element()
        });

        v_flex()
            .flex_1()
            .min_h_0()
            .w_full()
            .p_4()
            .gap_3()
            .id("settings-scroll")
            .overflow_y_scroll()
            .overflow_x_hidden()
            .child(
                ui::card(cx)
                    .child(ui::section_label(cx, self.t("set.language")))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.t("set.language.desc")),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("lang-en")
                                    .label(self.t("lang.en"))
                                    .ghost()
                                    .small()
                                    .selected(self.locale == Locale::En)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_locale(Locale::En, window, cx);
                                    })),
                            )
                            .child(
                                Button::new("lang-zh")
                                    .label(self.t("lang.zh"))
                                    .ghost()
                                    .small()
                                    .selected(self.locale == Locale::Zh)
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.set_locale(Locale::Zh, window, cx);
                                    })),
                            ),
                    ),
            )
            .child(
                ui::card(cx)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(ui::section_label(cx, self.t("set.mcp")))
                            .child(mcp_status),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(self.tr("set.mcp.endpoint", &[&self.mcp_bind])),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.t("set.mcp.desc")),
                    )
                    .child(mcp_button),
            )
            .child(
                ui::card(cx)
                    .child(
                        h_flex()
                            .items_center()
                            .justify_between()
                            .child(ui::section_label(cx, self.t("set.agents")))
                            .child(Tag::secondary().child(self.agents.len().to_string())),
                    )
                    .child(v_flex().gap_2().children(agent_rows))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.t("set.agents.desc")),
                    ),
            )
            .child(
                ui::card(cx)
                    .child(ui::section_label(cx, self.t("set.layout")))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.t("set.layout.desc")),
                    )
                    .child(v_flex().gap_2().children(path_rows)),
            )
            .child(
                ui::card(cx)
                    .child(ui::section_label(cx, self.t("set.layers")))
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(self.t("set.layer1")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(self.t("set.layer2")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(self.t("set.layer3")),
                    )
                    .child(
                        div()
                            .text_sm()
                            .text_color(muted)
                            .child(self.t("set.layer4")),
                    ),
            )
    }
}
