use agent_control_core::model::{Enforcement, Scope};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::{Input, Textarea};
use gpui_kit::component::menu::{ContextMenuExt as _, DropdownMenu, PopupMenu, PopupMenuItem};
use gpui_kit::component::switch::Switch;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::{ActiveTheme as _, Icon, Selectable, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::console::{Console, Nav};
use crate::ui;

impl Console {
    pub fn render_rules(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let border = cx.theme().border;
        let muted = cx.theme().muted_foreground;
        let bg = cx.theme().background;

        let tab = match self.nav {
            Nav::Preferences => Scope::User,
            Nav::Environment => Scope::Environment,
            _ => Scope::Project,
        };
        self.rules_tab = tab;
        let visible_count = self.rules.iter().filter(|r| r.scope == tab).count();
        let project_blocked =
            matches!(tab, Scope::Project | Scope::Environment) && self.current_project().is_none();

        let weak = cx.weak_entity();
        let rule_cards = self
            .rules
            .iter()
            .enumerate()
            .filter(|(_, rule)| rule.scope == tab)
            .map(|(index, rule)| {
                let scope_tag = match rule.scope {
                    Scope::User => Tag::primary().child(self.t("scope.user.tag")),
                    Scope::Project => Tag::warning().child(self.t("scope.project.tag")),
                    Scope::Session => Tag::info().child(self.t("scope.session.tag")),
                    Scope::Environment => Tag::secondary().child(self.t("scope.environment.tag")),
                };
                let enforcement_tag = match rule.enforcement {
                    Enforcement::Advisory => None,
                    Enforcement::Verify => Some(Tag::info().child(self.t("enforcement.verify"))),
                    Enforcement::Hard => Some(Tag::warning().child(self.t("enforcement.hard"))),
                };
                let enabled = rule.enabled;
                let title = rule.title.clone();
                let open_weak = weak.clone();
                let menu_weak = weak.clone();
                let ctx_weak = weak.clone();
                let switch_access = self.tr("rules.enable_switch", &[&title]);
                let edit_label = self.t("rules.edit").to_string();
                let toggle_label = self
                    .t(if enabled {
                        "rules.disable"
                    } else {
                        "rules.enable"
                    })
                    .to_string();
                let delete_label = self.t("rules.delete").to_string();
                let ctx_labels = (
                    edit_label.clone(),
                    toggle_label.clone(),
                    delete_label.clone(),
                );
                let menu_labels = (edit_label, toggle_label, delete_label);

                v_flex()
                    .id(format!("rule-row-{index}"))
                    .p_3()
                    .gap_2()
                    .border_1()
                    .border_color(border)
                    .rounded_md()
                    .bg(bg)
                    .context_menu(move |menu, _window, _cx| {
                        let (e, t, d) = ctx_labels.clone();
                        this_rule_menu(menu, ctx_weak.clone(), index, enabled, e, t, d)
                    })
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(
                                h_flex().flex_1().min_w_0().child(
                                    Button::new(format!("rule-open-{index}"))
                                        .ghost()
                                        .accessibility_label(title.clone())
                                        .child(div().font_medium().text_sm().child(title.clone()))
                                        .on_click(move |_, window, cx| {
                                            if let Some(entity) = open_weak.upgrade() {
                                                entity.update(cx, |this, ccx| {
                                                    this.open_rule_editor_dialog(
                                                        Some(index),
                                                        window,
                                                        ccx,
                                                    );
                                                });
                                            }
                                        }),
                                ),
                            )
                            .child(
                                Button::new(format!("rule-menu-{index}"))
                                    .icon(Icon::new(IconName::Ellipsis))
                                    .ghost()
                                    .small()
                                    .accessibility_label(self.t("rules.actions"))
                                    .dropdown_menu(move |menu, _window, _cx| {
                                        let (e, t, d) = menu_labels.clone();
                                        this_rule_menu(
                                            menu,
                                            menu_weak.clone(),
                                            index,
                                            enabled,
                                            e,
                                            t,
                                            d,
                                        )
                                    }),
                            ),
                    )
                    .child(ui::mono(cx, rule.id.clone()))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .justify_between()
                            .child(
                                h_flex()
                                    .gap_1()
                                    .items_center()
                                    .flex_wrap()
                                    .child(scope_tag)
                                    .children(enforcement_tag)
                                    .child(Tag::secondary().child(rule.kind.to_string())),
                            )
                            .child(
                                Switch::new(format!("rule-enabled-{index}"))
                                    .checked(enabled)
                                    .accessibility_label(switch_access)
                                    .tooltip(self.t("rules.enabled_hint"))
                                    .on_click(cx.listener(move |this, next, window, cx| {
                                        if this.set_rule_enabled_at(index, *next, cx) {
                                            let msg = this.t(if *next {
                                                "rules.enabled_toast"
                                            } else {
                                                "rules.disabled_toast"
                                            });
                                            this.notify(window, cx, msg);
                                        }
                                        cx.notify();
                                    })),
                            ),
                    )
                    .into_any_element()
            });

        let tab_desc = match tab {
            Scope::Project => match self.current_project() {
                Some(p) => self.tr("rules.project_desc", &[&p.name]),
                None => self.t("rules.no_project").to_string(),
            },
            Scope::Environment => match self.current_project() {
                Some(p) => self.tr("rules.environment_desc", &[&p.name]),
                None => self.t("rules.no_project").to_string(),
            },
            _ => self.t("rules.personal_desc").to_string(),
        };

        let body = if project_blocked {
            v_flex()
                .gap_2()
                .child(ui::empty_state(
                    cx,
                    IconName::Folder,
                    self.t(if tab == Scope::Environment {
                        "rules.environment"
                    } else {
                        "rules.project"
                    }),
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
        } else if visible_count == 0 {
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
                .children(rule_cards)
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
                    .items_center()
                    .justify_between()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(ui::section_label(cx, self.t("rules.title")))
                            .child(Tag::secondary().child(visible_count.to_string())),
                    )
                    .when(!project_blocked, |this| {
                        this.child(
                            Button::new("rule-new")
                                .icon(Icon::new(IconName::Plus))
                                .label(self.t("rules.new"))
                                .primary()
                                .small()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.open_rule_editor_dialog(None, window, cx);
                                })),
                        )
                    }),
            )
            .child(div().text_sm().text_color(muted).child(tab_desc))
            .child(body)
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(self.t("rules.card_hint")),
            )
    }

    /// The rule editor form, rendered inside an on-demand modal (never docked permanently).
    pub fn render_rule_editor_form(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;
        let scope = self
            .selected_rule
            .and_then(|i| self.rules.get(i))
            .map(|r| r.scope)
            .unwrap_or(self.rules_tab);

        let selected_meta = self
            .selected_rule
            .and_then(|i| self.rules.get(i))
            .map(|rule| {
                let checksum_short = if rule.checksum.is_empty() {
                    "—".to_string()
                } else {
                    rule.checksum[..16.min(rule.checksum.len())].to_string()
                };
                v_flex()
                    .gap_0p5()
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

        let enforcement_selector = v_flex()
            .gap_1()
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.t("rules.enforcement")),
            )
            .child(
                h_flex().gap_1().children(
                    [
                        Enforcement::Advisory,
                        Enforcement::Verify,
                        Enforcement::Hard,
                    ]
                    .into_iter()
                    .map(|level| {
                        let key = match level {
                            Enforcement::Advisory => "enforcement.advisory",
                            Enforcement::Verify => "enforcement.verify",
                            Enforcement::Hard => "enforcement.hard",
                        };
                        Button::new(format!("rule-enforcement-{}", level.label()))
                            .label(self.t(key))
                            .ghost()
                            .small()
                            .selected(self.rule_enforcement == level)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.set_rule_enforcement(level, cx);
                            }))
                            .into_any_element()
                    }),
                ),
            )
            .child(
                div()
                    .text_xs()
                    .text_color(muted)
                    .child(self.t("enforcement.hint")),
            );

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
                    .child(
                        Textarea::new(&self.rule_content_input)
                            .h(px(160.))
                            .aria_label(self.t("rules.content")),
                    ),
            )
            .children(selected_meta)
            .children(if scope == Scope::Environment {
                None
            } else {
                Some(enforcement_selector)
            })
    }
}

