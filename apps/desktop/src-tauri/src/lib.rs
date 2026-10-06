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
use sync::auth::{AuthConfig, SessionStore, SupabaseAuthClient};
use sync::client::SupabaseSyncClient;

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
    usage_scan_failed: AtomicBool,
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
    transition_window_mode_with_visibility(app, target, icon_rect, true)
}

fn transition_startup_window_mode(
    app: &AppHandle,
    target: WindowMode,
    icon_rect: Option<Rect>,
) -> Result<(), String> {
    let window = app
        .get_webview_window("main")
        .ok_or_else(|| "main window unavailable".to_string())?;
    let state = app.state::<AppState>();
    if *state
        .window_mode
        .lock()
        .map_err(|_| "window mode unavailable".to_string())?
        == WindowMode::Detail
    {
        return Ok(());
    }
    let visible = window
        .is_visible()
        .map_err(|_| "window visibility unavailable".to_string())?;
    transition_window_mode_with_visibility(app, target, icon_rect, visible)
}

fn transition_window_mode_with_visibility(
    app: &AppHandle,
    target: WindowMode,
    icon_rect: Option<Rect>,
    show: bool,
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
    let result = platform::window_appearance::apply_with_rollback(previous, target, |mode| {
        apply_window_mode(&window, mode, icon_rect, show)?;
        state
            .window_mode
            .lock()
            .map(|mut current| *current = mode)
            .map_err(|_| "window mode unavailable".to_string())
    });
    state.mode_transitioning.store(false, Ordering::SeqCst);
    result
}

