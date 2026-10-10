use crate::collectors::discovery::{
    prepare_default_source_config, ScanSummary, SourceConfig, SourceHealth,
};
use crate::domain::device_reset::{
    DeviceResetPhase, DeviceResetResult, DeviceResetState, DeviceResetView, LocalCommandError,
    LocalContext, LocalEnvelope,
};
use crate::domain::usage::{TokenUsage, UsageCoverage};
use crate::sync::auth::{AuthConfig, LocalSessionRemover, SessionStore, SESSION_GATE};
use crate::{growth::world_snapshot, AppState};
use std::sync::atomic::Ordering;
use tauri::{AppHandle, Emitter, State};

pub enum DeviceResetAction {
    Start(LocalContext),
    Retry {
        request_id: String,
        context: LocalContext,
    },
    Recover,
}

fn view(state: DeviceResetState) -> DeviceResetView {
    DeviceResetView {
        actions_blocked: matches!(
            state.phase,
            DeviceResetPhase::Pending | DeviceResetPhase::LocalCommitted
        ),
        storage_completed: matches!(
            state.phase,
            DeviceResetPhase::LocalCommitted | DeviceResetPhase::Completed
        ),
        state,
    }
}

fn empty_usage() -> ScanSummary {
    let usage = TokenUsage {
        input_tokens: None,
        output_tokens: None,
        cache_read_tokens: None,
        cache_write_tokens: None,
        total_tokens: None,
        coverage: UsageCoverage::Unavailable,
    };
    ScanSummary {
        codex: usage.clone(),
        claude_code: usage,
        codex_source: SourceHealth::NotFound,
        claude_code_source: SourceHealth::NotFound,
        confirmed_subtotal: None,
        complete_total: None,
        scanned_at_utc: chrono::Utc::now(),
    }
}

