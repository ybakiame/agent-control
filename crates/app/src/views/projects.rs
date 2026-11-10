use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::input::Input;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::tag::Tag;
use gpui_kit::component::tree::tree;
use gpui_kit::component::{ActiveTheme as _, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::console::Console;
use crate::ui;

impl Console {
    pub fn render_projects(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;

        let weak = cx.weak_entity();
        let project_tree = tree(
            &self.project_tree,
            move |ix, entry, selected, _window, _cx| {
                let node_id = entry.item().id.to_string();
                let label = entry.item().label.clone();
                let depth = entry.depth();
                let is_folder = entry.is_folder();
                let expanded = entry.is_expanded();
                let weak = weak.clone();
                ListItem::new(format!("proj-tree-node-{ix}"))
                    .accessibility_label(label.clone())
                    .selected(selected)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .pl(px(depth as f32 * 14.))
                            .child(div().w(px(12.)).text_xs().text_color(muted).child(
                                if is_folder {
                                    if expanded { "▾" } else { "▸" }
                                } else {
                                    ""
                                },
                            ))
                            .child(div().text_sm().child(label.to_string())),
                    )
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |console, ccx| {
                            console.handle_tree_click(&node_id, window, ccx);
                        });
                    })
            },
        );

        let detail_card = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|project| {
                let id = project.id;
                let is_current = self.current_project == Some(id);
                let (contracts, sessions) = self.project_stats.get(&id).copied().unwrap_or((0, 0));
                ui::card(cx)
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(ui::section_label(cx, self.t("proj.detail")))
                            .when(is_current, |this| {
                                this.child(Tag::success().child(self.t("proj.current")))
                            }),
                    )
                    .child(div().font_bold().text_lg().child(project.name.clone()))
                    .child(ui::kv(
                        cx,
                        self.t("proj.path_label"),
                        project.path.display().to_string(),
                    ))
                    .child(div().text_sm().child(self.tr(
                        "proj.counts",
                        &[&contracts.to_string(), &sessions.to_string()],
                    )))
                    .child(div().text_xs().text_color(muted).child(self.tr(
                        "proj.created",
                        &[&project.created_at.format("%Y-%m-%d %H:%M").to_string()],
                    )))
                    .child(div().text_xs().text_color(muted).child(self.tr(
                        "proj.opened",
                        &[&project.last_opened_at.format("%Y-%m-%d %H:%M").to_string()],
                    )))
                    .child(
                        h_flex()
                            .gap_2()
                            .flex_wrap()
                            .when(!is_current, |this| {
                                this.child(
                                    Button::new("project-set-current")
                                        .label(self.t("proj.set_current"))
                                        .primary()
                                        .small()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.set_current_project(id, cx);
                                            cx.notify();
                                        })),
                                )
                            })
                            .child(
                                Button::new("project-reveal")
                                    .label(self.t("proj.reveal"))
                                    .small()
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.reveal_project_contracts(cx);
                                        cx.notify();
                                    })),
                            )
                            .child(
                                Button::new("project-remove")
                                    .label(self.t("proj.remove"))
                                    .danger()
                                    .small()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.confirm_remove_project(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_sm()
                                    .text_color(muted)
                                    .child(self.t("proj.rename_title")),
                            )
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(
                                        div().flex_1().min_w_0().child(
                                            Input::new(&self.project_rename_input)
                                                .id("project-rename-input"),
                                        ),
                                    )
                                    .child(
                                        Button::new("project-rename-save")
                                            .label(self.t("proj.rename_save"))
                                            .small()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.rename_project_from_input(cx);
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                    .child(
                        div()
                            .text_xs()
                            .text_color(muted)
                            .child(self.t("proj.remove_hint")),
                    )
                    .into_any_element()
            });

        let onboarding = ui::card(cx)
            .child(ui::section_label(cx, self.t("proj.onboard_title")))
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.t("proj.onboard_desc")),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().text_color(muted).child(self.t("proj.path")))
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(div().flex_1().min_w_0().child(
                                Input::new(&self.project_path_input).id("project-path-input"),
                            ))
                            .child(
                                Button::new("project-browse")
                                    .label(self.t("proj.browse"))
                                    .small()
                                    .on_click(cx.listener(|this, _, window, cx| {
                                        this.browse_project_folder(window, cx);
                                        cx.notify();
                                    })),
                            ),
                    ),
            )
            .child(
                v_flex()
                    .gap_1()
                    .child(div().text_sm().text_color(muted).child(self.t("proj.name")))
                    .child(Input::new(&self.project_name_input).id("project-name-input")),
            )
            .child(
                Button::new("project-onboard")
                    .label(self.t("proj.onboard"))
                    .primary()
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.onboard_project(window, cx);
                        cx.notify();
                    })),
            );

        let guidance = ui::card(cx)
            .child(ui::section_label(cx, self.t("proj.guidance_title")))
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.t("proj.guidance1")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.t("proj.guidance2")),
            )
            .child(
                div()
                    .text_sm()
                    .text_color(muted)
                    .child(self.t("proj.guidance3")),
            );

        let right_content = if self.show_onboarding || self.projects.is_empty() {
            v_flex()
                .gap_3()
                .child(onboarding)
                .child(guidance)
                .into_any_element()
        } else if let Some(detail) = detail_card {
            detail
        } else {
            ui::empty_state(
                cx,
                IconName::Folder,
                self.t("proj.detail"),
                self.t("proj.select_hint"),
            )
            .into_any_element()
        };

        let tree_panel = if self.projects.is_empty() {
            ui::empty_state(
                cx,
                IconName::FolderOpen,
                self.t("proj.title"),
                self.t("proj.empty"),
            )
            .into_any_element()
        } else {
            div()
                .flex_1()
                .min_h_0()
                .child(project_tree)
                .into_any_element()
        };

        v_flex().flex_1().min_h_0().w_full().p_4().gap_3().child(
            h_flex()
                .flex_1()
                .min_h_0()
                .gap_3()
                .child(
                    v_flex()
                        .w(px(360.))
                        .h_full()
                        .gap_2()
                        .child(
                            h_flex()
                                .items_center()
                                .justify_between()
                                .child(ui::section_label(cx, self.t("proj.tree")))
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .child(
                                            Tag::secondary().child(self.projects.len().to_string()),
                                        )
                                        .child(
                                            Button::new("project-add")
                                                .label(self.t("proj.add"))
                                                .small()
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.show_onboarding = !this.show_onboarding;
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        )
                        .child(tree_panel),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .min_w_0()
                        .h_full()
                        .id("projects-detail")
                        .overflow_y_scroll()
                        .child(right_content),
                ),
        )
    }
}
