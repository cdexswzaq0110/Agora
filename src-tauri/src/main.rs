//! Agora desktop (Tauri). The window is a thin shell around the embedded HTTP server, so the
//! desktop, a browser and a paired phone all run the same UI against the same engine.
//!
//! - Closing the main window hides it to the tray: paired phones and running sessions keep
//!   working. Quit from the tray menu.
//! - The always-on-top usage widget (tray → 用量小工具) shows the HP bars.
//! - A native notification fires when a session finishes while the window is not focused.
//!
//! The pages are loaded from `http://127.0.0.1:<port>` with no Tauri IPC exposed to them.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use agora_core::{Engine, Event, Status, Store};
use agora_server::{serve, spawn_background, AppState};
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};
use tauri_plugin_notification::NotificationExt;
use tokio::sync::broadcast::error::RecvError;

static HIDE_HINT_SHOWN: AtomicBool = AtomicBool::new(false);

fn base_url(port: u16) -> String {
    format!("http://127.0.0.1:{port}")
}

fn open_main(app: &AppHandle, base: &str) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        return;
    }
    let Ok(url) = format!("{base}/?shell=desktop").parse() else { return };
    let _ = WebviewWindowBuilder::new(app, "main", WebviewUrl::External(url))
        .title("Agora")
        .inner_size(1320.0, 880.0)
        .min_inner_size(400.0, 600.0)
        .build();
}

fn toggle_widget(app: &AppHandle, base: &str) {
    if let Some(w) = app.get_webview_window("widget") {
        let _ = w.close();
        return;
    }
    let Ok(url) = format!("{base}/widget").parse() else { return };
    let _ = WebviewWindowBuilder::new(app, "widget", WebviewUrl::External(url))
        .title("Agora 用量")
        .inner_size(340.0, 176.0)
        .min_inner_size(260.0, 120.0)
        .always_on_top(true)
        .skip_taskbar(true)
        .build();
}

fn open_in_browser(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = std::process::Command::new("rundll32")
            .args(["url.dll,FileProtocolHandler", url])
            .creation_flags(0x0800_0000)
            .spawn();
    }
    #[cfg(target_os = "macos")]
    {
        let _ = std::process::Command::new("open").arg(url).spawn();
    }
    #[cfg(all(unix, not(target_os = "macos")))]
    {
        let _ = std::process::Command::new("xdg-open").arg(url).spawn();
    }
}

fn notify_on_finish(app: AppHandle, engine: Arc<Engine>) {
    let mut rx = engine.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            let focused = || app.get_webview_window("main").and_then(|w| w.is_focused().ok()).unwrap_or(false);
            match rx.recv().await {
                Ok(Event::Status { title, status, error, .. }) if status != Status::Running => {
                    if focused() {
                        continue;
                    }
                    let body = match status {
                        Status::Done => format!("完成：{title}"),
                        Status::Failed => format!("失敗：{title}{}", error.map(|e| format!("（{e}）")).unwrap_or_default()),
                        Status::Cancelled => format!("已取消：{title}"),
                        Status::AwaitingApproval => format!("方案已整理好，等你核准：{title}"),
                        Status::Running => continue,
                    };
                    let _ = app.notification().builder().title("Agora").body(body).show();
                }
                Ok(Event::Digest { status, date, .. }) if status == "done" => {
                    let _ = app.notification().builder().title("Agora · 每日技術情報").body(format!("{date} 的技術情報已整理好")).show();
                }
                Ok(_) | Err(RecvError::Lagged(_)) => {}
                Err(RecvError::Closed) => break,
            }
        }
    });
}

fn build_tray(app: &tauri::App, base: String) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "開啟 Agora", true, None::<&str>)?;
    let digest = MenuItem::with_id(app, "digest", "今日技術情報", true, None::<&str>)?;
    let widget = MenuItem::with_id(app, "widget", "用量小工具（置頂）", true, None::<&str>)?;
    let browser = MenuItem::with_id(app, "browser", "在瀏覽器開啟", true, None::<&str>)?;
    let sep = PredefinedMenuItem::separator(app)?;
    let quit = MenuItem::with_id(app, "quit", "結束 Agora", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &digest, &widget, &browser, &sep, &quit])?;
    let mut tray = TrayIconBuilder::with_id("agora").tooltip("Agora — 多模型協作平台").menu(&menu).show_menu_on_left_click(false);
    if let Some(icon) = app.default_window_icon() {
        tray = tray.icon(icon.clone());
    }
    let menu_base = base.clone();
    tray.on_menu_event(move |app, event| match event.id.as_ref() {
        "open" => open_main(app, &menu_base),
        "digest" => {
            open_main(app, &menu_base);
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.eval("location.hash = '#/digest'");
            }
        }
        "widget" => toggle_widget(app, &menu_base),
        "browser" => open_in_browser(&format!("{menu_base}/")),
        "quit" => app.exit(0),
        _ => {}
    })
    .on_tray_icon_event(move |tray, event| {
        if let TrayIconEvent::Click { button: MouseButton::Left, button_state: MouseButtonState::Up, .. } = event {
            open_main(tray.app_handle(), &base);
        }
    })
    .build(app)?;
    Ok(())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_notification::init())
        .setup(|app| {
            let store = Store::open(Store::default_root())?;
            let engine = Engine::new(store)?;
            let port = engine.settings().port;
            let base = base_url(port);
            // Bind synchronously so the window never races the server. If the port is taken,
            // another Agora (desktop or headless) is already serving: use it as a client.
            match std::net::TcpListener::bind(("127.0.0.1", port)) {
                Ok(listener) => {
                    listener.set_nonblocking(true)?;
                    let state = AppState::new(engine.clone(), port);
                    tauri::async_runtime::spawn(async move {
                        spawn_background(state.engine.clone());
                        match tokio::net::TcpListener::from_std(listener) {
                            Ok(l) => {
                                if let Err(e) = serve(state, l).await {
                                    eprintln!("Agora 伺服器停止：{e:#}");
                                }
                            }
                            Err(e) => eprintln!("Agora 無法啟動伺服器：{e}"),
                        }
                    });
                    notify_on_finish(app.handle().clone(), engine);
                }
                Err(e) => eprintln!("連接埠 {port} 已被使用（{e}），改為連到現有的 Agora"),
            }
            open_main(app.handle(), &base);
            build_tray(app, base)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
                if !HIDE_HINT_SHOWN.swap(true, Ordering::SeqCst) {
                    let _ = window
                        .app_handle()
                        .notification()
                        .builder()
                        .title("Agora 仍在系統匣執行")
                        .body("手機連線與進行中的議題不會中斷。要完全結束請在系統匣選「結束 Agora」。")
                        .show();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Agora 無法啟動");
}
