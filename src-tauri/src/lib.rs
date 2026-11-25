mod backup;
mod commands;
mod logging;
mod scheduler;
mod sound;
mod state;
mod sync_server;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use ltd_core::db;
#[cfg(desktop)]
use tauri::menu::{Menu, MenuItem};
#[cfg(desktop)]
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(desktop)]
use tauri::{AppHandle, Emitter};
use tauri::Manager;
#[cfg(desktop)]
use tauri::WindowEvent;
#[cfg(desktop)]
use tauri_plugin_window_state::StateFlags;

use state::AppState;

pub fn run() {
    let builder = tauri::Builder::default();
    #[cfg(desktop)]
    let builder = builder
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            show_main(app);
        }))
        .plugin(
            tauri_plugin_window_state::Builder::default()
                .with_state_flags(StateFlags::all() & !StateFlags::VISIBLE)
                .build(),
        )
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            Some(vec!["--autostart"]),
        ));

    builder
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let data_dir = app.path().app_data_dir()?;
            std::fs::create_dir_all(&data_dir)?;
            let log_dir = data_dir.join("logs");
            let _ = logging::init(&log_dir);
            let db_path = data_dir.join("local-to-do.db");
            let db = db::Db::open(&db_path)?;
            let backup_dir = data_dir.join("backups");
            let state = Arc::new(AppState::new(db, data_dir.clone(), backup_dir, log_dir));
            app.manage(state.clone());

            // 同步开机自启的真实状态（用户可能在系统设置里改过）
            {
                #[allow(unused_mut)]
                let mut settings = {
                    let conn = state.db.lock();
                    db::load_settings(&conn)?
                };
                #[cfg(desktop)]
                {
                    use tauri_plugin_autostart::ManagerExt;
                    if let Ok(enabled) = app.autolaunch().is_enabled() {
                        if settings.autostart != enabled {
                            settings.autostart = enabled;
                            let conn = state.db.lock();
                            db::save_settings(&conn, &settings)?;
                        }
                    }
                }
                if let Some(w) = app.get_webview_window("main") {
                    let theme = if settings.theme == "light" {
                        tauri::Theme::Light
                    } else {
                        tauri::Theme::Dark
                    };
                    let _ = w.set_theme(Some(theme));
                }
                #[cfg(desktop)]
                {
                    let launched_by_autostart = std::env::args().any(|a| a == "--autostart");
                    if launched_by_autostart && settings.start_minimized {
                        if let Some(w) = app.get_webview_window("main") {
                            let _ = w.hide();
                        }
                    }
                }
            }

            #[cfg(windows)]
            ensure_notification_identity();

            #[cfg(desktop)]
            setup_tray(app.handle())?;
            scheduler::spawn(app.handle().clone(), state.clone());
            logging::info(format!(
                "应用启动：数据目录 {}，数据库 {}",
                data_dir.display(),
                db_path.display()
            ));
            Ok(())
        })
        .on_window_event(|_window, _event| {
            #[cfg(desktop)]
            if let WindowEvent::CloseRequested { api, .. } = _event {
                let app = _window.app_handle();
                let state = app.state::<Arc<AppState>>();
                let close_to_tray = {
                    let conn = state.db.lock();
                    db::load_settings(&conn)
                        .map(|s| s.close_to_tray)
                        .unwrap_or(true)
                };
                if close_to_tray {
                    api.prevent_close();
                    let _ = _window.hide();
                    logging::info("窗口已隐藏到系统托盘（提醒调度继续运行）");
                } else {
                    logging::info("用户关闭应用窗口，应用即将退出");
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::list_lists,
            commands::create_list,
            commands::update_list,
            commands::delete_list,
            commands::reorder_lists,
            commands::list_deleted_lists,
            commands::restore_list,
            commands::purge_list,
            commands::list_tasks,
            commands::calendar_month,
            commands::list_tasks_for_day,
            commands::list_notified_tasks,
            commands::get_task,
            commands::create_task,
            commands::update_task,
            commands::set_important,
            commands::set_my_day,
            commands::move_task,
            commands::complete_task,
            commands::delete_task,
            commands::restore_task,
            commands::purge_task,
            commands::empty_deleted,
            commands::reorder_tasks,
            commands::sidebar_counts,
            commands::add_step,
            commands::update_step,
            commands::delete_step,
            commands::list_history,
            commands::list_task_history,
            commands::list_notes,
            commands::get_note,
            commands::create_note,
            commands::update_note,
            commands::delete_note,
            commands::get_settings,
            commands::update_settings,
            commands::reset_settings,
            commands::resync_reminders,
            commands::get_app_info,
            commands::get_db_stats,
            commands::send_test_notification,
            commands::preview_sound,
            commands::check_sound_file,
            commands::get_next_reminder,
            commands::create_backup,
            commands::list_backups,
            commands::export_json_file,
            commands::export_csv_file,
            commands::restore_backup,
            commands::validate_backup,
            commands::open_data_dir,
            commands::open_backup_dir,
            commands::sync_server_status,
            commands::sync_server_start,
            commands::sync_server_stop,
            commands::sync_create_pairing,
            commands::sync_cancel_pairing,
            commands::sync_list_devices,
            commands::sync_revoke_device,
            commands::sync_reset_pairing,
        ])
        .build(tauri::generate_context!())
        .expect("Local To Do 启动失败")
        .run(|app_handle, event| {
            // 应用退出时确保同步服务停止并释放监听端口
            if matches!(event, tauri::RunEvent::Exit) {
                let state = app_handle.state::<Arc<AppState>>();
                let _ = state.sync.stop();
            }
        });
}

#[cfg(desktop)]
fn show_main(app: &AppHandle) {
    if let Some(w) = app.get_webview_window("main") {
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
    }
}

/// 在注册表中登记 AUMID，使 Windows 通知中心可以接收本应用的原生通知。
/// 未安装的 Win32 程序没有开始菜单快捷方式时，系统会静默丢弃 Toast。
#[cfg(windows)]
fn ensure_notification_identity() {
    use windows::core::HSTRING;
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_WRITE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    fn set_string(key: HKEY, name: &str, value: &str) {
        let wide: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
        let bytes = unsafe {
            std::slice::from_raw_parts(wide.as_ptr() as *const u8, wide.len() * std::mem::size_of::<u16>())
        };
        unsafe {
            let _ = RegSetValueExW(key, &HSTRING::from(name), None, REG_SZ, Some(bytes));
        }
    }

    unsafe {
        let mut key = HKEY::default();
        let status = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            &HSTRING::from("Software\\Classes\\AppUserModelId\\com.localtodo.desktop"),
            None,
            windows::core::PCWSTR::null(),
            REG_OPTION_NON_VOLATILE,
            KEY_WRITE,
            None,
            &mut key,
            None,
        );
        if status != ERROR_SUCCESS {
            logging::info("通知身份登记失败，Windows 通知可能不会显示");
            return;
        }
        set_string(key, "DisplayName", "Local To Do");
        if let Ok(exe) = std::env::current_exe() {
            set_string(key, "IconUri", &exe.display().to_string());
        }
        let _ = RegCloseKey(key);
    }
}

#[cfg(desktop)]
fn setup_tray(app: &AppHandle) -> tauri::Result<()> {
    let show = MenuItem::with_id(app, "show", "打开 Local To Do", true, None::<&str>)?;
    let new_task = MenuItem::with_id(app, "new_task", "新建任务", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&show, &new_task, &quit])?;

    let mut builder = TrayIconBuilder::with_id("main-tray")
        .tooltip("Local To Do")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "show" => show_main(app),
            "new_task" => {
                show_main(app);
                let _ = app.emit("new-task-requested", ());
            }
            "quit" => {
                logging::info("从托盘菜单退出应用");
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
                show_main(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}