pub async fn run_device_reset(
    state: &AppState,
    session_remover: &dyn LocalSessionRemover,
    action: DeviceResetAction,
    prepare_config: impl FnOnce(chrono_tz::Tz) -> Result<SourceConfig, String>,
) -> Result<DeviceResetResult, LocalCommandError> {
    let mut permit = match &action {
        DeviceResetAction::Start(context) | DeviceResetAction::Retry { context, .. } => {
            state.lifecycle.enter_reset(context.generation).await?
        }
        DeviceResetAction::Recover => state.lifecycle.enter_recovery().await,
    };
    let _sync = state.sync_gate.lock().await;
    let _session = SESSION_GATE.lock().await;
    let mut reset = match state
        .ledger
        .lock()
        .map_err(|_| {
            LocalCommandError::new(
                permit.generation(),
                "reset_recovery_required",
                "초기화 저장소를 읽을 수 없습니다",
            )
        })?
        .device_reset_state()
    {
        Ok(reset) => reset,
        Err(_) => {
            permit.block();
            return Err(LocalCommandError::new(
                permit.generation(),
                "reset_recovery_required",
                "초기화 상태를 확인할 수 없습니다",
            ));
        }
    };
    if matches!(action, DeviceResetAction::Recover)
        && matches!(
            reset.phase,
            DeviceResetPhase::Idle | DeviceResetPhase::Completed
        )
    {
        permit.update(&reset);
        return Ok(DeviceResetResult {
            storage_completed: reset.phase == DeviceResetPhase::Completed,
            state: reset,
        });
    }
    let service_id = session_remover.service_id().map_err(|_| {
        LocalCommandError::new(
            permit.generation(),
            "command_failed",
            "대상 보안 저장소를 확인할 수 없습니다",
        )
    })?;
    let timezone = state
        .ledger
        .lock()
        .map_err(|_| {
            LocalCommandError::new(permit.generation(), "command_failed", "초기화 저장소 오류")
        })?
        .timezone;
    let prepared_config = prepare_config(timezone).map_err(|message| {
        LocalCommandError::new(permit.generation(), "command_failed", message)
    })?;
    {
        let ledger = state.ledger.lock().map_err(|_| {
            LocalCommandError::new(permit.generation(), "command_failed", "초기화 저장소 오류")
        })?;
        let current_config = state.config.lock().map_err(|_| {
            LocalCommandError::new(
                permit.generation(),
                "reset_recovery_required",
                "소스 설정을 읽을 수 없습니다",
            )
        })?;
        if prepared_config.timezone != ledger.timezone || current_config.timezone != ledger.timezone
        {
            return Err(LocalCommandError::new(
                permit.generation(),
                "command_failed",
                "저장소와 소스 시간대가 일치하지 않습니다",
            ));
        }
    }
    match &action {
        DeviceResetAction::Start(context) => {
            if matches!(
                reset.phase,
                DeviceResetPhase::Pending | DeviceResetPhase::LocalCommitted
            ) {
                return Err(LocalCommandError::new(
                    reset.generation,
                    "reset_recovery_required",
                    "진행 중인 초기화를 재시도해 주세요",
                ));
            }
            reset = state
                .ledger
                .lock()
                .map_err(|_| {
                    LocalCommandError::new(
                        permit.generation(),
                        "command_failed",
                        "초기화 저장소 오류",
                    )
                })?
                .prepare_device_reset(context.generation, &service_id, chrono::Utc::now())
                .map_err(|_| {
                    LocalCommandError::new(
                        permit.generation(),
                        "command_failed",
                        "기기 초기화를 준비할 수 없습니다",
                    )
                })?;
            permit.update(&reset);
        }
        DeviceResetAction::Retry { request_id, .. } => {
            if reset.request_id.as_deref() != Some(request_id) {
                return Err(LocalCommandError::new(
                    reset.generation,
                    "command_failed",
                    "초기화 요청이 변경되었습니다",
                ));
            }
        }
        DeviceResetAction::Recover => {}
    }
    if reset.service_id.as_deref() != Some(&service_id) {
        return Err(LocalCommandError::new(
            reset.generation,
            "reset_recovery_required",
            "초기화 대상 보안 저장소가 일치하지 않습니다",
        ));
    }
    let request_id = reset.request_id.clone().ok_or_else(|| {
        LocalCommandError::new(
            reset.generation,
            "reset_recovery_required",
            "초기화 요청을 확인할 수 없습니다",
        )
    })?;
    if reset.phase == DeviceResetPhase::Pending {
        session_remover.remove_local_session().map_err(|_| {
            LocalCommandError::new(
                reset.generation,
                "reset_recovery_required",
                "로컬 로그아웃에 실패했습니다. 같은 초기화를 재시도해 주세요",
            )
        })?;
        reset = state
            .ledger
            .lock()
            .map_err(|_| {
                LocalCommandError::new(
                    reset.generation,
                    "reset_recovery_required",
                    "초기화 저장소 오류",
                )
            })?
            .commit_device_reset(&request_id)
            .map_err(|_| {
                LocalCommandError::new(
                    reset.generation,
                    "reset_recovery_required",
                    "로그아웃은 완료됐지만 데이터 초기화를 복구해야 합니다",
                )
            })?;
        permit.update(&reset);
    }
    if reset.phase == DeviceResetPhase::LocalCommitted {
        *state.config.lock().map_err(|_| {
            LocalCommandError::new(
                reset.generation,
                "reset_recovery_required",
                "새 소스 설정을 복구해야 합니다",
            )
        })? = prepared_config;
        let snapshot = {
            let ledger = state.ledger.lock().map_err(|_| {
                LocalCommandError::new(
                    reset.generation,
                    "reset_recovery_required",
                    "새 행성 저장소 오류",
                )
            })?;
            world_snapshot(&ledger, empty_usage()).map_err(|_| {
                LocalCommandError::new(
                    reset.generation,
                    "reset_recovery_required",
                    "새 행성 화면을 복구해야 합니다",
                )
            })?
        };
        *state.latest.lock().map_err(|_| {
            LocalCommandError::new(
                reset.generation,
                "reset_recovery_required",
                "새 행성 화면을 복구해야 합니다",
            )
        })? = Some(snapshot);
        state.usage_scan_failed.store(false, Ordering::SeqCst);
        *state.sync_failed.lock().map_err(|_| {
            LocalCommandError::new(
                reset.generation,
                "reset_recovery_required",
                "동기화 상태를 복구해야 합니다",
            )
        })? = false;
        reset = state
            .ledger
            .lock()
            .map_err(|_| {
                LocalCommandError::new(
                    reset.generation,
                    "reset_recovery_required",
                    "초기화 완료 기록 오류",
                )
            })?
            .complete_device_reset(&request_id)
            .map_err(|_| {
                LocalCommandError::new(
                    reset.generation,
                    "reset_recovery_required",
                    "초기화 완료 기록을 복구해야 합니다",
                )
            })?;
    }
    permit.update(&reset);
    Ok(DeviceResetResult {
        state: reset,
        storage_completed: true,
    })
}

