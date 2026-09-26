pub mod collectors;
pub mod commands;
pub mod domain;
pub mod growth;
pub mod platform;
pub mod storage;
pub mod sync;

use std::{fs, io, sync::Mutex};

use tauri::{AppHandle, Emitter, LogicalSize, Manager, State, WebviewWindow, WindowEvent};

use collectors::discovery::{resolve_roots, scan_sources, RootOptions, SourceConfig};
use domain::planet::PlanetAvatar;
use domain::usage::Agent;
use growth::{world_snapshot, WorldSnapshot};
use storage::ledger::Ledger;
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    config: Mutex<SourceConfig>,
    pub(crate) ledger: Mutex<Ledger>,
    pub(crate) latest: Mutex<Option<WorldSnapshot>>,
    pub(crate) sync_failed: Mutex<bool>,
    pub(crate) sync_gate: tokio::sync::Mutex<()>,
}

impl AppState {
    pub(crate) fn select_planet_account(&self, user_id: &str) -> Result<(), String> {
        let changed = self
            .ledger
            .lock()
            .map_err(|_| "local ledger unavailable")?
            .ensure_planet_account(user_id)
            .map_err(|_| "행성 계정을 변경할 수 없습니다")?;
        if changed {
            // Invalidate the previous account's frontend snapshot before any
            // fallible scan, so it can never become an upload for this account.
            *self.latest.lock().map_err(|_| "usage status unavailable")? = None;
            self.scan()?;
        }
        Ok(())
    }

    pub(crate) fn scan(&self) -> Result<WorldSnapshot, String> {
        let config = self
            .config
            .lock()
            .map_err(|_| "source settings unavailable")?
            .clone();
        let mut ledger = self.ledger.lock().map_err(|_| "local ledger unavailable")?;
        let summary = scan_sources(&config, &mut ledger).map_err(|_| "usage scan unavailable")?;
        ledger
            .set_source_health(Agent::Codex, summary.codex_source)
            .map_err(|_| "source health unavailable")?;
        ledger
            .set_source_health(Agent::ClaudeCode, summary.claude_code_source)
            .map_err(|_| "source health unavailable")?;
        let snapshot = world_snapshot(&ledger, summary).map_err(|_| "world growth unavailable")?;
        *self.latest.lock().map_err(|_| "usage status unavailable")? = Some(snapshot.clone());
        Ok(snapshot)
    }
}

