pub mod collectors;
pub mod commands;
pub mod domain;
pub mod growth;
pub mod platform;
pub mod storage;
pub mod sync;

use std::{
    fs, io,
    sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    },
};

use tauri::{
    AppHandle, Emitter, LogicalSize, Manager, PhysicalPosition, PhysicalSize, Rect, State,
    WebviewWindow, WindowEvent,
};

use collectors::discovery::{resolve_roots, scan_sources, RootOptions, SourceConfig};
use domain::planet::PlanetAvatar;
use domain::usage::Agent;
use growth::{world_snapshot, WorldSnapshot};
use storage::ledger::Ledger;
use tauri_plugin_dialog::DialogExt;

use platform::popover::{physical_icon_bounds, popup_bounds, Bounds};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum WindowMode {
    Popup,
    Detail,
    Setup,
}

pub(crate) fn initial_mode(has_profile: bool) -> WindowMode {
    if has_profile {
        WindowMode::Popup
    } else {
        WindowMode::Setup
    }
}

pub(crate) fn initial_mode_after_scan<E>(scan_result: Result<bool, E>) -> WindowMode {
    match scan_result {
        Ok(has_profile) => initial_mode(has_profile),
        Err(_) => WindowMode::Popup,
    }
}

pub(crate) fn tray_target(mode: WindowMode, visible: bool) -> Option<WindowMode> {
    if visible && matches!(mode, WindowMode::Popup | WindowMode::Setup) {
        None
    } else if !visible && mode == WindowMode::Setup {
        Some(WindowMode::Setup)
    } else {
        Some(WindowMode::Popup)
    }
}

pub(crate) fn should_hide_on_blur(mode: WindowMode, tray_press_pending: bool) -> bool {
    mode == WindowMode::Popup && !tray_press_pending
}

pub struct AppState {
    config: Mutex<SourceConfig>,
    pub(crate) ledger: Mutex<Ledger>,
    pub(crate) latest: Mutex<Option<WorldSnapshot>>,
    pub(crate) sync_failed: Mutex<bool>,
    pub(crate) sync_gate: tokio::sync::Mutex<()>,
    pub(crate) window_mode: Mutex<WindowMode>,
    mode_transitioning: AtomicBool,
    pub(crate) tray_press_pending: AtomicBool,
}

pub(crate) fn transition_window_mode(
    app: &AppHandle,
    target: WindowMode,
    icon_rect: Option<Rect>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window unavailable".to_string())?;
    let state = app.state::<AppState>();
    let previous = *state
        .window_mode
        .lock()
        .map_err(|_| "window mode unavailable".to_string())?;
    state
        .mode_transitioning
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .map_err(|_| "window transition already in progress".to_string())?;
    let applied = apply_window_mode(&window, target, icon_rect);
    let result = if applied.is_ok() {
        state
            .window_mode
            .lock()
            .map(|mut mode| *mode = target)
            .map_err(|_| "window mode unavailable".to_string())
    } else {
        applied
    };
    if result.is_err() {
        let _ = apply_window_mode(&window, previous, icon_rect);
    }
    state.mode_transitioning.store(false, Ordering::SeqCst);
    result
}