fn apply_window_mode(
    window: &WebviewWindow,
    mode: WindowMode,
    icon_rect: Option<Rect>,
    show: bool,
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

    platform::window_appearance::apply(window, mode)?;

    if show {
        window
            .unminimize()
            .map_err(|_| "window restore unavailable".to_string())?;
        window
            .show()
            .map_err(|_| "window show unavailable".to_string())?;
        window
            .set_focus()
            .map_err(|_| "window focus unavailable".to_string())?;
    }
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
        initial_mode, initial_mode_after_scan, should_hide_on_blur, tray_target,
        with_guest_planet_reset_authority, WindowMode,
    };
    use crate::storage::ledger::Ledger;
    use std::path::Path;

    #[test]
    fn restored_authenticated_account_is_blocked_before_local_reset_mutation() {
        let mut ledger = Ledger::open(Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger
            .ensure_planet_account("00000000-0000-0000-0000-000000000001")
            .unwrap();
        let account_id = ledger.cosmetic_account_id().unwrap();
        let cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO planet_wallet_credit(previous_cycle_id,amount,created_at_utc)
             VALUES (?1,42,'2026-10-01T00:00:00Z')",
                [&cycle_id],
            )
            .unwrap();
        ledger.connection.execute(
            "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
             VALUES (?1,'saved-tree','land_tree',0,'saved-seed',1,'2026-10-01T00:00:00Z')",
            [&account_id],
        ).unwrap();
        ledger
            .connection
            .execute(
                "INSERT INTO shop_landscape_placement(account_id,instance_id,cycle_id,x,y,version)
             VALUES (?1,'saved-tree',?2,12.0,18.0,4)",
                rusqlite::params![account_id, cycle_id],
            )
            .unwrap();

        let before_cycle = ledger.planet_cycle_id().unwrap();
        let before_credits = ledger.planet_wallet_credits().unwrap();
        let before_shop = ledger.shop_state().unwrap();
        let before_reset_available_at = ledger.reset_available_at().unwrap();

        let mut scan_or_reset_reached = false;
        assert_eq!(
            with_guest_planet_reset_authority(&account_id, || {
                scan_or_reset_reached = true;
                Ok(())
            }),
            Err("로그인된 행성은 서버에서 초기화해야 합니다".to_string()),
        );
        assert!(!scan_or_reset_reached);

        assert_eq!(ledger.planet_cycle_id().unwrap(), before_cycle);
        assert_eq!(ledger.planet_wallet_credits().unwrap(), before_credits);
        assert_eq!(ledger.shop_state().unwrap(), before_shop);
        assert_eq!(
            ledger.reset_available_at().unwrap(),
            before_reset_available_at
        );
        let reset_requests: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_action_request WHERE request_id LIKE 'legacy-reset:%'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(reset_requests, 0);
    }

    #[test]
    fn guest_account_still_passes_local_reset_authority_guard() {
        let mut ledger = Ledger::open(Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let before_cycle = ledger.planet_cycle_id().unwrap();
        let account_id = ledger.cosmetic_account_id().unwrap();
        let mut guest_action_reached = false;
        with_guest_planet_reset_authority(&account_id, || {
            guest_action_reached = true;
            Ok(())
        })
        .unwrap();
        assert!(guest_action_reached);

        ledger
            .reset_planet(
                chrono::DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
                    .unwrap()
                    .to_utc(),
            )
            .unwrap();

        assert_ne!(ledger.planet_cycle_id().unwrap(), before_cycle);
    }

    #[test]
    fn initial_mode_depends_on_profile() {
        assert_eq!(initial_mode(true), WindowMode::Popup);
        assert_eq!(initial_mode(false), WindowMode::Setup);
    }

    #[test]
    fn failed_startup_scan_shows_loading_popup_without_offering_profile_creation() {
        assert_eq!(
            initial_mode_after_scan(Err::<bool, ()>(())),
            WindowMode::Popup
        );
        assert_eq!(
            initial_mode_after_scan(Ok::<bool, ()>(true)),
            WindowMode::Popup
        );
        assert_eq!(
            initial_mode_after_scan(Ok::<bool, ()>(false)),
            WindowMode::Setup
        );
    }

    #[test]
    fn tray_click_toggles_popup() {
        assert_eq!(tray_target(WindowMode::Popup, true), None);
        assert_eq!(
            tray_target(WindowMode::Popup, false),
            Some(WindowMode::Popup)
        );
        assert_eq!(
            tray_target(WindowMode::Detail, true),
            Some(WindowMode::Popup)
        );
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

#[cfg(test)]
mod reset_planet_response_tests {
    use super::ResetPlanetCommandError;

    #[test]
    fn normal_errors_remain_strings_and_confirmed_view_failure_is_discriminated() {
        assert_eq!(
            serde_json::to_value(ResetPlanetCommandError::Message("retry later".into())).unwrap(),
            serde_json::json!("retry later"),
        );
        assert_eq!(
            serde_json::to_value(ResetPlanetCommandError::ConfirmedViewUnavailable {
                kind: "confirmed_reset_view_unavailable",
                account_id: "00000000-0000-0000-0000-000000000052".into(),
                request_id: "90000000-0000-0000-0000-000000000052".into(),
                expected_old_cycle_id: "old-cycle".into(),
                new_cycle_id: "new-cycle".into(),
            })
            .unwrap(),
            serde_json::json!({
                "kind": "confirmed_reset_view_unavailable",
                "account_id": "00000000-0000-0000-0000-000000000052",
                "request_id": "90000000-0000-0000-0000-000000000052",
                "expected_old_cycle_id": "old-cycle",
                "new_cycle_id": "new-cycle",
            }),
        );
    }
}

#[cfg(test)]
mod account_switch_tests {
    use super::{
        allow_local_scan_on_guest_shop_hold, AppState, PlanetAccountSwitchError, WindowMode,
    };
    use crate::collectors::discovery::SourceConfig;
    use crate::collectors::{ParsedRecord, RecordKind};
    use crate::domain::planet::PlanetAvatar;
    use crate::domain::usage::{Agent, TokenUsage, UsageCoverage};
    use crate::storage::ledger::Ledger;
    use chrono::{DateTime, Duration, Utc};
    use std::{
        path::Path,
        sync::{atomic::AtomicBool, Mutex},
    };

    const USER_ID: &str = "00000000-0000-0000-0000-000000000071";

    fn test_state(ledger: Ledger) -> AppState {
        AppState {
            config: Mutex::new(SourceConfig {
                codex_root: Path::new("/private/tmp/token-planet-account-switch-codex")
                    .to_path_buf(),
                claude_root: Path::new("/private/tmp/token-planet-account-switch-claude")
                    .to_path_buf(),
                timezone: chrono_tz::UTC,
            }),
            ledger: Mutex::new(ledger),
            latest: Mutex::new(None),
            usage_scan_failed: AtomicBool::new(false),
            sync_failed: Mutex::new(false),
            sync_gate: tokio::sync::Mutex::new(()),
            window_mode: Mutex::new(WindowMode::Popup),
            mode_transitioning: AtomicBool::new(false),
            tray_press_pending: AtomicBool::new(false),
        }
    }

    fn insert_local_usage(ledger: &mut Ledger, event_key: &str) {
        ledger
            .insert(&ParsedRecord {
                agent: Agent::Codex,
                kind: RecordKind::Response,
                event_key: event_key.into(),
                occurred_at_utc: DateTime::parse_from_rfc3339("2026-10-01T12:00:00Z")
                    .unwrap()
                    .with_timezone(&Utc),
                usage: TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(1_000),
                    coverage: UsageCoverage::Complete,
                },
            })
            .unwrap();
    }

    fn insert_local_shop_instance(ledger: &Ledger) {
        ledger
            .connection
            .execute(
                "INSERT INTO shop_landscape_instance(account_id,instance_id,sku,variation_index,seed,variation_version,acquired_at_utc)
                 VALUES ('local','guest-tree','land_tree',0,'guest-seed',1,'2026-10-01T00:00:00Z')",
                [],
            )
            .unwrap();
    }

    fn prepared_first_reset_ledger() -> Ledger {
        let mut ledger = Ledger::open(Path::new(":memory:"), chrono_tz::UTC).unwrap();
        let occurred_at = Utc::now();
        ledger
            .insert(&ParsedRecord {
                agent: Agent::Codex,
                kind: RecordKind::Response,
                event_key: "account-switch-v2-source".into(),
                occurred_at_utc: occurred_at,
                usage: TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(1_000_000),
                    coverage: UsageCoverage::Complete,
                },
            })
            .unwrap();
        ledger.rebuild_shop_contributions().unwrap();
        ledger
            .settle_guest_rewards(occurred_at + Duration::milliseconds(1))
            .unwrap();
        let old_cycle_id = ledger.planet_cycle_id().unwrap();
        ledger
            .settle_guest_cycle_tokens(&old_cycle_id, occurred_at + Duration::milliseconds(2))
            .unwrap();
        ledger.prepare_growth_journal().unwrap();
        let reset_at = occurred_at + Duration::milliseconds(250);
        assert_eq!(ledger.reset_planet(reset_at).unwrap(), 1_000_000);
        if Utc::now() < reset_at {
            std::thread::sleep((reset_at - Utc::now()).to_std().unwrap());
        }
        ledger.prepare_growth_journal().unwrap();
        ledger
    }

    #[test]
    fn first_login_persists_v2_capture_before_holding_account_switch() {
        let state = test_state(prepared_first_reset_ledger());

        assert_eq!(
            state.switch_planet_account(USER_ID),
            Err(PlanetAccountSwitchError::GuestShopImportPending)
        );

        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
        let capture_table: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type='table' AND name='guest_shop_import_v2_capture'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let capture_rows = if capture_table == 0 {
            0
        } else {
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM guest_shop_import_v2_capture
                     WHERE target_account_id=?1",
                    [format!("account:{USER_ID}")],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(
            capture_rows, 1,
            "the stable v2 request must be durable before account switch is held"
        );
        let legacy_table: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM sqlite_master
                 WHERE type='table' AND name='guest_shop_import_capture'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let legacy_rows = if legacy_table == 0 {
            0
        } else {
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM guest_shop_import_capture
                     WHERE target_account_id=?1",
                    [format!("account:{USER_ID}")],
                    |row| row.get(0),
                )
                .unwrap()
        };
        assert_eq!(legacy_rows, 0, "v2 capture must not create a v1 request");
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM cosmetic_guest_import WHERE account_id=?1",
                    [format!("account:{USER_ID}")],
                    |row| row.get(0),
                )
                .unwrap(),
            0,
            "v2 capture must precede legacy cosmetic import creation"
        );
    }

    #[test]
    fn first_login_records_auth_selection_while_v2_guest_ownership_stays_local() {
        let state = test_state(prepared_first_reset_ledger());
        let target_account = format!("account:{USER_ID}");

        let selection = state.select_planet_account(USER_ID);

        assert!(
            selection.is_err(),
            "active account ownership remains held until guest import completes"
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
        let selected_auth_account: String = ledger
            .connection
            .query_row(
                "SELECT coalesce((SELECT value FROM setting WHERE key='selected_auth_account_id'), '')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(selected_auth_account, target_account);
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM guest_shop_import_v2_capture
                     WHERE target_account_id=?1",
                    [&target_account],
                    |row| row.get(0),
                )
                .unwrap(),
            1,
            "first login must leave a durable v2 capture for the selected auth account"
        );
    }

    #[test]
    fn next_sync_records_auth_selection_for_existing_v2_capture_without_switching_owner() {
        let mut ledger = prepared_first_reset_ledger();
        let target_account = format!("account:{USER_ID}");
        ledger
            .capture_guest_shop_import_request(&target_account)
            .unwrap();
        let request_before: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture
                 WHERE target_account_id=?1",
                [&target_account],
                |row| row.get(0),
            )
            .unwrap();
        let state = test_state(ledger);

        let selection = state.select_planet_account(USER_ID);

        assert!(
            selection.is_err(),
            "active account ownership remains held while an existing v2 capture is pending"
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
        let selected_auth_account: String = ledger
            .connection
            .query_row(
                "SELECT coalesce((SELECT value FROM setting WHERE key='selected_auth_account_id'), '')",
                [],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(selected_auth_account, target_account);
        let request_after: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture
                 WHERE target_account_id=?1",
                [&target_account],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(request_after, request_before);
    }

    #[test]
    fn app_scan_ingests_raw_usage_without_mutating_frozen_game_state() {
        let temp = tempfile::tempdir().unwrap();
        let codex_root = temp.path().join("codex");
        let claude_root = temp.path().join("claude");
        std::fs::create_dir(&codex_root).unwrap();
        std::fs::create_dir(&claude_root).unwrap();

        let mut ledger = prepared_first_reset_ledger();
        ledger
            .capture_guest_shop_import_request(&format!("account:{USER_ID}"))
            .unwrap();
        let state = test_state(ledger);
        *state.config.lock().unwrap() = SourceConfig {
            codex_root: codex_root.clone(),
            claude_root,
            timezone: chrono_tz::UTC,
        };
        let source_timestamp = Utc::now().to_rfc3339();
        let source_row = serde_json::json!({
            "timestamp": source_timestamp,
            "type": "token_usage_record",
            "payload": {
                "session_id": "frozen-v2-scan",
                "response_id": "append-1",
                "usage": {"total_tokens": 3_200_000}
            }
        });
        std::fs::write(codex_root.join("session.jsonl"), format!("{source_row}\n")).unwrap();

        let ledger = state.ledger.lock().unwrap();
        let capture_before: String = ledger
            .connection
            .query_row(
                "SELECT request_json FROM guest_shop_import_v2_capture
                 WHERE target_account_id=?1",
                [format!("account:{USER_ID}")],
                |row| row.get(0),
            )
            .unwrap();
        let journal_before: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM growth_journal_entry WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let contribution_before: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_effect_contribution WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let reward_before: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM shop_game_reward WHERE account_id='local'",
                [],
                |row| row.get(0),
            )
            .unwrap();
        let objects_before = ledger.planet_objects().unwrap();
        drop(ledger);

        let snapshot = state.scan().unwrap();

        let ledger = state.ledger.lock().unwrap();
        assert_eq!(snapshot.usage.codex.total_tokens, Some(4_200_000));
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM usage_record
                     WHERE event_key='codex:response:frozen-v2-scan:append-1'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            1,
            "raw source usage must be stored during a pending capture"
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM growth_journal_entry WHERE account_id='local'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            journal_before
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_effect_contribution WHERE account_id='local'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            contribution_before
        );
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM shop_game_reward WHERE account_id='local'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            reward_before
        );
        assert_eq!(ledger.planet_objects().unwrap(), objects_before);
        assert_eq!(
            ledger
                .connection
                .query_row::<String, _, _>(
                    "SELECT request_json FROM guest_shop_import_v2_capture
                     WHERE target_account_id=?1",
                    [format!("account:{USER_ID}")],
                    |row| row.get(0),
                )
                .unwrap(),
            capture_before
        );
    }

    #[test]
    fn first_login_guest_shop_hold_preserves_local_account_raw_usage_profile_and_snapshot() {
        let mut ledger = Ledger::open(Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger
            .set_planet_profile("Guest planet", PlanetAvatar::Masculine)
            .unwrap();
        insert_local_usage(&mut ledger, "guest-owned-usage");
        insert_local_shop_instance(&ledger);
        let original_cycle = ledger.planet_cycle_id().unwrap();
        let original_usage = ledger.planet_usage_totals().unwrap();
        let original_profile = ledger.planet_profile().unwrap();

        let state = test_state(ledger);
        state.scan().unwrap();
        let original_snapshot =
            serde_json::to_value(state.latest.lock().unwrap().as_ref().unwrap()).unwrap();

        let result = state.switch_planet_account(USER_ID);

        assert!(
            result.is_err(),
            "first-login account transition must hold for guest shop state"
        );
        let ledger = state.ledger.lock().unwrap();
        assert_eq!(ledger.cosmetic_account_id().unwrap(), "local");
        assert_eq!(ledger.planet_cycle_id().unwrap(), original_cycle);
        assert_eq!(ledger.planet_usage_totals().unwrap(), original_usage);
        assert_eq!(ledger.planet_profile().unwrap(), original_profile);
        assert_eq!(
            ledger
                .connection
                .query_row::<String, _, _>(
                    "SELECT account_id FROM planet_usage_owner WHERE event_key='guest-owned-usage'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            "local"
        );
        assert_eq!(ledger.connection.query_row::<i64, _, _>(
            "SELECT count(*) FROM shop_landscape_instance WHERE account_id='local' AND instance_id='guest-tree'",
            [],
            |row| row.get(0),
        ).unwrap(), 1);
        assert_eq!(
            ledger
                .connection
                .query_row::<i64, _, _>(
                    "SELECT count(*) FROM planet_account_state WHERE account_id=?1",
                    [format!("account:{USER_ID}")],
                    |row| row.get(0),
                )
                .unwrap(),
            0
        );
        let current_snapshot =
            serde_json::to_value(state.latest.lock().unwrap().as_ref().unwrap()).unwrap();
        assert_eq!(
            current_snapshot, original_snapshot,
            "a denied transition must not invalidate the local scan snapshot"
        );
    }

    #[test]
    fn pending_guest_restore_keeps_local_scan_available_and_other_restore_errors_visible() {
        let ledger = Ledger::open(Path::new(":memory:"), chrono_tz::UTC).unwrap();
        insert_local_shop_instance(&ledger);
        let original_cycle = ledger.planet_cycle_id().unwrap();
        let state = test_state(ledger);

        let restore = state.switch_planet_account(USER_ID).map(|_| ());
        allow_local_scan_on_guest_shop_hold(restore).unwrap();
        let snapshot = state.scan().unwrap();

        assert_eq!(snapshot.planet.current_cycle_id, original_cycle);
        assert_eq!(
            state.ledger.lock().unwrap().cosmetic_account_id().unwrap(),
            "local"
        );
        assert!(state.latest.lock().unwrap().is_some());
        assert_eq!(
            allow_local_scan_on_guest_shop_hold(Err(PlanetAccountSwitchError::Other(
                "keychain failure".into(),
            ))),
            Err("keychain failure".into()),
            "scan-only restore must not swallow real keychain errors",
        );
    }

    #[test]
    fn raw_only_local_usage_and_zero_revision_shop_baseline_allow_first_login() {
        let mut ledger = Ledger::open(Path::new(":memory:"), chrono_tz::UTC).unwrap();
        insert_local_usage(&mut ledger, "raw-only-usage");
        assert!(!ledger.has_local_guest_shop_state().unwrap());
        let state = test_state(ledger);

        assert!(state.switch_planet_account(USER_ID).unwrap());

        let ledger = state.ledger.lock().unwrap();
        let account_id = format!("account:{USER_ID}");
        assert_eq!(ledger.cosmetic_account_id().unwrap(), account_id);
        assert_eq!(
            ledger
                .connection
                .query_row::<String, _, _>(
                    "SELECT account_id FROM planet_usage_owner WHERE event_key='raw-only-usage'",
                    [],
                    |row| row.get(0),
                )
                .unwrap(),
            account_id
        );
    }

    #[test]
    fn already_active_signed_account_can_restore_even_with_local_guest_rows() {
        let mut ledger = Ledger::open(Path::new(":memory:"), chrono_tz::UTC).unwrap();
        ledger.ensure_planet_account(USER_ID).unwrap();
        insert_local_shop_instance(&ledger);
        let state = test_state(ledger);

        assert!(!state.switch_planet_account(USER_ID).unwrap());
        assert_eq!(
            state.ledger.lock().unwrap().cosmetic_account_id().unwrap(),
            format!("account:{USER_ID}")
        );
    }
}

impl AppState {
    pub(crate) fn select_planet_account(&self, user_id: &str) -> Result<(), String> {
        self.select_planet_account_with_outcome(user_id)
            .map_err(PlanetAccountSwitchError::into_message)
    }

    pub(crate) fn select_planet_account_with_outcome(
        &self,
        user_id: &str,
    ) -> Result<(), PlanetAccountSwitchError> {
        let authenticated_account_id = format!("account:{user_id}");
        self.ledger
            .lock()
            .map_err(|_| PlanetAccountSwitchError::Other("행성 계정을 확인할 수 없습니다".into()))?
            .set_selected_auth_account(&authenticated_account_id)
            .map_err(|_| {
                PlanetAccountSwitchError::Other("인증 계정을 저장할 수 없습니다".into())
            })?;
        if self.switch_planet_account(user_id)? {
            self.scan()
                .map_err(|error| PlanetAccountSwitchError::Other(error.into()))?;
        }
        Ok(())
    }

    fn switch_planet_account(&self, user_id: &str) -> Result<bool, PlanetAccountSwitchError> {
        let mut ledger = self
            .ledger
            .lock()
            .map_err(|_| PlanetAccountSwitchError::Other("local ledger unavailable".into()))?;
        let current_account = ledger.cosmetic_account_id().map_err(|_| {
            PlanetAccountSwitchError::Other("행성 계정을 확인할 수 없습니다".into())
        })?;
        let target_account = format!("account:{user_id}");
        if current_account != target_account {
            if ledger
                .guest_shop_import_v2_target()
                .map_err(|_| {
                    PlanetAccountSwitchError::Other(
                        "게스트 상점 가져오기 상태를 확인할 수 없습니다".into(),
                    )
                })?
                .is_some()
            {
                return Err(PlanetAccountSwitchError::GuestShopImportPending);
            }
            if current_account == "local"
                && ledger.has_guest_shop_import_v2_candidate().map_err(|_| {
                    PlanetAccountSwitchError::Other(
                        "게스트 상점 가져오기 원본을 확인할 수 없습니다".into(),
                    )
                })?
            {
                ledger
                    .capture_guest_shop_import_request(&target_account)
                    .map_err(|_| {
                        PlanetAccountSwitchError::Other(
                            "게스트 상점 가져오기 요청을 저장할 수 없습니다".into(),
                        )
                    })?;
                return Err(PlanetAccountSwitchError::GuestShopImportPending);
            }
            if sync::worker::guest_shop_import_pending(&ledger)
                .map_err(PlanetAccountSwitchError::Other)?
            {
                return Err(PlanetAccountSwitchError::GuestShopImportPending);
            }
        }
        let changed = ledger.ensure_planet_account(user_id).map_err(|_| {
            PlanetAccountSwitchError::Other("행성 계정을 변경할 수 없습니다".into())
        })?;
        drop(ledger);
        if changed {
            // Invalidate the previous account's frontend snapshot before any
            // fallible scan, so it can never become an upload for this account.
            *self.latest.lock().map_err(|_| {
                PlanetAccountSwitchError::Other("usage status unavailable".into())
            })? = None;
            self.usage_scan_failed.store(false, Ordering::SeqCst);
        }
        Ok(changed)
    }

    pub(crate) fn scan(&self) -> Result<WorldSnapshot, String> {
        let result = self.scan_inner();
        self.usage_scan_failed
            .store(result.is_err(), Ordering::SeqCst);
        result
    }

    fn scan_inner(&self) -> Result<WorldSnapshot, String> {
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
        ledger
            .settle_guest_rewards(chrono::Utc::now())
            .map_err(|_| "shop rewards unavailable")?;
        let snapshot = world_snapshot(&ledger, summary).map_err(|_| "world growth unavailable")?;
        *self.latest.lock().map_err(|_| "usage status unavailable")? = Some(snapshot.clone());
        Ok(snapshot)
    }

    pub(crate) fn rebuild_snapshot_from_latest_usage(&self) -> Result<WorldSnapshot, String> {
        let mut ledger = self.ledger.lock().map_err(|_| "local ledger unavailable")?;
        if self.usage_scan_failed.load(Ordering::SeqCst) {
            return Err("usage scan unavailable".into());
        }
        let usage = self
            .latest
            .lock()
            .map_err(|_| "usage status unavailable")?
            .as_ref()
            .map(|snapshot| snapshot.usage.clone());
        let Some(usage) = usage else {
            drop(ledger);
            return self.scan();
        };
        ledger
            .prepare_growth_journal()
            .map_err(|_| "growth journal unavailable")?;
        ledger
            .settle_guest_rewards(chrono::Utc::now())
            .map_err(|_| "shop rewards unavailable")?;
        let snapshot = world_snapshot(&ledger, usage).map_err(|_| "world growth unavailable")?;
        *self.latest.lock().map_err(|_| "usage status unavailable")? = Some(snapshot.clone());
        Ok(snapshot)
    }

    pub(crate) fn has_usage_scan_failure(&self) -> bool {
        self.usage_scan_failed.load(Ordering::SeqCst)
    }
}

#[derive(Debug, Eq, PartialEq)]
pub(crate) enum PlanetAccountSwitchError {
    GuestShopImportPending,
    Other(String),
}

impl PlanetAccountSwitchError {
    pub(crate) fn into_message(self) -> String {
        match self {
            Self::GuestShopImportPending => {
                "게스트 상점 가져오기를 완료한 뒤 계정을 전환할 수 있습니다".into()
            }
            Self::Other(message) => message,
        }
    }
}

fn restore_saved_planet_account_inner(state: &AppState) -> Result<(), PlanetAccountSwitchError> {
    let Some(config) = AuthConfig::from_env() else {
        return Ok(());
    };
    let store = SessionStore::new(&config)
        .map_err(|_| PlanetAccountSwitchError::Other("로그인 정보 오류".into()))?;
    if let Some(saved) = store
        .load()
        .map_err(|_| PlanetAccountSwitchError::Other("로그인 정보 오류".into()))?
    {
        state.switch_planet_account(&saved.user.id)?;
    }
    Ok(())
}

fn restore_saved_planet_account(state: &AppState) -> Result<(), String> {
    restore_saved_planet_account_inner(state).map_err(PlanetAccountSwitchError::into_message)
}

fn allow_local_scan_on_guest_shop_hold(
    restore_result: Result<(), PlanetAccountSwitchError>,
) -> Result<(), String> {
    match restore_result {
        Ok(()) | Err(PlanetAccountSwitchError::GuestShopImportPending) => Ok(()),
        Err(PlanetAccountSwitchError::Other(message)) => Err(message),
    }
}

fn restore_saved_planet_account_for_scan(state: &AppState) -> Result<(), String> {
    allow_local_scan_on_guest_shop_hold(restore_saved_planet_account_inner(state))
}

#[tauri::command]
async fn refresh_usage(app: AppHandle) -> Result<WorldSnapshot, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let state = app.state::<AppState>();
        restore_saved_planet_account_for_scan(&state)?;
        let snapshot = state.scan()?;
        let _ = platform::tray::refresh_status(&app, &snapshot);
        Ok(snapshot)
    })
    .await
    .map_err(|_| "사용량 새로고침 작업을 완료하지 못했습니다".to_string())?
}