#[tauri::command]
fn refresh_usage(state: State<'_, AppState>, app: AppHandle) -> Result<WorldSnapshot, String> {
    let snapshot = state.scan()?;
    let _ = platform::tray::refresh_status(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn current_usage(state: State<'_, AppState>) -> Result<Option<WorldSnapshot>, String> {
    Ok(state
        .latest
        .lock()
        .map_err(|_| "usage status unavailable")?
        .clone())
}

#[tauri::command]
fn set_planet_profile(
    nickname: String,
    avatar: PlanetAvatar,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<WorldSnapshot, String> {
    state
        .ledger
        .lock()
        .map_err(|_| "local ledger unavailable")?
        .set_planet_profile(&nickname, avatar)
        .map_err(|_| "행성 프로필을 저장할 수 없습니다")?;
    let snapshot = state.scan()?;
    let _ = platform::tray::refresh_status(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn reset_planet(state: State<'_, AppState>, app: AppHandle) -> Result<WorldSnapshot, String> {
    let _ = state.scan()?;
    state
        .ledger
        .lock()
        .map_err(|_| "local ledger unavailable")?
        .reset_planet(chrono::Utc::now())
        .map_err(|error| match error {
            storage::ledger::ScanError::ResetCooldown => {
                String::from("초기화는 마지막 초기화 24시간 뒤에 가능합니다")
            }
            _ => String::from("행성을 초기화할 수 없습니다"),
        })?;
    let snapshot = state.scan()?;
    let _ = platform::tray::refresh_status(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
fn set_source_enabled(
    agent: Agent,
    enabled: bool,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<WorldSnapshot, String> {
    state
        .ledger
        .lock()
        .map_err(|_| "local ledger unavailable")?
        .set_agent_enabled(agent, enabled)
        .map_err(|_| "source setting unavailable")?;
    let snapshot = state.scan()?;
    let _ = platform::tray::refresh_status(&app, &snapshot);
    Ok(snapshot)
}

#[tauri::command]
async fn choose_source_folder(
    agent: Agent,
    app: AppHandle,
    state: State<'_, AppState>,
) -> Result<Option<WorldSnapshot>, String> {
    let title = match agent {
        Agent::Codex => "Codex sessions 폴더 선택",
        Agent::ClaudeCode => "Claude Code projects 폴더 선택",
    };
    let dialog_app = app.clone();
    let selection = tauri::async_runtime::spawn_blocking(move || {
        dialog_app
            .dialog()
            .file()
            .set_title(title)
            .blocking_pick_folder()
    })
    .await
    .map_err(|_| "folder dialog unavailable")?;
    let Some(folder) = selection else {
        return Ok(None);
    };
    let folder = folder.into_path().map_err(|_| "folder unavailable")?;
    if !folder.is_dir() {
        return Err("folder unavailable".into());
    }
    state
        .ledger
        .lock()
        .map_err(|_| "local ledger unavailable")?
        .set_custom_root(agent, &folder)
        .map_err(|_| "source setting unavailable")?;
    {
        let mut config = state
            .config
            .lock()
            .map_err(|_| "source settings unavailable")?;
        match agent {
            Agent::Codex => config.codex_root = folder,
            Agent::ClaudeCode => config.claude_root = folder,
        }
    }
    let snapshot = state.scan()?;
    let _ = platform::tray::refresh_status(&app, &snapshot);
    Ok(Some(snapshot))
}

#[tauri::command]
fn set_detail_view(detail: bool, window: WebviewWindow) -> Result<(), String> {
    let (width, height) = if detail {
        (820.0, 600.0)
    } else {
        (390.0, 700.0)
    };
    window
        .set_size(LogicalSize::new(width, height))
        .map_err(|_| "window size unavailable")?;
    window.center().map_err(|_| "window position unavailable")?;
    Ok(())
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            refresh_usage,
            current_usage,
            set_planet_profile,
            reset_planet,
            set_source_enabled,
            set_detail_view,
            choose_source_folder,
            commands::sharing::get_sharing_state,
            commands::sharing::request_email_code,
            commands::sharing::verify_email_code,
            commands::sharing::create_shared_world,
            commands::sharing::join_world,
            commands::sharing::create_invite,
            commands::sharing::list_invites,
            commands::sharing::list_world_members,
            commands::sharing::revoke_invite,
            commands::sharing::pause_sharing,
            commands::sharing::transfer_world_owner,
            commands::sharing::leave_world,
            commands::sharing::delete_synced_usage,
            commands::sharing::sign_out
        ])
        .setup(|app| {
            #[cfg(target_os = "macos")]
            platform::macos::configure(app);
            #[cfg(target_os = "windows")]
            platform::windows::configure(app);
            platform::tray::install(app)?;
            let app_data = app.path().app_local_data_dir()?;
            fs::create_dir_all(&app_data)?;
            let ledger_path = app_data.join("usage-ledger.sqlite3");
            let timezone = match Ledger::saved_timezone(&ledger_path)? {
                Some(saved) => saved,
                None => iana_time_zone::get_timezone()
                    .map_err(|_| io::Error::other("system timezone unavailable"))?
                    .parse()
                    .map_err(|_| io::Error::other("system timezone is not an IANA timezone"))?,
            };
            let ledger = Ledger::open(&ledger_path, timezone)?;
            let roots = RootOptions::from_env(
                ledger.custom_root(Agent::Codex)?,
                ledger.custom_root(Agent::ClaudeCode)?,
                timezone,
            )
            .ok_or_else(|| io::Error::other("home directory unavailable"))?;
            let config = resolve_roots(&roots);
            let state = AppState {
                config: Mutex::new(config),
                ledger: Mutex::new(ledger),
                latest: Mutex::new(None),
                sync_failed: Mutex::new(false),
                sync_gate: tokio::sync::Mutex::new(()),
            };
            let _ = state.scan();
            app.manage(state);
            if let Some(snapshot) = app
                .state::<AppState>()
                .latest
                .lock()
                .ok()
                .and_then(|value| value.clone())
            {
                let _ = platform::tray::refresh_status(app.handle(), &snapshot);
            }
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                let mut retry = sync::worker::RetryDelay::default();
                loop {
                    let state = handle.state::<AppState>();
                    if let Ok(summary) = state.scan() {
                        let _ = platform::tray::refresh_status(&handle, &summary);
                        let _ = handle.emit("usage-updated", summary);
                    }
                    let result = tauri::async_runtime::block_on(sync::worker::sync_once(&state));
                    if let Ok(mut failed) = state.sync_failed.lock() {
                        *failed = result.is_err();
                    }
                    let _ = handle.emit("sync-status-updated", ());
                    std::thread::sleep(retry.next_after(result.is_ok()));
                }
            });
            Ok(())
        })
        .on_window_event(|window, event| {
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
