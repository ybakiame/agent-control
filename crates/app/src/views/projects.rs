use agent_control_core::{ExportTarget, HookScope, HookTarget};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants};
use gpui_kit::component::checkbox::Checkbox;
use gpui_kit::component::input::Input;
use gpui_kit::component::list::ListItem;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::component::tag::Tag;
use gpui_kit::component::tree::tree;
use gpui_kit::component::{ActiveTheme as _, Icon, Sizable, StyledExt, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;
use uuid::Uuid;

use crate::console::{Console, TreeAction};
use crate::ui;

impl Console {
    pub fn render_projects(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let muted = cx.theme().muted_foreground;

        let labels = TreeMenuLabels {
            set_current: self.t("proj.set_current").to_string(),
            rename: self.t("proj.rename").to_string(),
            export: self.t("proj.export").to_string(),
            revoke: self.t("proj.revoke").to_string(),
            register_hooks: self.t("proj.hooks_register").to_string(),
            revoke_hooks: self.t("proj.hooks_revoke").to_string(),
            reveal: self.t("proj.reveal").to_string(),
            remove: self.t("proj.remove").to_string(),
            open: self.t("proj.ctx_open").to_string(),
            open_rule: self.t("proj.ctx_open_rule").to_string(),
            enable: self.t("rules.enable").to_string(),
            disable: self.t("rules.disable").to_string(),
        };

        let weak = cx.weak_entity();
        let render_weak = weak.clone();
        let menu_weak = weak.clone();
        let menu_labels = labels.clone();
        let project_tree = tree(
            &self.project_tree,
            move |ix, entry, selected, _window, _cx| {
                let node_id = entry.item().id.to_string();
                let label = entry.item().label.clone();
                let depth = entry.depth();
                let is_folder = entry.is_folder();
                let expanded = entry.is_expanded();
                let weak = render_weak.clone();
                let icon_name = if node_id.starts_with("project:") {
                    if expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::Folder
                    }
                } else if node_id.starts_with("contracts:") {
                    if expanded {
                        IconName::FolderOpen
                    } else {
                        IconName::FolderClosed
                    }
                } else if node_id.starts_with("rule:") {
                    IconName::FileText
                } else if node_id.starts_with("sessions:") {
                    IconName::Bot
                } else if node_id.starts_with("conflicts:") {
                    IconName::TriangleAlert
                } else {
                    IconName::File
                };
                ListItem::new(format!("proj-tree-node-{ix}"))
                    .accessibility_label(label.clone())
                    .selected(selected)
                    .child(
                        h_flex()
                            .gap_1()
                            .items_center()
                            .pl(px(depth as f32 * 16.))
                            .child(
                                h_flex()
                                    .size(px(16.))
                                    .flex_shrink_0()
                                    .items_center()
                                    .justify_center()
                                    .child(if is_folder {
                                        Icon::new(if expanded {
                                            IconName::ChevronDown
                                        } else {
                                            IconName::ChevronRight
                                        })
                                        .size(px(14.))
                                        .text_color(muted)
                                        .into_any_element()
                                    } else {
                                        div().size(px(16.)).into_any_element()
                                    }),
                            )
                            .child(Icon::new(icon_name).size(px(16.)).text_color(muted))
                            .child(div().text_sm().truncate().child(label.to_string())),
                    )
                    .on_click(move |_, window, cx| {
                        let _ = weak.update(cx, |console, ccx| {
                            console.handle_tree_click(&node_id, window, ccx);
                        });
                    })
            },
        )
        .context_menu(move |_ix, entry, menu, _window, cx| {
            let node_id = entry.item().id.to_string();
            let nid = node_id.clone();
            let enabled = menu_weak
                .read_with(cx, |c, _| {
                    c.tree_rule_enabled.get(&nid).copied().unwrap_or(true)
                })
                .unwrap_or(true);
            build_tree_menu(
                menu,
                menu_weak.clone(),
                node_id,
                enabled,
                menu_labels.clone(),
            )
        });

        let detail_card = self
            .selected_project
            .and_then(|id| self.projects.iter().find(|p| p.id == id))
            .map(|project| {
                let id = project.id;
                let is_current = self.current_project == Some(id);
                let (contracts, sessions) = self.project_stats.get(&id).copied().unwrap_or((0, 0));
                let registered_count = self.hook_statuses.iter().filter(|s| s.registered).count();
                let hooks_total = HookTarget::ALL.len();
                let hooks_summary = self.tr(
                    "proj.hooks_summary",
                    &[&registered_count.to_string(), &hooks_total.to_string()],
                );
                let hooks_tag = if registered_count == hooks_total {
                    Tag::success().child(hooks_summary)
                } else if registered_count > 0 {
                    Tag::warning().child(hooks_summary)
                } else {
                    Tag::secondary().child(hooks_summary)
                };
                let export_help_open = self.open_sections.contains("proj-export-help");
                let hooks_help_open = self.open_sections.contains("proj-hooks-help");

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
                            .items_center()
                            .justify_between()
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
                                        Button::new("project-rename")
                                            .label(self.t("proj.rename"))
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.open_rename_dialog(window, cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("project-reveal")
                                            .label(self.t("proj.reveal"))
                                            .small()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.reveal_project_contracts(cx);
                                                cx.notify();
                                            })),
                                    ),
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
                    .child(div().h(px(1.)).w_full().bg(cx.theme().border))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(ui::section_label(cx, self.t("proj.export_title")))
                                    .when(self.export_stale, |this| {
                                        this.child(
                                            Tag::warning().child(self.t("proj.export_stale")),
                                        )
                                    }),
                            )
                            .child(h_flex().gap_3().flex_wrap().children(
                                ExportTarget::ALL.iter().map(|t| {
                                    let (key, id) = match t {
                                        ExportTarget::Agents => {
                                            ("proj.target_agents", "export-target-agents")
                                        }
                                        ExportTarget::Claude => {
                                            ("proj.target_claude", "export-target-claude")
                                        }
                                        ExportTarget::Gemini => {
                                            ("proj.target_gemini", "export-target-gemini")
                                        }
                                        ExportTarget::Cursor => {
                                            ("proj.target_cursor", "export-target-cursor")
                                        }
                                    };
                                    let checked = self.export_targets.contains(t);
                                    let target = *t;
                                    Checkbox::new(id)
                                        .label(self.t(key))
                                        .checked(checked)
                                        .on_click(cx.listener(move |this, next, _, cx| {
                                            this.toggle_export_target(target, *next, cx);
                                        }))
                                        .into_any_element()
                                }),
                            ))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        Button::new("project-export")
                                            .label(self.t("proj.export"))
                                            .primary()
                                            .small()
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.export_native_for_selected_project(cx);
                                            })),
                                    )
                                    .child(
                                        Button::new("project-revoke")
                                            .label(self.t("proj.revoke"))
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.confirm_revoke_native(window, cx);
                                            })),
                                    ),
                            )
                            .child(ui::disclosure(
                                "proj-export-help",
                                self.t("proj.export_details"),
                                export_help_open,
                                v_flex()
                                    .gap_1()
                                    .pt_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(self.t("proj.export_targets_hint")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(self.t("proj.export_hint")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(self.t("proj.revoke_hint")),
                                    )
                                    .when(self.export_stale, |this| {
                                        this.child(
                                            div()
                                                .text_xs()
                                                .text_color(cx.theme().warning)
                                                .child(self.t("proj.export_stale_hint")),
                                        )
                                    }),
                                cx.listener(|this, _, _, cx| {
                                    this.toggle_section("proj-export-help", cx);
                                }),
                            )),
                    )
                    .child(div().h(px(1.)).w_full().bg(cx.theme().border))
                    .child(
                        v_flex()
                            .gap_2()
                            .child(
                                h_flex()
                                    .gap_2()
                                    .items_center()
                                    .child(ui::section_label(cx, self.t("proj.hooks_title")))
                                    .child(hooks_tag),
                            )
                            .child(h_flex().gap_3().flex_wrap().children(
                                HookTarget::ALL.iter().map(|t| {
                                    let target = *t;
                                    let (key, id) = match target {
                                        HookTarget::Claude => {
                                            ("proj.hook_target_claude", "hook-target-claude")
                                        }
                                        HookTarget::CodeBuddy => {
                                            ("proj.hook_target_codebuddy", "hook-target-codebuddy")
                                        }
                                        HookTarget::WorkBuddy => {
                                            ("proj.hook_target_workbuddy", "hook-target-workbuddy")
                                        }
                                        HookTarget::Qoder => {
                                            ("proj.hook_target_qoder", "hook-target-qoder")
                                        }
                                        HookTarget::QoderCn => {
                                            ("proj.hook_target_qoder_cn", "hook-target-qoder-cn")
                                        }
                                        HookTarget::Codex => {
                                            ("proj.hook_target_codex", "hook-target-codex")
                                        }
                                        HookTarget::Cursor => {
                                            ("proj.hook_target_cursor", "hook-target-cursor")
                                        }
                                        HookTarget::Antigravity => (
                                            "proj.hook_target_antigravity",
                                            "hook-target-antigravity",
                                        ),
                                        HookTarget::Kimi => {
                                            ("proj.hook_target_kimi", "hook-target-kimi")
                                        }
                                    };
                                    let checked = self.hook_targets.contains(&target);
                                    let status =
                                        self.hook_statuses.iter().find(|s| s.target == target);
                                    let registered = status.map(|s| s.registered).unwrap_or(false);
                                    let installed = status.map(|s| s.installed).unwrap_or(false);
                                    let user_scope = target.scope() == HookScope::User;
                                    h_flex()
                                        .gap_1()
                                        .items_center()
                                        .child(
                                            Checkbox::new(id)
                                                .label(self.t(key))
                                                .checked(checked)
                                                .on_click(cx.listener(move |this, next, _, cx| {
                                                    this.toggle_hook_target(target, *next, cx);
                                                })),
                                        )
                                        .when(user_scope, |this| {
                                            this.child(
                                                Tag::info().child(self.t("proj.hook_scope_user")),
                                            )
                                        })
                                        .when(registered, |this| {
                                            this.child(
                                                Tag::success()
                                                    .child(self.t("proj.hook_registered")),
                                            )
                                        })
                                        .when(!registered && installed, |this| {
                                            this.child(
                                                Tag::secondary()
                                                    .child(self.t("proj.hook_installed")),
                                            )
                                        })
                                        .into_any_element()
                                }),
                            ))
                            .child(
                                h_flex()
                                    .gap_2()
                                    .flex_wrap()
                                    .child(
                                        Button::new("project-hooks-register")
                                            .label(self.t("proj.hooks_register"))
                                            .primary()
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.register_hooks_for_selected_project(
                                                    window, cx,
                                                );
                                            })),
                                    )
                                    .child(
                                        Button::new("project-hooks-revoke")
                                            .label(self.t("proj.hooks_revoke"))
                                            .small()
                                            .on_click(cx.listener(|this, _, window, cx| {
                                                this.confirm_revoke_hooks(window, cx);
                                            })),
                                    ),
                            )
                            .child(ui::disclosure(
                                "proj-hooks-help",
                                self.t("proj.hooks_details"),
                                hooks_help_open,
                                v_flex()
                                    .gap_1()
                                    .pt_1()
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(self.t("proj.hooks_hint")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(self.t("proj.hooks_register_hint")),
                                    )
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(muted)
                                            .child(self.t("proj.hooks_revoke_hint")),
                                    ),
                                cx.listener(|this, _, _, cx| {
                                    this.toggle_section("proj-hooks-help", cx);
                                }),
                            )),
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
            v_flex()
                .flex_1()
                .min_h_0()
                .gap_1()
                .child(div().flex_1().min_h_0().child(project_tree))
                .child(
                    div()
                        .text_xs()
                        .text_color(muted)
                        .child(self.t("proj.tree_hint")),
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

/// Localized labels for the project tree context menu (captured once per render).
#[derive(Clone)]
struct TreeMenuLabels {
    set_current: String,
    rename: String,
    export: String,
    revoke: String,
    register_hooks: String,
    revoke_hooks: String,
    reveal: String,
    remove: String,
    open: String,
    open_rule: String,
    enable: String,
    disable: String,
}

/// One context-menu item that dispatches a [`TreeAction`] on the Console when clicked.
fn tree_item(
    weak: &WeakEntity<Console>,
    label: String,
    icon: IconName,
    action: TreeAction,
) -> PopupMenuItem {
    let weak = weak.clone();
    PopupMenuItem::new(label)
        .icon(Icon::new(icon))
        .on_click(move |_, window, cx| {
            if let Some(entity) = weak.upgrade() {
                entity.update(cx, |this, ccx| {
                    this.run_tree_action(action.clone(), window, ccx);
                });
            }
        })
}

/// Build the right-click menu for a tree node, shaped by what the node represents.
fn build_tree_menu(
    mut menu: PopupMenu,
    weak: WeakEntity<Console>,
    node_id: String,
    rule_enabled: bool,
    labels: TreeMenuLabels,
) -> PopupMenu {
    if let Some(rest) = node_id.strip_prefix("project:") {
        if let Ok(id) = Uuid::parse_str(rest) {
            menu = menu
                .item(tree_item(
                    &weak,
                    labels.set_current,
                    IconName::Check,
                    TreeAction::SetCurrent(id),
                ))
                .item(tree_item(
                    &weak,
                    labels.rename,
                    IconName::FileText,
                    TreeAction::Rename(id),
                ))
                .separator()
                .item(tree_item(
                    &weak,
                    labels.export,
                    IconName::ExternalLink,
                    TreeAction::Export(id),
                ))
                .item(tree_item(
                    &weak,
                    labels.revoke,
                    IconName::Close,
                    TreeAction::Revoke(id),
                ))
                .separator()
                .item(tree_item(
                    &weak,
                    labels.register_hooks,
                    IconName::Settings2,
                    TreeAction::RegisterHooks(id),
                ))
                .item(tree_item(
                    &weak,
                    labels.revoke_hooks,
                    IconName::Close,
                    TreeAction::RevokeHooks(id),
                ))
                .separator()
                .item(tree_item(
                    &weak,
                    labels.reveal,
                    IconName::FolderOpen,
                    TreeAction::Reveal(id),
                ))
                .item(tree_item(
                    &weak,
                    labels.remove,
                    IconName::Close,
                    TreeAction::Remove(id),
                ));
        }
    } else if let Some(rest) = node_id.strip_prefix("contracts:") {
        if let Ok(id) = Uuid::parse_str(rest) {
            menu = menu
                .item(tree_item(
                    &weak,
                    labels.open,
                    IconName::ChevronRight,
                    TreeAction::Open(node_id.clone()),
                ))
                .item(tree_item(
                    &weak,
                    labels.reveal,
                    IconName::FolderOpen,
                    TreeAction::Reveal(id),
                ))
                .item(tree_item(
                    &weak,
                    labels.export,
                    IconName::ExternalLink,
                    TreeAction::Export(id),
                ));
        }
    } else if let Some(rest) = node_id.strip_prefix("rule:") {
        if let Some((pid, rid)) = rest.split_once(':')
            && let Ok(id) = Uuid::parse_str(pid)
        {
            let (toggle_label, toggle_icon) = if rule_enabled {
                (labels.disable, IconName::Close)
            } else {
                (labels.enable, IconName::Check)
            };
            menu = menu
                .item(tree_item(
                    &weak,
                    labels.open_rule,
                    IconName::FileText,
                    TreeAction::Open(node_id.clone()),
                ))
                .item(tree_item(
                    &weak,
                    toggle_label,
                    toggle_icon,
                    TreeAction::ToggleRule(id, rid.to_string()),
                ));
        }
    } else if node_id.starts_with("sessions:") || node_id.starts_with("conflicts:") {
        menu = menu.item(tree_item(
            &weak,
            labels.open,
            IconName::ChevronRight,
            TreeAction::Open(node_id.clone()),
        ));
    }
    menu
}
