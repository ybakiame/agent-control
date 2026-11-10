//! Agent Control brand theme. A dark, low-glare "command console" palette anchored by a
//! violet-indigo primary, applied on top of gpui-kit's dark mode via semantic tokens so every
//! component (buttons, sidebar, tables, dialogs, notifications) inherits the brand consistently.

use gpui_kit::component::Theme;
use gpui_kit::{App, Hsla, px, rgb};

/// Brand anchor (violet-indigo). Exposed for the wordmark glyph and any bespoke accent surfaces.
pub const BRAND_PRIMARY: u32 = 0x7c6af0;

fn h(hex: u32) -> Hsla {
    rgb(hex).into()
}

/// Apply the Agent Control brand over the current (dark) theme. Call once after `Theme::change`.
pub fn apply_brand(cx: &mut App) {
    Theme::update(cx, |t| {
        // Primary / accent — the brand violet.
        t.primary = h(BRAND_PRIMARY);
        t.primary_hover = h(0x8d7df5);
        t.primary_active = h(0x6b59e0);
        t.primary_foreground = h(0xffffff);
        t.accent = h(0x23204a);
        t.accent_foreground = h(0xd9d4ff);
        t.ring = h(BRAND_PRIMARY);
        t.selection = h(0x2c2856);
        t.link = h(0x9d90ff);
        t.link_hover = h(0xb3a8ff);
        t.link_active = h(0x8d7df5);

        // Neutral surfaces — deep slate, layered for depth.
        t.background = h(0x0e1116);
        t.foreground = h(0xe6e9ef);
        t.muted = h(0x1a2029);
        t.muted_foreground = h(0x8b93a3);
        t.border = h(0x262d38);
        t.input = h(0x2a323e);
        t.caret = h(BRAND_PRIMARY);
        t.secondary = h(0x1c232d);
        t.secondary_hover = h(0x232c38);
        t.secondary_active = h(0x2a3441);
        t.secondary_foreground = h(0xc4cbd8);
        t.popover = h(0x161b22);
        t.popover_foreground = h(0xe6e9ef);
        t.group_box = h(0x141922);
        t.group_box_foreground = h(0xe6e9ef);

        // Default buttons read as subtle raised surfaces.
        t.button = h(0x1c232d);
        t.button_hover = h(0x232c38);
        t.button_active = h(0x2a3441);
        t.button_foreground = h(0xd6dbe4);

        // Semantic state colors.
        t.success = h(0x2f9e5f);
        t.success_hover = h(0x37b06c);
        t.success_active = h(0x288752);
        t.success_foreground = h(0xffffff);
        t.warning = h(0xd99a2b);
        t.warning_foreground = h(0x1a1305);
        t.danger = h(0xe0524a);
        t.danger_hover = h(0xe8645c);
        t.danger_active = h(0xc8433c);
        t.danger_foreground = h(0xffffff);
        t.info = h(0x3f97d6);
        t.info_foreground = h(0xffffff);

        // Sidebar — slightly lifted from the content background.
        t.sidebar = h(0x12161d);
        t.sidebar_foreground = h(0xb9c0cd);
        t.sidebar_border = h(0x232a35);
        t.sidebar_accent = h(0x23204a);
        t.sidebar_accent_foreground = h(0xe9e6ff);
        t.sidebar_primary = h(BRAND_PRIMARY);
        t.sidebar_primary_foreground = h(0xffffff);

        // Lists & tables.
        t.colors.list = h(0x12161d);
        t.list_head = h(0x161b23);
        t.list_hover = h(0x1a212b);
        t.list_active = h(0x23204a);
        t.list_active_border = h(0x3a3470);
        t.list_even = h(0x141922);
        t.table = h(0x12161d);
        t.table_head = h(0x161b23);
        t.table_head_foreground = h(0x9aa2b1);
        t.table_hover = h(0x1a212b);
        t.table_active = h(0x23204a);
        t.table_active_border = h(0x3a3470);
        t.table_even = h(0x141922);
        t.table_row_border = h(0x202732);
        t.table_foot = h(0x161b23);
        t.table_foot_foreground = h(0x9aa2b1);

        // Tabs.
        t.tab = h(0x12161d);
        t.tab_bar = h(0x12161d);
        t.tab_bar_segmented = h(0x1a2029);
        t.tab_foreground = h(0x8b93a3);
        t.tab_active = h(0x23204a);
        t.tab_active_foreground = h(0xe9e6ff);

        // Chrome.
        t.title_bar = h(0x10141a);
        t.title_bar_border = h(0x232a35);
        t.status_bar = h(0x10141a);
        t.status_bar_border = h(0x232a35);
        t.scrollbar = h(0x0e1116);
        t.scrollbar_thumb = h(0x2a323e);
        t.scrollbar_thumb_hover = h(0x3a4451);

        // Shape — soft, modern radii.
        t.radius = px(8.);
        t.radius_lg = px(12.);
    });
}
