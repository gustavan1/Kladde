//! Kladde desktop app: one window, one copy at a time, data in %APPDATA%\Kladde\kladde.json.

mod ai;
mod codex_client;
mod claude;
mod commands;
mod feeds;
mod importers;
mod planner;
mod school;
mod store;

use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

use tauri::webview::NewWindowResponse;
use tauri::{AppHandle, Manager, Url, WebviewUrl, WebviewWindowBuilder};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};
use tauri_plugin_opener::OpenerExt;

use commands::AppState;

static LOG_FILE: OnceLock<PathBuf> = OnceLock::new();

/// Appends a line to %APPDATA%\Kladde\kladde.log (only used for problems).
pub fn log(message: &str) {
    if let Some(path) = LOG_FILE.get() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
            let _ = writeln!(f, "{} {message}", store::now_iso());
        }
    }
}

/// The folder for kladde.json: %APPDATA%\Kladde on Windows (~/.local/share/Kladde on Linux).
/// KLADDE_DATA_DIR can point somewhere else (used by the automated tests).
fn data_dir(app: &AppHandle) -> tauri::Result<PathBuf> {
    if let Some(dir) = std::env::var_os("KLADDE_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    Ok(app.path().data_dir()?.join("Kladde"))
}

/// Pages that belong to Kladde itself; everything else opens in the normal browser.
fn is_internal(url: &Url) -> bool {
    match url.scheme() {
        "tauri" | "about" => true,
        "http" | "https" => {
            url.host_str() == Some("tauri.localhost") || (cfg!(debug_assertions) && url.host_str() == Some("localhost"))
        }
        _ => false,
    }
}

fn open_external(app: &AppHandle, url: &Url) {
    if matches!(url.scheme(), "http" | "https" | "mailto") {
        if app.opener().open_url(url.as_str(), None::<&str>).is_err() {
            log("Could not open an external link.");
        }
    }
}

/// Opens an outside link (http, https or mailto) in the user's normal browser or mail app.
/// The window calls this for every outside link, so it works the same in every web view.
#[tauri::command]
fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    let parsed = Url::parse(&url).map_err(|_| "Invalid link".to_string())?;
    if !matches!(parsed.scheme(), "http" | "https" | "mailto") {
        return Err("Only web and mail links can be opened".into());
    }
    app.opener().open_url(parsed.as_str(), None::<&str>).map_err(|_| {
        log("Could not open an external link.");
        "System handler could not open the link".to_string()
    })
}

fn create_window(app: &AppHandle) -> tauri::Result<()> {
    let nav_app = app.clone();
    let popup_app = app.clone();
    WebviewWindowBuilder::new(app, "main", WebviewUrl::App("index.html".into()))
        .title("Kladde")
        .inner_size(1320.0, 860.0)
        .min_inner_size(760.0, 560.0)
        .center()
        .on_navigation(move |url| {
            if is_internal(url) {
                true
            } else {
                open_external(&nav_app, url);
                false
            }
        })
        .on_new_window(move |url, _features| {
            open_external(&popup_app, &url);
            NewWindowResponse::Deny
        })
        .build()?;
    Ok(())
}

pub fn run() {
    #[cfg(target_os = "macos")]
    {
        let mut args = std::env::args_os().skip(1);
        if args.next().as_deref() == Some(std::ffi::OsStr::new("--kladde-mcp")) {
            if let Some(dir) = args.next() { claude::stdio(std::path::Path::new(&dir)); }
            return;
        }
    }
    tauri::Builder::default()
        // Only one copy of Kladde: starting it again just brings the open window forward.
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.show();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        // Signed updates from the GitHub releases (see tauri.conf.json → plugins.updater).
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let dir = data_dir(app.handle())?;
            std::fs::create_dir_all(&dir)?;
            let _ = LOG_FILE.set(dir.join("kladde.log"));
            match store::Store::open(&dir.join("kladde.json")) {
                Ok(store) => {
                    app.manage(AppState { store: Mutex::new(store), chat_generation: Default::default(), refreshing: Default::default() });
                    // Provider preferences contain no credentials.
                    app.manage(ai::AiState::load(dir.join("providers.json")));
                    create_window(app.handle())?;
                    feeds::start_auto_sync(app.handle().clone());
                }
                Err(e) => {
                    // Never start with an empty file over unreadable data: explain and stop.
                    log(&format!("Could not open data: {e}"));
                    let handle = app.handle().clone();
                    app.dialog()
                        .message(format!(
                            "{e}\n\nFilen er ikke ændret. Gendan en backup (se README), og start Kladde igen."
                        ))
                        .title("Kladde kunne ikke starte")
                        .kind(MessageDialogKind::Error)
                        .show(move |_| handle.exit(1));
                }
            }
            Ok(())
        })
        .invoke_handler({
            let dispatch: fn(tauri::ipc::Invoke<tauri::Wry>) -> bool = tauri::generate_handler![
                open_url,
                school::open_school,
                commands::dashboard,
                commands::weekly_summary_cmd,
                commands::export_backup,
                commands::create_subject,
                commands::rename_subject,
                commands::create_note,
                commands::update_note,
                commands::delete_note,
                commands::create_task,
                commands::create_project_tasks,
                commands::update_task,
                commands::delete_task,
                commands::create_block,
                commands::delete_block,
                commands::make_study_plan_cmd,
                commands::clear_study_plan,
                commands::import_preview,
                commands::import_commit,
                commands::list_feeds,
                commands::add_feed,
                commands::sync_feed,
                commands::delete_feed,
                ai::ai_status,
                ai::ai_save_key,
                ai::ai_codex,
                claude::claude_action,
                ai::ai_forget_key,
                ai::ai_note,
                ai::ai_project_plan,
                ai::ai_messages,
                ai::ai_clear_chat,
                ai::ai_chat,
            ];
            move |invoke: tauri::ipc::Invoke<tauri::Wry>| {
                if invoke.message.webview_ref().label() != "main" {
                    invoke.resolver.reject("This command requires the main window.");
                    return true;
                }
                dispatch(invoke)
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Kladde");
}
