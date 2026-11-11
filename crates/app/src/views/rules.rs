use agent_control_core::model::Scope;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::console::{Console, Nav};
use crate::ui;

impl Console {
    pub fn render_rules(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let bg = cx.theme().background;
        let muted_bg = cx.theme().muted;

        let tab = if self.nav == Nav::Preferences {
            Scope::User
        } else {
            Scope::Project
        };
        self.rules_tab = tab;
        let visible_count = self.rules.iter().filter(|r| r.scope == tab).count();
        let project_blocked = tab == Scope::Project && self.current_project().is_none();

        let rule_rows = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.scope == tab)
            .map(|(index, rule)| {
                let selected = self.selected_rule == Some(index);
                let scope_tag = match rule.scope {
                    Scope::User => Tag::primary().child(self.t("scope.user.tag")),
                    Scope::Project => Tag::warning().child(self.t("scope.project.tag")),
                    Scope::Session => Tag::info().child(self.t("scope.session.tag")),
                };
                let enabled_tag = if rule.enabled {
                    Tag::success().child(self.t("rules.on"))
                } else {
                    Tag::secondary().child(self.t("rules.off"))
                };
                v_flex()
                    .id(format!("rule-row-{index}"))
                    .role(Role::Button)
                    .aria_label(rule.title.clone())
                    .p_3()
                    .gap_1()
                    .border_1()
                    .border_color(if selected { cx.theme().primary } else { border })
                    .rounded_md()
                    .bg(if selected { muted_bg } else { bg })
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.load_rule_into_editor(index, window, cx);
                        cx.notify();
                    }))
                    .child(div().font_medium().text_sm().child(rule.title.clone()))
                    .child(ui::mono(cx, rule.id.clone()))
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .child(scope_tag)
                            .child(enabled_tag)
                            .child(Tag::secondary().child(rule.kind.to_string())),
                    )
                    .into_any_element()
            });

        let selected_meta = self
            .selected_rule
            .and_then(|i| self.rules.get(i))
            .map(|rule| {
                let checksum_short = if rule.checksum.is_empty() {
                    "—".to_string()
                } else {
                    rule.checksum[..16.min(rule.checksum.len())].to_string()
                };
                ui::card(cx)
                    .p_3()
                    .child(div().text_xs().text_color(muted).child(self.tr(
                        "rules.meta",
                        &[
                            &rule.version.to_string(),
                            &rule.priority.to_string(),
                            &format!("{:?}", rule.source.kind),
                        ],
                    )))
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.tr("rules.checksum", &[&checksum_short])),
                    )
                    .into_any_element()
            });

        let enabled_switch = self
            .selected_rule
            .and_then(|i| self.rules.get(i))
            .map(|rule| {
                let enabled = rule.enabled;
                v_flex()
                    .gap_1()
                    .child(
                        Switch::new("rule-enabled-switch")
                            .checked(enabled)
                            .label(self.t("rules.enabled"))
                            .on_click(cx.listener(|this, next, window, cx| {
                                if this.set_selected_rule_enabled(*next, cx) {
                                    let msg = this.t(if *next {
                                        "rules.enabled_toast"
                                    } else {
                                        "rules.disabled_toast"
                                    });
                                    this.notify(window, cx, msg);
                                }
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.t("rules.enabled_hint")),
                    )
                    .into_any_element()
            });

        let tab_desc = if tab == Scope::Project {
            match self.current_project() {
                Some(p) => self.tr("rules.project_desc", &[&p.name]),
                None => self.t("rules.no_project").to_string(),
            }
        } else {
            self.t("rules.personal_desc").to_string()
        };

        let editor_body = if project_blocked {
            v_flex()
                .gap_2()
                .child(ui::empty_state(
                    cx,
                    IconName::Folder,
                    self.t("rules.project"),
                    self.t("rules.no_project"),
                ))
                .child(
                    Button::new("rules-go-projects")
                        .label(self.t("rules.go_projects"))
                        .small()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.nav = Nav::Projects;
                            this.refresh_data(cx);
                            cx.notify();
                        })),
                )
                .into_any_element()
        } else {
            v_flex()
                .gap_3()
                .child(
                    v_flex()
                        .gap_1()
                        .child(div().text_sm().text_color(muted).child(self.t("rules.id")))
                        .child(Input::new(&self.rule_id_input).id("rule-id-input")),
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
                        .child(Input::new(&self.rule_title_input).id("rule-title-input")),
                )
                .child(
                    v_flex()
                        .gap_1()
                        .child(
                            div()
                                .text_sm()
                                .text_color(muted)
                                .child(self.t("rules.topic")),
                        )
                        .child(Input::new(&self.rule_topic_input).id("rule-topic-input")),
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
                        .child(Textarea::new(&self.rule_content_input).h(px(160.))),
                )
                .children(selected_meta)
                .children(enabled_switch)
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            Button::new("rule-save")
                                .label(self.t("rules.save"))
                                .primary()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    if this.save_rule_from_inputs(cx) {
                                        this.notify(window, cx, this.t("rules.saved"));
                                    }
                                    cx.notify();
                                })),
                        )
                        .child(
                            Button::new("rule-delete")
                                .label(self.t("rules.delete"))
                                .danger()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.confirm_delete_rule(window, cx);
                                    cx.notify();
                                })),
                        ),
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
                                .child(ui::section_label(cx, self.t("rules.title")))
                                .child(Tag::secondary().child(visible_count.to_string())),
                        )
                        .child(if visible_count == 0 {
                            ui::empty_state(
                                cx,
                                IconName::BookOpen,
                                self.t("rules.title"),
                                tab_desc.clone(),
                            )
                            .into_any_element()
                        } else {
                            v_flex()
                                .flex_1()
                                .min_h_0()
                                .gap_2()
                                .id("rules-list")
                                .overflow_y_scroll()
                                .children(rule_rows)
                                .into_any_element()
                        }),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .gap_3()
                        .id("rules-editor")
                        .overflow_y_scroll()
                        .child(ui::section_label(cx, self.t("rules.editor")))
                        .child(div().text_sm().text_color(muted).child(tab_desc))
                        .child(editor_body),
                ),
        )
    }
}