#[tauri::command]
fn current_usage(state: State<'_, AppState>) -> Result<Option<WorldSnapshot>, String> {
    let snapshot = state
        .latest
        .lock()
        .map_err(|_| "usage status unavailable")?
        .clone();
    if snapshot.is_none() && state.usage_scan_failed.load(Ordering::SeqCst) {
        return Err("usage scan unavailable".into());
    }
    Ok(snapshot)
}

#[tauri::command]
fn set_planet_profile(
    nickname: String,
    avatar: PlanetAvatar,
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<WorldSnapshot, String> {
    restore_saved_planet_account(&state)?;
    state
        .ledger
        .lock()
        .map_err(|_| "local ledger unavailable")?
        .set_planet_profile(&nickname, avatar)
        .map_err(|_| "행성 프로필을 저장할 수 없습니다")?;
    let snapshot = state.scan()?;
    let _ = platform::tray::refresh_status(&app, &snapshot);
    transition_window_mode(&app, WindowMode::Popup, platform::tray::icon_rect(&app))?;
    Ok(snapshot)
}

fn with_guest_planet_reset_authority<T>(
    account_id: &str,
    action: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    if account_id.starts_with("account:") {
        return Err("로그인된 행성은 서버에서 초기화해야 합니다".into());
    }
    action()
}

fn require_signed_reset_context(
    state: &AppState,
    user_id: &str,
    expected_cycle_id: &str,
) -> Result<(), String> {
    let ledger = state.ledger.lock().map_err(|_| "행성 상태 오류")?;
    if ledger
        .cosmetic_account_id()
        .map_err(|_| "행성 계정을 확인할 수 없습니다")?
        != format!("account:{user_id}")
        || ledger
            .planet_cycle_id()
            .map_err(|_| "행성 주기를 확인할 수 없습니다")?
            != expected_cycle_id
    {
        return Err("계정 또는 행성 주기가 변경되어 초기화를 중단했습니다".into());
    }
    Ok(())
}

#[derive(serde::Serialize)]
#[serde(untagged)]
enum ResetPlanetCommandError {
    Message(String),
    ConfirmedViewUnavailable {
        kind: &'static str,
        account_id: String,
        request_id: String,
        expected_old_cycle_id: String,
        new_cycle_id: String,
    },
}

impl From<String> for ResetPlanetCommandError {
    fn from(message: String) -> Self {
        Self::Message(message)
    }
}

impl From<&str> for ResetPlanetCommandError {
    fn from(message: &str) -> Self {
        Self::Message(message.to_owned())
    }
}

fn signed_reset_snapshot(
    completion: sync::worker::SignedResetCompletion,
) -> Result<WorldSnapshot, ResetPlanetCommandError> {
    match completion {
        sync::worker::SignedResetCompletion::Confirmed(snapshot) => Ok(snapshot),
        sync::worker::SignedResetCompletion::ConfirmedViewUnavailable {
            account_id,
            request_id,
            expected_old_cycle_id,
            new_cycle_id,
        } => Err(ResetPlanetCommandError::ConfirmedViewUnavailable {
            kind: "confirmed_reset_view_unavailable",
            account_id,
            request_id: request_id.to_string(),
            expected_old_cycle_id,
            new_cycle_id,
        }),
    }
}

#[tauri::command]
async fn reset_planet(
    state: State<'_, AppState>,
    app: AppHandle,
) -> Result<WorldSnapshot, ResetPlanetCommandError> {
    let _gate = state.sync_gate.lock().await;
    restore_saved_planet_account(&state)?;
    let account_id = {
        let ledger = state
            .ledger
            .lock()
            .map_err(|_| "local ledger unavailable")?;
        ledger
            .cosmetic_account_id()
            .map_err(|_| "local ledger unavailable".to_string())?
    };
    if account_id.starts_with("account:") {
        let user_id = account_id
            .strip_prefix("account:")
            .filter(|user_id| !user_id.is_empty())
            .ok_or_else(|| "로그인 행성 계정을 확인할 수 없습니다".to_string())?;
        let expected_cycle_id = state
            .ledger
            .lock()
            .map_err(|_| "행성 상태 오류")?
            .planet_cycle_id()
            .map_err(|_| "행성 주기를 확인할 수 없습니다")?;
        require_signed_reset_context(&state, user_id, &expected_cycle_id)?;

        let config = AuthConfig::from_env()
            .ok_or_else(|| "로그인 초기화 설정을 확인할 수 없습니다".to_string())?;
        let store = SessionStore::new(&config).map_err(|_| "로그인 정보를 확인할 수 없습니다")?;
        let saved = store
            .load()
            .map_err(|_| "로그인 정보를 확인할 수 없습니다")?
            .ok_or_else(|| "로그인 세션을 확인할 수 없습니다".to_string())?;
        if saved.user.id != user_id {
            return Err("로그인 계정이 행성 계정과 일치하지 않습니다".into());
        }
        let session = SupabaseAuthClient::new(config.clone())
            .session(&store)
            .await
            .map_err(|_| "로그인 세션을 확인할 수 없습니다")?;
        if session.user.id != user_id {
            return Err("로그인 계정이 행성 계정과 일치하지 않습니다".into());
        }
        require_signed_reset_context(&state, user_id, &expected_cycle_id)?;

        let client = SupabaseSyncClient::new(&config.base_url, &config.publishable_key);
        if let Some(completion) = sync::worker::recover_pending_signed_reset(
            &state,
            user_id,
            &session.access_token,
            &client,
        )
        .await?
        {
            let snapshot = signed_reset_snapshot(completion)?;
            let _ = platform::tray::refresh_status(&app, &snapshot);
            return Ok(snapshot);
        }

        let world = client
            .current_world(&session.access_token)
            .await
            .map_err(|_| "행성 초기화 동기화 정책을 확인할 수 없습니다")?;
        require_signed_reset_context(&state, user_id, &expected_cycle_id)?;
        let world_policy_paused = if let Some(world) = world {
            let policy = client
                .my_sync_policy(&session.access_token, &world.id)
                .await
                .map_err(|_| "행성 초기화 동기화 정책을 확인할 수 없습니다")?;
            require_signed_reset_context(&state, user_id, &expected_cycle_id)?;
            policy.paused
        } else {
            false
        };
        let completion = sync::worker::reset_signed_planet(
            &state,
            user_id,
            &session.access_token,
            &client,
            world_policy_paused,
        )
        .await?;
        let snapshot = signed_reset_snapshot(completion)?;
        let _ = platform::tray::refresh_status(&app, &snapshot);
        return Ok(snapshot);
    }
    let _ = with_guest_planet_reset_authority(&account_id, || state.scan())?;
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
    let snapshot = state.rebuild_snapshot_from_latest_usage()?;
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
    restore_saved_planet_account_for_scan(&state)?;
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
    restore_saved_planet_account_for_scan(&state)?;
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
    restore_saved_planet_account_for_scan(&state)?;
    let snapshot = state.scan()?;
    let _ = platform::tray::refresh_status(&app, &snapshot);
    Ok(Some(snapshot))
}

#[tauri::command]
fn set_detail_view(detail: bool, app: AppHandle) -> Result<(), String> {
    let target = if detail {
        WindowMode::Detail
    } else {
        WindowMode::Popup
    };
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
            commands::cosmetic_shop::get_legacy_cosmetic_shop_state,
            commands::cosmetic_shop::get_shop_state,
            commands::cosmetic_shop::quote_shop_action,
            commands::cosmetic_shop::apply_shop_action,
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
                usage_scan_failed: AtomicBool::new(false),
                sync_failed: Mutex::new(false),
                sync_gate: tokio::sync::Mutex::new(()),
                window_mode: Mutex::new(WindowMode::Popup),
                mode_transitioning: AtomicBool::new(false),
                tray_press_pending: AtomicBool::new(false),
            };
            app.manage(state);
            let handle = app.handle().clone();
            handle
                .clone()
                .run_on_main_thread(move || {
                    let _ = transition_window_mode(
                        &handle,
                        WindowMode::Popup,
                        platform::tray::icon_rect(&handle),
                    );
                    let worker_handle = handle.clone();
                    std::thread::spawn(move || {
                        let state = worker_handle.state::<AppState>();
                        let initial_scan = restore_saved_planet_account_for_scan(&state)
                            .and_then(|_| state.scan());
                        state
                            .usage_scan_failed
                            .store(initial_scan.is_err(), Ordering::SeqCst);
                        let startup_mode = initial_mode_after_scan(
                            initial_scan
                                .as_ref()
                                .map(|snapshot| snapshot.planet.profile.is_some()),
                        );
                        match &initial_scan {
                            Ok(snapshot) => {
                                let _ = platform::tray::refresh_status(&worker_handle, snapshot);
                                let _ = worker_handle.emit("usage-updated", snapshot);
                            }
                            Err(error) => {
                                let _ = worker_handle.emit("usage-scan-failed", error);
                            }
                        }
                        let mode_handle = worker_handle.clone();
                        let mode_dispatch = mode_handle.clone();
                        let _ = mode_dispatch.run_on_main_thread(move || {
                            let icon_rect = (startup_mode == WindowMode::Popup)
                                .then(|| platform::tray::icon_rect(&mode_handle))
                                .flatten();
                            let _ = transition_startup_window_mode(
                                &mode_handle,
                                startup_mode,
                                icon_rect,
                            );
                        });

                        let mut retry = sync::worker::RetryDelay::default();
                        let mut first_cycle = true;
                        loop {
                            let state = worker_handle.state::<AppState>();
                            if !first_cycle {
                                match restore_saved_planet_account_for_scan(&state)
                                    .and_then(|_| state.scan())
                                {
                                    Ok(snapshot) => {
                                        let _ = platform::tray::refresh_status(
                                            &worker_handle,
                                            &snapshot,
                                        );
                                        let _ = worker_handle.emit("usage-updated", snapshot);
                                    }
                                    Err(error) => {
                                        state.usage_scan_failed.store(true, Ordering::SeqCst);
                                        let _ = worker_handle.emit("usage-scan-failed", error);
                                    }
                                }
                            }
                            first_cycle = false;
                            let result =
                                tauri::async_runtime::block_on(sync::worker::sync_once(&state));
                            if let Ok(mut failed) = state.sync_failed.lock() {
                                *failed = result.is_err();
                            }
                            let _ = worker_handle.emit("sync-status-updated", ());
                            std::thread::sleep(retry.next_after(result.is_ok()));
                        }
                    });
                })
                .map_err(io::Error::other)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if window.label() != "main" {
                return;
            }
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            } else if let WindowEvent::Focused(false) = event {
                let Some(state) = window.app_handle().try_state::<AppState>() else {
                    return;
                };
                if !state.mode_transitioning.load(Ordering::SeqCst)
                    && state.window_mode.lock().is_ok_and(|mode| {
                        should_hide_on_blur(*mode, state.tray_press_pending.load(Ordering::SeqCst))
                    })
                {
                    let _ = window.hide();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