fn apply_window_mode(
    window: &WebviewWindow,
    mode: WindowMode,
    icon_rect: Option<Rect>,
) -> Result<(), String> {
    match mode {
        WindowMode::Detail => {
            let mut target: (f64, f64) = (960.0, 700.0);
            if let Ok(Some(monitor)) = window.current_monitor() {
                let size = monitor.size();
                let scale = monitor.scale_factor();
                target.0 = target.0.min((size.width as f64 / scale - 80.0).max(1.0));
                target.1 = target.1.min((size.height as f64 / scale - 120.0).max(1.0));
            }
            window
                .set_decorations(true)
                .map_err(|_| "window decorations unavailable".to_string())?;
            window
                .set_always_on_top(false)
                .map_err(|_| "window level unavailable".to_string())?;
            window
                .set_size(LogicalSize::new(target.0, target.1))
                .map_err(|_| "window size unavailable".to_string())?;
            window
                .center()
                .map_err(|_| "window position unavailable".to_string())?;
        }
        WindowMode::Setup => {
            window
                .set_decorations(true)
                .map_err(|_| "window decorations unavailable".to_string())?;
            window
                .set_always_on_top(false)
                .map_err(|_| "window level unavailable".to_string())?;
            window
                .set_size(LogicalSize::new(390.0, 700.0))
                .map_err(|_| "window size unavailable".to_string())?;
            window
                .center()
                .map_err(|_| "window position unavailable".to_string())?;
        }
        WindowMode::Popup => {
            window
                .set_decorations(false)
                .map_err(|_| "window decorations unavailable".to_string())?;
            window
                .set_always_on_top(true)
                .map_err(|_| "window level unavailable".to_string())?;
            let (monitor, icon) = popup_monitor(window, icon_rect)?;
            let scale = monitor.scale_factor();
            let position = monitor.position();
            let size = monitor.size();
            let bounds = popup_bounds(
                icon,
                Bounds {
                    x: position.x,
                    y: position.y,
                    width: size.width,
                    height: size.height,
                },
                (400.0 * scale).round().max(1.0) as u32,
                (700.0 * scale).round().max(1.0) as u32,
            );
            window
                .set_size(PhysicalSize::new(bounds.width, bounds.height))
                .map_err(|_| "window size unavailable".to_string())?;
            window
                .set_position(PhysicalPosition::new(bounds.x, bounds.y))
                .map_err(|_| "window position unavailable".to_string())?;
        }
    }

    window
        .unminimize()
        .map_err(|_| "window restore unavailable".to_string())?;
    window
        .show()
        .map_err(|_| "window show unavailable".to_string())?;
    window
        .set_focus()
        .map_err(|_| "window focus unavailable".to_string())?;
    Ok(())
}

fn popup_monitor(
    window: &WebviewWindow,
    icon_rect: Option<Rect>,
) -> Result<(tauri::Monitor, Option<Bounds>), String> {
    let monitors = window
        .available_monitors()
        .map_err(|_| "display information unavailable".to_string())?;
    let icon_monitor = icon_rect.and_then(|rect| {
        monitors.iter().find_map(|monitor| {
            let icon = physical_icon_bounds(rect, monitor.scale_factor());
            let center_x = i64::from(icon.x) + i64::from(icon.width) / 2;
            let center_y = i64::from(icon.y) + i64::from(icon.height) / 2;
            let position = monitor.position();
            let size = monitor.size();
            let left = i64::from(position.x);
            let top = i64::from(position.y);
            (center_x >= left
                && center_x < left + i64::from(size.width)
                && center_y >= top
                && center_y < top + i64::from(size.height))
                .then(|| (monitor.clone(), icon))
        })
    });
    if let Some((monitor, icon)) = icon_monitor {
        return Ok((monitor, Some(icon)));
    }

    let monitor = window
        .current_monitor()
        .ok()
        .flatten()
        .or_else(|| window.primary_monitor().ok().flatten())
        .or_else(|| monitors.first().cloned())
        .ok_or_else(|| "display information unavailable".to_string())?;
    let icon = icon_rect.map(|rect| physical_icon_bounds(rect, monitor.scale_factor()));
    Ok((monitor, icon))
}

#[cfg(test)]
mod window_mode_tests {
    use super::{
        initial_mode, initial_mode_after_scan, should_hide_on_blur, tray_target, WindowMode,
    };

    #[test]
    fn initial_mode_depends_on_profile() {
        assert_eq!(initial_mode(true), WindowMode::Popup);
        assert_eq!(initial_mode(false), WindowMode::Setup);
    }

    #[test]
    fn failed_startup_scan_shows_loading_popup_without_offering_profile_creation() {
        assert_eq!(initial_mode_after_scan(Err::<bool, ()>(())), WindowMode::Popup);
        assert_eq!(initial_mode_after_scan(Ok::<bool, ()>(true)), WindowMode::Popup);
        assert_eq!(initial_mode_after_scan(Ok::<bool, ()>(false)), WindowMode::Setup);
    }