fn saved_remover(state: &AppState, generation: u64) -> Result<SessionStore, LocalCommandError> {
    let reset = state
        .ledger
        .lock()
        .map_err(|_| LocalCommandError::new(generation, "reset_recovery_required", "저장소 오류"))?
        .device_reset_state()
        .map_err(|_| {
            LocalCommandError::new(generation, "reset_recovery_required", "초기화 상태 오류")
        })?;
    let service = reset.service_id.ok_or_else(|| {
        LocalCommandError::new(
            generation,
            "reset_recovery_required",
            "대상 보안 저장소를 확인할 수 없습니다",
        )
    })?;
    SessionStore::from_service_id(&service).map_err(|_| {
        LocalCommandError::new(
            generation,
            "reset_recovery_required",
            "대상 보안 저장소를 열 수 없습니다",
        )
    })
}
fn notify(app: &AppHandle, result: &DeviceResetResult) {
    let _ = app.emit(
        "device-reset-updated",
        LocalEnvelope {
            generation: result.state.generation,
            data: view(result.state.clone()),
        },
    );
}

fn notify_current(app: &AppHandle, state: &AppState) {
    if let Ok(ledger) = state.ledger.lock() {
        if let Ok(reset) = ledger.device_reset_state() {
            let _ = app.emit(
                "device-reset-updated",
                LocalEnvelope {
                    generation: reset.generation,
                    data: view(reset),
                },
            );
        }
    }
}

#[tauri::command]
pub async fn get_device_reset_state(
    state: State<'_, AppState>,
) -> Result<LocalEnvelope<DeviceResetView>, LocalCommandError> {
    let generation = state.lifecycle.generation().await;
    let reset = state
        .ledger
        .lock()
        .map_err(|_| LocalCommandError::new(generation, "reset_recovery_required", "저장소 오류"))?
        .device_reset_state()
        .map_err(|_| {
            LocalCommandError::new(generation, "reset_recovery_required", "초기화 상태 오류")
        })?;
    Ok(LocalEnvelope {
        generation: reset.generation,
        data: view(reset),
    })
}
#[tauri::command]
pub async fn reset_device_data(
    state: State<'_, AppState>,
    app: AppHandle,
    context: LocalContext,
) -> Result<LocalEnvelope<DeviceResetResult>, LocalCommandError> {
    let auth = AuthConfig::from_env().ok_or_else(|| {
        LocalCommandError::new(
            context.generation,
            "command_failed",
            "현재 서비스 설정을 확인할 수 없습니다",
        )
    })?;
    let remover = SessionStore::new(&auth).map_err(|_| {
        LocalCommandError::new(
            context.generation,
            "command_failed",
            "대상 보안 저장소를 열 수 없습니다",
        )
    })?;
    let result = run_device_reset(
        &state,
        &remover,
        DeviceResetAction::Start(context),
        prepare_default_source_config,
    )
    .await;
    notify_current(&app, &state);
    let result = result?;
    Ok(LocalEnvelope {
        generation: result.state.generation,
        data: result,
    })
}
#[tauri::command]
pub async fn retry_device_reset(
    state: State<'_, AppState>,
    app: AppHandle,
    request_id: String,
    context: LocalContext,
) -> Result<LocalEnvelope<DeviceResetResult>, LocalCommandError> {
    let remover = saved_remover(&state, context.generation)?;
    let result = run_device_reset(
        &state,
        &remover,
        DeviceResetAction::Retry {
            request_id,
            context,
        },
        prepare_default_source_config,
    )
    .await;
    notify_current(&app, &state);
    let result = result?;
    Ok(LocalEnvelope {
        generation: result.state.generation,
        data: result,
    })
}

