#![windows_subsystem = "windows"]

mod console;
mod i18n;
mod theme;
mod ui;
mod views;

use std::sync::Arc;

use agent_control_acp::HarnessManager;
use agent_control_core::Store;
use gpui_kit::assets::Assets;
use gpui_kit::component::{Theme, ThemeMode, TitleBar};
use gpui_kit::*;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

use console::Console;

fn main() {
    let store = match Store::open_default() {
        Ok(store) => Arc::new(store),
        Err(e) => {
            eprintln!("failed to open store: {e}");
            std::process::exit(1);
        }
    };
    let _log_guard = init_logging(&store.layout, "app.log");
    let manager = match HarnessManager::new(store.clone()) {
        Ok(m) => Arc::new(m),
        Err(e) => {
            eprintln!("failed to create tokio runtime: {e}");
            std::process::exit(1);
        }
    };

    gpui_kit::application().with_assets(Assets).run(move |cx| {
        gpui_kit::init(cx);
        Theme::change(ThemeMode::Dark, None, cx);
        theme::apply_brand(cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::centered(size(px(1280.), px(800.)), cx)),
            titlebar: Some(TitlebarOptions {
                title: Some("Agent Control".into()),
                ..TitleBar::title_bar_options()
            }),
            ..TitleBar::window_options()
        };
        let store = store.clone();
        let manager = manager.clone();
        gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| Console::new(store, manager, window, cx))
        })
        .expect("failed to open window");
    });
}

/// Log to stderr (dev) and to `<data root>/logs/<name>` (visible even with no console).
/// The returned guard must outlive the process or the file writer thread stops.
fn init_logging(
    layout: &agent_control_core::DataLayout,
    name: &str,
) -> tracing_appender::non_blocking::WorkerGuard {
    let appender = tracing_appender::rolling::never(layout.logs_dir(), name);
    let (file_writer, guard) = tracing_appender::non_blocking(appender);
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(filter)
        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(file_writer)
                .with_ansi(false),
        )
        .init();
    guard
}