    #[test]
    fn tray_click_toggles_popup() {
        assert_eq!(tray_target(WindowMode::Popup, true), None);
        assert_eq!(tray_target(WindowMode::Popup, false), Some(WindowMode::Popup));
        assert_eq!(tray_target(WindowMode::Detail, true), Some(WindowMode::Popup));
    }

    #[test]
    fn blur_hides_popup_only() {
        assert!(should_hide_on_blur(WindowMode::Popup, false));
        assert!(!should_hide_on_blur(WindowMode::Detail, false));
        assert!(!should_hide_on_blur(WindowMode::Setup, false));
    }

    #[test]
    fn blur_before_tray_release_keeps_popup_open_to_toggle_closed() {
        let tray_press_pending = true;
        let mut visible = true;
        if should_hide_on_blur(WindowMode::Popup, tray_press_pending) {
            visible = false;
        }

        assert!(tray_press_pending);
        assert_eq!(tray_target(WindowMode::Popup, visible), None);
    }
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
        ledger
            .prepare_growth_journal()
            .map_err(|_| "growth journal unavailable")?;
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
    transition_window_mode(
        &app,
        WindowMode::Popup,
        platform::tray::icon_rect(&app),
    )?;
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
fn set_detail_view(detail: bool, app: AppHandle) -> Result<(), String> {
    let target = if detail { WindowMode::Detail } else { WindowMode::Popup };
    let icon_rect = (!detail).then(|| platform::tray::icon_rect(&app)).flatten();
    transition_window_mode(&app, target, icon_rect)
}

#[tauri::command]
fn hide_popover(app: AppHandle) -> Result<(), String> {
    let state = app.state::<AppState>();
    let mode = *state
        .window_mode
        .lock()
        .map_err(|_| "window mode unavailable".to_string())?;
    if mode != WindowMode::Popup {
        return Ok(());
    }
    app.get_webview_window("main")
        .ok_or_else(|| "main window unavailable".to_string())?
        .hide()
        .map_err(|_| "window hide unavailable".to_string())
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
            hide_popover,
            choose_source_folder,
            commands::cosmetic_shop::get_shop_state,
            commands::cosmetic_shop::purchase_cosmetic,
            commands::cosmetic_shop::equip_cosmetic,
            commands::growth_journal::get_growth_journal,
            commands::growth_journal::delete_growth_journal,
            commands::sharing::get_sharing_state,
            commands::sharing::start_anonymous_session,
            commands::sharing::create_shared_world,
            commands::sharing::join_world,
            commands::sharing::get_my_member_code,
            commands::sharing::rotate_my_member_code,
            commands::sharing::list_world_members,
            commands::sharing::pause_sharing,
            commands::sharing::transfer_world_owner,
            commands::sharing::leave_world,
            commands::sharing::delete_synced_usage
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
                window_mode: Mutex::new(WindowMode::Popup),
                mode_transitioning: AtomicBool::new(false),
                tray_press_pending: AtomicBool::new(false),
            };
            let initial_scan = state.scan();
            let startup_mode = initial_mode_after_scan(
                initial_scan
                    .as_ref()
                    .map(|snapshot| snapshot.planet.profile.is_some()),
            );
            *state
                .window_mode
                .lock()
                .map_err(|_| io::Error::other("window mode unavailable"))? = startup_mode;
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
            transition_window_mode(
                app.handle(),
                startup_mode,
                (startup_mode == WindowMode::Popup)
                    .then(|| platform::tray::icon_rect(app.handle()))
                    .flatten(),
            )
            .map_err(io::Error::other)?;
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
            } else if let WindowEvent::Focused(false) = event {
                let Some(state) = window.app_handle().try_state::<AppState>() else {
                    return;
                };
                if !state.mode_transitioning.load(Ordering::SeqCst)
                    && state
                        .window_mode
                        .lock()
                        .is_ok_and(|mode| {
                            should_hide_on_blur(
                                *mode,
                                state.tray_press_pending.load(Ordering::SeqCst),
                            )
                        })
                {
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
