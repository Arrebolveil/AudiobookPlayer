// Learn more about Tauri commands at https://tauri.app/develop/calling-rust/
#[tauri::command]
fn greet(name: &str) -> String {
    format!("Hello, {}! You've been greeted from Rust!", name)
}

use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct BookDir {
    name: String,
    path: String,
    files: Vec<String>,
}

#[tauri::command]
fn exit_app(app: tauri::AppHandle) {
    app.exit(0);
}

#[tauri::command]
fn scan_audio_files(dir_path: String) -> Result<Vec<BookDir>, String> {
    use std::fs;
    use std::path::{Path, PathBuf};

    let mut books = Vec::new();
    let root = Path::new(&dir_path);

    fn scan_single_dir(path: &Path) -> Vec<String> {
        let mut files = Vec::new();
        if let Ok(entries) = fs::read_dir(path) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_file() {
                    if let Some(ext) = p.extension() {
                        let ext_str = ext.to_string_lossy().to_lowercase();
                        if matches!(ext_str.as_str(), "mp3" | "wav" | "flac" | "m4a") {
                            files.push(p.to_string_lossy().into_owned());
                        }
                    }
                }
            }
            files.sort_by(|a, b| natord::compare(a, b));
        }
        files
    }

    let root_files = scan_single_dir(root);
    if !root_files.is_empty() {
        books.push(BookDir {
            name: root
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            path: dir_path.clone(),
            files: root_files,
        });
    }

    if let Ok(entries) = fs::read_dir(root) {
        let mut subdirs: Vec<PathBuf> = entries
            .flatten()
            .filter(|e| e.path().is_dir())
            .map(|e| e.path())
            .collect();
        subdirs.sort_by(|a, b| natord::compare(&a.to_string_lossy(), &b.to_string_lossy()));

        for subdir in subdirs {
            let sub_files = scan_single_dir(&subdir);
            if !sub_files.is_empty() {
                books.push(BookDir {
                    name: subdir
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                    path: subdir.to_string_lossy().into_owned(),
                    files: sub_files,
                });
            }
        }
    }

    Ok(books)
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    #[allow(unused_mut)]
    let mut builder = tauri::Builder::default()
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_fs::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![greet, scan_audio_files, exit_app]);

    #[cfg(desktop)]
    {
        builder = builder.setup(|app| {
            use tauri::Manager;
            use tauri::Emitter;
            let play_pause_i = tauri::menu::MenuItem::with_id(app, "play_pause", "播放 / 暂停", true, None::<&str>)?;
            let prev_i = tauri::menu::MenuItem::with_id(app, "prev", "上一首", true, None::<&str>)?;
            let next_i = tauri::menu::MenuItem::with_id(app, "next", "下一首", true, None::<&str>)?;
            let sep_i = tauri::menu::PredefinedMenuItem::separator(app)?;
            let show_i = tauri::menu::MenuItem::with_id(app, "show", "显示播放器界面", true, None::<&str>)?;
            let quit_i = tauri::menu::MenuItem::with_id(app, "quit", "完全退出悦听", true, None::<&str>)?;
            let menu = tauri::menu::Menu::with_items(app, &[&play_pause_i, &prev_i, &next_i, &sep_i, &show_i, &quit_i])?;

            tauri::tray::TrayIconBuilder::new()
                .menu(&menu)
                .tooltip("悦听 Audiobook Player")
                .icon(app.default_window_icon().unwrap().clone())
                .on_menu_event(|app, event| match event.id.as_ref() {
                    "quit" => {
                        app.exit(0);
                    }
                    "show" => {
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                    "play_pause" => {
                        let _ = app.emit("tray-action", "play_pause");
                    }
                    "prev" => {
                        let _ = app.emit("tray-action", "prev");
                    }
                    "next" => {
                        let _ = app.emit("tray-action", "next");
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if let tauri::tray::TrayIconEvent::Click {
                        button: tauri::tray::MouseButton::Left,
                        button_state: tauri::tray::MouseButtonState::Up,
                        ..
                    } = event {
                        let app = tray.app_handle();
                        if let Some(window) = app.get_webview_window("main") {
                            let _ = window.show();
                            let _ = window.set_focus();
                        }
                    }
                })
                .build(app)?;
            Ok(())
        });
    }

    builder.run(tauri::generate_context!())
        .expect("error while running tauri application");
}