pub async fn recover_device_reset_before_startup(
    state: &AppState,
    app: &AppHandle,
) -> Result<DeviceResetView, LocalCommandError> {
    let generation = state.lifecycle.generation().await;
    let reset = state
        .ledger
        .lock()
        .map_err(|_| LocalCommandError::new(generation, "reset_recovery_required", "저장소 오류"))?
        .device_reset_state()
        .map_err(|_| {
            LocalCommandError::new(generation, "reset_recovery_required", "초기화 상태 오류")
        })?;
    if matches!(
        reset.phase,
        DeviceResetPhase::Idle | DeviceResetPhase::Completed
    ) {
        return Ok(view(reset));
    }
    let remover = saved_remover(state, generation)?;
    let result = run_device_reset(
        state,
        &remover,
        DeviceResetAction::Recover,
        prepare_default_source_config,
    )
    .await?;
    notify(app, &result);
    Ok(view(result.state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::device_reset::{DeviceResetPhase, DeviceResetState};
    use crate::sync::auth::AuthError;
    use crate::{lifecycle::LocalLifecycle, storage::ledger::Ledger, WindowMode};
    use std::sync::{
        atomic::{AtomicBool, Ordering},
        Mutex,
    };
    struct FakeKeyring {
        credential: Mutex<Option<String>>,
        fail: AtomicBool,
    }
    impl LocalSessionRemover for FakeKeyring {
        fn service_id(&self) -> Result<String, AuthError> {
            Ok("fake-service".into())
        }
        fn remove_local_session(&self) -> Result<(), AuthError> {
            if self.fail.load(Ordering::SeqCst) {
                return Err(AuthError::CredentialStore);
            }
            *self.credential.lock().unwrap() = None;
            Ok(())
        }
    }
    fn config() -> SourceConfig {
        SourceConfig {
            codex_root: "/isolated-default-codex".into(),
            claude_root: "/isolated-default-claude".into(),
            timezone: chrono_tz::UTC,
        }
    }
    fn state(ledger: Ledger) -> AppState {
        AppState {
            lifecycle: LocalLifecycle::new(&ledger.device_reset_state().unwrap()),
            config: Mutex::new(SourceConfig {
                codex_root: "/old-custom-codex".into(),
                ..config()
            }),
            ledger: Mutex::new(ledger),
            latest: Mutex::new(None),
            usage_scan_failed: AtomicBool::new(true),
            sync_failed: Mutex::new(true),
            sync_gate: tokio::sync::Mutex::new(()),
            window_mode: Mutex::new(WindowMode::Popup),
            mode_transitioning: AtomicBool::new(false),
            tray_press_pending: AtomicBool::new(false),
        }
    }
    fn keyring(fail: bool) -> FakeKeyring {
        FakeKeyring {
            credential: Mutex::new(Some("isolated-session".into())),
            fail: AtomicBool::new(fail),
        }
    }
    #[test]
    fn local_reset_success_reopen_preserves_all_timezone_holders() {
        tauri::async_runtime::block_on(async {
            let file = tempfile::NamedTempFile::new().unwrap();
            let timezone = chrono_tz::Asia::Seoul;
            let instance = state(Ledger::open(file.path(), timezone).unwrap());
            instance.config.lock().unwrap().timezone = timezone;
            let result = run_device_reset(
                &instance,
                &keyring(false),
                DeviceResetAction::Start(LocalContext { generation: 0 }),
                |tz| {
                    Ok(SourceConfig {
                        timezone: tz,
                        ..config()
                    })
                },
            )
            .await
            .unwrap();
            assert_eq!(result.state.phase, DeviceResetPhase::Completed);
            let assert_timezone = |instance: &AppState| {
                let ledger = instance.ledger.lock().unwrap();
                let saved: String = ledger
                    .connection
                    .query_row(
                        "SELECT value FROM setting WHERE key='world_timezone'",
                        [],
                        |r| r.get(0),
                    )
                    .unwrap();
                assert_eq!(saved, "Asia/Seoul");
                assert_eq!(ledger.timezone, timezone);
                assert_eq!(instance.config.lock().unwrap().timezone, timezone);
            };
            assert_timezone(&instance);
            drop(instance);
            let saved = Ledger::saved_timezone(file.path()).unwrap().unwrap();
            assert_eq!(saved, timezone);
            let reopened = state(Ledger::open(file.path(), saved).unwrap());
            *reopened.config.lock().unwrap() =
                crate::collectors::discovery::prepare_default_source_config(saved).unwrap();
            assert_eq!(
                reopened
                    .ledger
                    .lock()
                    .unwrap()
                    .device_reset_state()
                    .unwrap()
                    .phase,
                DeviceResetPhase::Completed
            );
            assert_timezone(&reopened);
        });
    }
    #[test]
    fn local_reset_coordinator_happy_path_without_scan() {
        tauri::async_runtime::block_on(async {
            let state =
                state(Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap());
            let keyring = keyring(false);
            let result = run_device_reset(
                &state,
                &keyring,
                DeviceResetAction::Start(LocalContext { generation: 0 }),
                |_| Ok(config()),
            )
            .await
            .unwrap();
            assert_eq!(result.state.phase, DeviceResetPhase::Completed);
            assert!(result.storage_completed);
            assert!(keyring.credential.lock().unwrap().is_none());
            assert_eq!(
                state.config.lock().unwrap().codex_root,
                std::path::PathBuf::from("/isolated-default-codex")
            );
            assert!(!state.usage_scan_failed.load(Ordering::SeqCst));
            assert!(!*state.sync_failed.lock().unwrap());
            assert!(state
                .latest
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .planet
                .profile
                .is_none());
            assert!(state.lifecycle.enter(1).await.is_ok());
        });
    }
    #[test]
    fn local_reset_coordinator_keyring_failure_blocks_old_account_until_retry() {
        tauri::async_runtime::block_on(async {
            let state =
                state(Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap());
            let old_cycle = state.ledger.lock().unwrap().planet_cycle_id().unwrap();
            let keyring = keyring(true);
            assert!(run_device_reset(
                &state,
                &keyring,
                DeviceResetAction::Start(LocalContext { generation: 0 }),
                |_| Ok(config())
            )
            .await
            .is_err());
            let pending = state.ledger.lock().unwrap().device_reset_state().unwrap();
            assert_eq!(pending.phase, DeviceResetPhase::Pending);
            assert_eq!(
                state.ledger.lock().unwrap().planet_cycle_id().unwrap(),
                old_cycle
            );
            assert_eq!(
                state.lifecycle.enter(1).await.err().unwrap().code,
                "reset_recovery_required"
            );
            keyring.fail.store(false, Ordering::SeqCst);
            let result = run_device_reset(
                &state,
                &keyring,
                DeviceResetAction::Retry {
                    request_id: pending.request_id.clone().unwrap(),
                    context: LocalContext { generation: 1 },
                },
                |_| Ok(config()),
            )
            .await
            .unwrap();
            assert_eq!(result.state.request_id, pending.request_id);
            assert_eq!(result.state.cutoff_at_utc, pending.cutoff_at_utc);
        });
    }
    #[test]
    fn local_reset_coordinator_preflight_failure_changes_nothing() {
        tauri::async_runtime::block_on(async {
            let state =
                state(Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap());
            let keyring = keyring(false);
            let invalid = SourceConfig {
                timezone: chrono_tz::Asia::Seoul,
                ..config()
            };
            assert!(run_device_reset(
                &state,
                &keyring,
                DeviceResetAction::Start(LocalContext { generation: 0 }),
                |_| Ok(invalid)
            )
            .await
            .is_err());
            assert_eq!(
                state.ledger.lock().unwrap().device_reset_state().unwrap(),
                DeviceResetState::default()
            );
            assert!(keyring.credential.lock().unwrap().is_some());
            assert!(state.lifecycle.enter(0).await.is_ok());
        });
    }

    #[test]
    fn local_reset_coordinator_logout_then_db_failure_restarts_same_request() {
        tauri::async_runtime::block_on(async {
            let folder = tempfile::tempdir().unwrap();
            let path = folder.path().join("isolated.sqlite3");
            let ledger = Ledger::open(&path, chrono_tz::UTC).unwrap();
            ledger.connection.execute_batch("DROP TABLE guest_provenance_meta; CREATE TABLE guest_provenance_meta(singleton INTEGER PRIMARY KEY,seed_allowed INTEGER CHECK(seed_allowed=1)); INSERT INTO guest_provenance_meta VALUES (1,1)").unwrap();
            let first = state(ledger);
            let keyring = keyring(false);
            assert!(run_device_reset(
                &first,
                &keyring,
                DeviceResetAction::Start(LocalContext { generation: 0 }),
                |_| Ok(config())
            )
            .await
            .is_err());
            assert!(keyring.credential.lock().unwrap().is_none());
            let pending = first.ledger.lock().unwrap().device_reset_state().unwrap();
            assert_eq!(pending.phase, DeviceResetPhase::Pending);
            first.ledger.lock().unwrap().connection.execute_batch("DROP TABLE guest_provenance_meta; CREATE TABLE guest_provenance_meta(singleton INTEGER PRIMARY KEY CHECK(singleton=1),seed_allowed INTEGER CHECK(seed_allowed IN (0,1))); INSERT INTO guest_provenance_meta VALUES (1,0)").unwrap();
            drop(first);
            let restarted = state(Ledger::open(&path, chrono_tz::UTC).unwrap());
            let result = run_device_reset(&restarted, &keyring, DeviceResetAction::Recover, |_| {
                Ok(config())
            })
            .await
            .unwrap();
            assert_eq!(result.state.request_id, pending.request_id);
            assert_eq!(result.state.new_cycle_id, pending.new_cycle_id);
            assert_eq!(result.state.cutoff_at_utc, pending.cutoff_at_utc);
            assert_eq!(result.state.phase, DeviceResetPhase::Completed);
            assert_eq!(
                restarted.config.lock().unwrap().codex_root,
                config().codex_root
            );
            assert_eq!(
                restarted
                    .latest
                    .lock()
                    .unwrap()
                    .as_ref()
                    .unwrap()
                    .planet_ordinal
                    .current,
                Some(1)
            );
            let cutoff = chrono::DateTime::parse_from_rfc3339(
                result.state.cutoff_at_utc.as_deref().unwrap(),
            )
            .unwrap()
            .with_timezone(&chrono::Utc);
            let mut ledger = restarted.ledger.lock().unwrap();
            for (key, at, tokens) in [
                ("old-source", cutoff, 100),
                ("new-source", cutoff + chrono::Duration::microseconds(1), 7),
            ] {
                ledger
                    .insert(&crate::collectors::ParsedRecord {
                        agent: crate::domain::usage::Agent::Codex,
                        kind: crate::collectors::RecordKind::Response,
                        event_key: key.into(),
                        occurred_at_utc: at,
                        usage: TokenUsage {
                            input_tokens: None,
                            output_tokens: None,
                            cache_read_tokens: None,
                            cache_write_tokens: None,
                            total_tokens: Some(tokens),
                            coverage: UsageCoverage::Complete,
                        },
                    })
                    .unwrap();
            }
            assert_eq!(
                ledger.planet_usage_totals().unwrap().1,
                7,
                "rescan may restore raw records but cannot restore old game contributions"
            );
            assert_eq!(
                ledger
                    .connection
                    .query_row("SELECT count(*) FROM usage_record", [], |row| row
                        .get::<_, i64>(0))
                    .unwrap(),
                2
            );
            assert_eq!(ledger.planet_ordinal().unwrap().current, Some(1));
            assert_eq!(ledger.connection.query_row("SELECT count(*) FROM sqlite_master WHERE type='trigger' AND name LIKE 'guest_provenance_%'",[],|row| row.get::<_,i64>(0)).unwrap(),7);
        });
    }
    #[test]
    fn local_reset_coordinator_local_committed_recovery_never_removes_session_again() {
        tauri::async_runtime::block_on(async {
            let mut ledger =
                Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
            let reset = ledger
                .prepare_device_reset(0, "fake-service", chrono::Utc::now())
                .unwrap();
            ledger
                .commit_device_reset(reset.request_id.as_deref().unwrap())
                .unwrap();
            let state = state(ledger);
            let keyring = keyring(true);
            let result = run_device_reset(&state, &keyring, DeviceResetAction::Recover, |_| {
                Ok(config())
            })
            .await
            .unwrap();
            assert_eq!(result.state.phase, DeviceResetPhase::Completed);
            assert!(keyring.credential.lock().unwrap().is_some());
            assert_eq!(result.state.new_cycle_id, reset.new_cycle_id);
        });
    }
    #[test]
    fn local_reset_coordinator_signed_cache_reset_and_double_click_stale() {
        tauri::async_runtime::block_on(async {
            let mut ledger =
                Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap();
            ledger
                .ensure_planet_account("11111111-1111-4111-8111-111111111111")
                .unwrap();
            let state = state(ledger);
            let keyring = keyring(false);
            let first = run_device_reset(
                &state,
                &keyring,
                DeviceResetAction::Start(LocalContext { generation: 0 }),
                |_| Ok(config()),
            )
            .await
            .unwrap();
            assert_eq!(
                state.ledger.lock().unwrap().cosmetic_account_id().unwrap(),
                "local"
            );
            let error = run_device_reset(
                &state,
                &keyring,
                DeviceResetAction::Start(LocalContext { generation: 0 }),
                |_| Ok(config()),
            )
            .await
            .unwrap_err();
            assert_eq!(error.code, "stale_generation");
            assert_eq!(
                state.ledger.lock().unwrap().device_reset_state().unwrap(),
                first.state
            );
        });
    }

    struct PoisonConfigRemover<'a>(&'a AppState);
    impl LocalSessionRemover for PoisonConfigRemover<'_> {
        fn service_id(&self) -> Result<String, AuthError> {
            Ok("fake-service".into())
        }
        fn remove_local_session(&self) -> Result<(), AuthError> {
            std::thread::scope(|scope| {
                let _ = scope
                    .spawn(|| {
                        let _guard = self.0.config.lock().unwrap();
                        panic!("isolated config fault");
                    })
                    .join();
            });
            Ok(())
        }
    }
    #[test]
    fn local_reset_coordinator_memory_failure_blocks_old_config_scan_and_recovers_without_delete() {
        tauri::async_runtime::block_on(async {
            let instance =
                state(Ledger::open(std::path::Path::new(":memory:"), chrono_tz::UTC).unwrap());
            let result = run_device_reset(
                &instance,
                &PoisonConfigRemover(&instance),
                DeviceResetAction::Start(LocalContext { generation: 0 }),
                |_| Ok(config()),
            )
            .await;
            assert!(result.is_err());
            let committed = instance
                .ledger
                .lock()
                .unwrap()
                .device_reset_state()
                .unwrap();
            assert_eq!(committed.phase, DeviceResetPhase::LocalCommitted);
            assert!(instance.scan_background().await.is_err());
            let restarted = state(instance.ledger.into_inner().unwrap());
            let keyring = keyring(true); // Any repeated removal would fail.
            let result = run_device_reset(&restarted, &keyring, DeviceResetAction::Recover, |_| {
                Ok(config())
            })
            .await
            .unwrap();
            assert_eq!(result.state.new_cycle_id, committed.new_cycle_id);
            assert_eq!(result.state.phase, DeviceResetPhase::Completed);
        });
    }
}