/// Shared action menu for a rule card (right-click context menu and the ellipsis dropdown).
fn this_rule_menu(
    mut menu: PopupMenu,
    weak: WeakEntity<Console>,
    index: usize,
    enabled: bool,
    edit_label: String,
    toggle_label: String,
    delete_label: String,
) -> PopupMenu {
    let edit = weak.clone();
    let toggle = weak.clone();
    let delete = weak.clone();
    menu = menu.item(
        PopupMenuItem::new(edit_label)
            .icon(Icon::new(IconName::FileText))
            .on_click(move |_, window, cx| {
                if let Some(entity) = edit.upgrade() {
                    entity.update(cx, |this, ccx| {
                        this.open_rule_editor_dialog(Some(index), window, ccx);
                    });
                }
            }),
    );
    menu = menu.item(
        PopupMenuItem::new(toggle_label).on_click(move |_, window, cx| {
            if let Some(entity) = toggle.upgrade() {
                entity.update(cx, |this, ccx| {
                    if this.set_rule_enabled_at(index, !enabled, ccx) {
                        let msg = this.t(if !enabled {
                            "rules.enabled_toast"
                        } else {
                            "rules.disabled_toast"
                        });
                        this.notify(window, ccx, msg);
                    }
                    ccx.notify();
                });
            }
        }),
    );
    menu.separator().item(
        PopupMenuItem::new(delete_label)
            .icon(Icon::new(IconName::Close))
            .on_click(move |_, window, cx| {
                if let Some(entity) = delete.upgrade() {
                    entity.update(cx, |this, ccx| {
                        this.selected_rule = Some(index);
                        this.confirm_delete_rule(window, ccx);
                        ccx.notify();
                    });
                }
            }),
    )
}
