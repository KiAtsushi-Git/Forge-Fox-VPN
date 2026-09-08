mod commands;
mod config;
mod vpn;
mod admin;

use commands::AppState;
use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    Manager, WindowEvent,
};

pub fn run() {
    env_logger::init();

    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .plugin(tauri_plugin_notification::init())
        .manage(AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::get_settings,
            commands::update_settings,
            commands::update_theme,
            commands::get_servers,
            commands::add_server,
            commands::update_server,
            commands::delete_server,
            commands::add_subscription,
            commands::update_subscription,
            commands::delete_subscription,
            commands::parse_ssh_link,
            commands::add_rule,
            commands::update_rule,
            commands::delete_rule,
            commands::toggle_rule,
            commands::get_rules,
            commands::clear_rules,
            commands::set_split_enabled,
            commands::set_split_mode,
            commands::get_split_status,
            commands::list_running_apps,
            commands::export_rules,
            commands::import_rules,
            commands::vpn_connect,
            commands::vpn_disconnect,
            commands::vpn_get_state,
            commands::ping_server,
            commands::fetch_subscription,
            commands::get_logs,
            commands::clear_logs,
            commands::sh_install_host,
            commands::sh_install_provider,
            commands::sh_clean_host,
            commands::sh_list_users,
            commands::sh_add_user,
            commands::sh_del_user,
            commands::sh_reset_password,
            commands::sh_get_stats,
        ])
        .setup(|app| {
            // System tray
            let show = MenuItem::with_id(app, "show", "Показать", true, None::<&str>)?;
            let hide = MenuItem::with_id(app, "hide", "Скрыть", true, None::<&str>)?;
            let sep  = tauri::menu::PredefinedMenuItem::separator(app)?;
            let quit = MenuItem::with_id(app, "quit", "Выход", true, None::<&str>)?;

            let menu = Menu::with_items(app, &[&show, &hide, &sep, &quit])?;

            let _ = TrayIconBuilder::new()
                .icon(app.default_window_icon().unwrap().clone())
                .menu(&menu)
                .tooltip("ForgeFox VPN")
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "show" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                    "hide" => {
                        if let Some(win) = app.get_webview_window("main") {
                            let _ = win.hide();
                        }
                    }
                    "quit" => {
                        vpn::stop();
                        app.exit(0);
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let TrayIconEvent::Click {
                        button: MouseButton::Left,
                        button_state: MouseButtonState::Up,
                        ..
                    } = event
                    {
                        if let Some(win) = tray.app_handle().get_webview_window("main") {
                            let _ = win.show();
                            let _ = win.set_focus();
                        }
                    }
                })
                .build(app)?;

            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                let settings = config::get_settings();
                if settings.minimize_to_tray {
                    api.prevent_close();
                    let _ = window.hide();
                } else {
                    vpn::stop();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("Error running ForgeFox VPN");
}
