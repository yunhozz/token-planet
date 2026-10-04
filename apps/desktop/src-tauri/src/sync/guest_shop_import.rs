use std::future::Future;

use crate::{
    domain::guest_shop_import::{
        GuestImportPhase, GuestImportSourceRelation, GuestShopImportV2Request,
        GuestShopImportV2Result,
    },
    storage::{
        guest_shop_import_v2::GuestImportCompletion as StorageGuestImportCompletion,
        PendingGuestShopImport,
    },
    sync::client::{SupabaseSyncClient, SyncError},
    AppState,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum GuestImportSyncOutcome {
    NoPending,
    Held,
    Imported,
    ImportedWithCorrectionHold,
}

pub(crate) trait GuestShopImportTransport: Send + Sync {
    fn import_guest_shop<'a>(
        &'a self,
        access_token: &'a str,
        request: &'a GuestShopImportV2Request,
    ) -> impl Future<Output = Result<GuestShopImportV2Result, SyncError>> + Send + 'a;
}

impl GuestShopImportTransport for SupabaseSyncClient {
    fn import_guest_shop<'a>(
        &'a self,
        access_token: &'a str,
        request: &'a GuestShopImportV2Request,
    ) -> impl Future<Output = Result<GuestShopImportV2Result, SyncError>> + Send + 'a {
        SupabaseSyncClient::import_guest_shop(self, access_token, request)
    }
}

pub(crate) async fn sync_pending_guest_shop_import(
    state: &AppState,
    client: &impl GuestShopImportTransport,
    access_token: &str,
    target: &str,
) -> Result<GuestImportSyncOutcome, String> {
    let request = {
        let mut ledger = state
            .ledger
            .lock()
            .map_err(|_| "게스트 가져오기 상태 오류")?;
        let pending = ledger
            .pending_guest_shop_import_request(target)
            .map_err(|_| "게스트 상점 가져오기 상태를 확인할 수 없습니다")?;
        let Some(pending) = pending else {
            return Ok(GuestImportSyncOutcome::NoPending);
        };
        let PendingGuestShopImport::V2(status) = pending else {
            return Ok(GuestImportSyncOutcome::Held);
        };

        match status.phase {
            GuestImportPhase::Held => return Ok(GuestImportSyncOutcome::Held),
            GuestImportPhase::Imported => {
                return Ok(if status.correction_hold {
                    GuestImportSyncOutcome::ImportedWithCorrectionHold
                } else {
                    GuestImportSyncOutcome::Imported
                });
            }
            GuestImportPhase::Captured | GuestImportPhase::AttemptStarted => {}
        }

        let selected_account: String = ledger
            .connection
            .query_row(
                "SELECT value FROM setting WHERE key='selected_auth_account_id'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| "선택한 계정의 게스트 가져오기 상태를 확인할 수 없습니다")?;
        if selected_account != target {
            return Ok(GuestImportSyncOutcome::Held);
        }

        if status.phase == GuestImportPhase::Captured
            && (status.correction_hold
                || matches!(
                    status.source_relation,
                    GuestImportSourceRelation::CapturedPrefixChanged
                        | GuestImportSourceRelation::Unverifiable
                ))
        {
            return Ok(GuestImportSyncOutcome::Held);
        }

        if status.phase == GuestImportPhase::Captured {
            let import_id = status
                .request
                .snapshot
                .import_id
                .parse()
                .map_err(|_| "게스트 가져오기 요청 ID를 확인할 수 없습니다")?;
            ledger
                .mark_guest_shop_import_attempt_started(target, import_id)
                .map_err(|_| "게스트 상점 가져오기 요청을 저장할 수 없습니다")?;
        }
        status.request
    };

    let result = client
        .import_guest_shop(access_token, &request)
        .await
        .map_err(|_| "게스트 상점 가져오기를 완료하지 못했습니다")?;

    let completion = state
        .ledger
        .lock()
        .map_err(|_| "게스트 가져오기 상태 오류")?
        .complete_guest_shop_import(target, &result)
        .map_err(|_| "게스트 상점 가져오기 응답을 저장할 수 없습니다")?;

    Ok(match completion {
        StorageGuestImportCompletion::Imported => GuestImportSyncOutcome::Imported,
        StorageGuestImportCompletion::ImportedWithCorrectionHold => {
            GuestImportSyncOutcome::ImportedWithCorrectionHold
        }
        StorageGuestImportCompletion::Held => GuestImportSyncOutcome::Held,
    })
}

#[cfg(test)]
mod local_api_e2e_probe {
    use super::sync_pending_guest_shop_import;
    use crate::{
        collectors::discovery::SourceConfig,
        collectors::{ParsedRecord, RecordKind},
        domain::{
            guest_shop_import::{GuestImportPhase, GuestShopImportV2Result},
            usage::{Agent, TokenUsage, UsageCoverage},
        },
        storage::{guest_shop_import_v2::guest_import_v2_capture_tests, ledger::Ledger},
        sync::{
            client::{SupabaseSyncClient, SyncError},
            guest_shop_import::{GuestImportSyncOutcome, GuestShopImportTransport},
        },
        AppState, WindowMode,
    };
    use std::{
        future::Future,
        path::{Path, PathBuf},
        sync::{atomic::AtomicBool, Mutex},
    };

    #[derive(Clone, Copy)]
    struct Task9ScenePlanetFixture {
        nickname: &'static str,
        current_planet_tokens: u64,
        lifetime_tokens: u64,
        growth_credit: f64,
        objects_empty: bool,
    }

    fn task9_unique_public_scene_row<'a, T, F>(
        scene: &'a [T],
        description: &str,
        matches_expected_row: F,
    ) -> &'a T
    where
        F: Fn(&T) -> bool,
    {
        let mut matching_rows = scene.iter().filter(|row| matches_expected_row(row));
        let row = matching_rows
            .next()
            .unwrap_or_else(|| panic!("the public scene must contain {description}"));
        assert!(
            matching_rows.next().is_none(),
            "the public scene must contain exactly one {description}"
        );
        row
    }

    const TASK9_GROWTH_CREDIT_TOLERANCE: f64 = 1e-12;

    fn task9_growth_credit_matches(actual: f64, expected: f64) -> bool {
        actual.is_finite()
            && expected.is_finite()
            && (actual - expected).abs() <= TASK9_GROWTH_CREDIT_TOLERANCE
    }

    #[test]
    fn task9_duplicate_nickname_scene_selection_uses_owner_state() {
        let expected_nickname = "행성 동기화 대기";
        let scene = [
            Task9ScenePlanetFixture {
                nickname: expected_nickname,
                current_planet_tokens: 0,
                lifetime_tokens: 0,
                growth_credit: 0.0,
                objects_empty: true,
            },
            Task9ScenePlanetFixture {
                nickname: expected_nickname,
                current_planet_tokens: 0,
                lifetime_tokens: 42,
                growth_credit: 0.0,
                objects_empty: true,
            },
        ];

        let owner = task9_unique_public_scene_row(&scene, "the imported owner", |planet| {
            planet.nickname == expected_nickname
                && planet.current_planet_tokens == 0
                && planet.lifetime_tokens == 42
                && planet.growth_credit == 0.0
                && planet.objects_empty
        });
        assert_eq!(
            owner.lifetime_tokens, 42,
            "nickname-only selection must not mistake the viewer placeholder for the owner"
        );

        let viewer =
            task9_unique_public_scene_row(&scene, "the fresh viewer placeholder", |planet| {
                planet.nickname == expected_nickname
                    && planet.current_planet_tokens == 0
                    && planet.lifetime_tokens == 0
                    && planet.growth_credit == 0.0
                    && planet.objects_empty
            });
        assert_eq!(viewer.lifetime_tokens, 0);
    }

    #[test]
    fn task9_current_owner_with_growth_is_not_filtered_as_zero_growth() {
        let expected_nickname = "행성 동기화 대기";
        let expected_growth = (1.0_f64 + 17.0 / 100_000.0).log2();
        let scene = [
            Task9ScenePlanetFixture {
                nickname: expected_nickname,
                current_planet_tokens: 0,
                lifetime_tokens: 0,
                growth_credit: 0.0,
                objects_empty: true,
            },
            Task9ScenePlanetFixture {
                nickname: expected_nickname,
                current_planet_tokens: 17,
                lifetime_tokens: 59,
                growth_credit: expected_growth,
                objects_empty: true,
            },
        ];

        let owner = task9_unique_public_scene_row(
            &scene,
            "the current-cycle owner with positive growth",
            |planet| {
                planet.nickname == expected_nickname
                    && planet.current_planet_tokens == 17
                    && planet.lifetime_tokens == 59
                    && task9_growth_credit_matches(planet.growth_credit, expected_growth)
                    && planet.objects_empty
            },
        );
        assert_eq!(owner.current_planet_tokens, 17);
        assert!(expected_growth > 0.0);
        assert!(owner.growth_credit > 0.0);

        let viewer =
            task9_unique_public_scene_row(&scene, "the fresh viewer placeholder", |planet| {
                planet.nickname == expected_nickname
                    && planet.current_planet_tokens == 0
                    && planet.lifetime_tokens == 0
                    && planet.growth_credit == 0.0
                    && planet.objects_empty
            });
        assert_eq!(viewer.growth_credit, 0.0);
    }

    #[test]
    fn task9_client_diagnostic_contains_only_safe_result_codes() {
        assert_eq!(
            task9_client_result_diagnostic(&Ok::<(), SyncError>(())),
            Task9GuestImportDiagnostic::ClientValidatedResult
        );
        assert_eq!(
            task9_client_result_diagnostic::<()>(&Err(SyncError::Transport)),
            Task9GuestImportDiagnostic::Transport
        );
        assert_eq!(
            task9_client_result_diagnostic::<()>(&Err(SyncError::Rejected(403))),
            Task9GuestImportDiagnostic::HttpRejected(403)
        );
        assert_eq!(
            task9_client_result_diagnostic::<()>(&Err(SyncError::InvalidResponse)),
            Task9GuestImportDiagnostic::InvalidResponse
        );
        assert_eq!(
            task9_client_result_diagnostic::<()>(&Err(SyncError::InvalidSnapshot)),
            Task9GuestImportDiagnostic::InvalidSnapshot
        );
    }

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum Task9GuestImportDiagnostic {
        ClientValidatedResult,
        Transport,
        HttpRejected(u16),
        InvalidResponse,
        InvalidSnapshot,
    }

    fn task9_client_result_diagnostic<T>(
        result: &Result<T, SyncError>,
    ) -> Task9GuestImportDiagnostic {
        match result {
            Ok(_) => Task9GuestImportDiagnostic::ClientValidatedResult,
            Err(SyncError::Transport) => Task9GuestImportDiagnostic::Transport,
            Err(SyncError::Rejected(status)) => Task9GuestImportDiagnostic::HttpRejected(*status),
            Err(SyncError::InvalidResponse) => Task9GuestImportDiagnostic::InvalidResponse,
            Err(SyncError::InvalidSnapshot) => Task9GuestImportDiagnostic::InvalidSnapshot,
        }
    }

    struct DiagnosticTransport<'a>(
        &'a SupabaseSyncClient,
        &'a Mutex<Option<Task9GuestImportDiagnostic>>,
    );

    impl GuestShopImportTransport for DiagnosticTransport<'_> {
        fn import_guest_shop<'a>(
            &'a self,
            access_token: &'a str,
            request: &'a crate::domain::guest_shop_import::GuestShopImportV2Request,
        ) -> impl Future<Output = Result<GuestShopImportV2Result, SyncError>> + Send + 'a {
            async move {
                let result = self.0.import_guest_shop(access_token, request).await;
                if let Err(error) = &result {
                    let diagnostic = task9_client_result_diagnostic(&result);
                    if let Ok(mut last_error) = self.1.lock() {
                        *last_error = Some(diagnostic);
                    }
                    eprintln!("Task 9 guest import client error: {error:?}");
                }
                result
            }
        }
    }

    fn append_tokens(
        ledger: &mut Ledger,
        event_key: &str,
        occurred_at_utc: chrono::DateTime<chrono::Utc>,
        tokens: u64,
    ) {
        ledger
            .insert(&ParsedRecord {
                agent: Agent::Codex,
                kind: RecordKind::Response,
                event_key: event_key.to_owned(),
                occurred_at_utc,
                usage: TokenUsage {
                    input_tokens: None,
                    output_tokens: None,
                    cache_read_tokens: None,
                    cache_write_tokens: None,
                    total_tokens: Some(tokens),
                    coverage: UsageCoverage::Complete,
                },
            })
            .expect("the E2E test's synthetic usage event must be accepted");
    }

    fn task9_api_url_is_owned_loopback_origin(value: &str) -> bool {
        crate::sync::client::task9_local_api_origin_port(value).is_some()
    }

    fn required_task9_env(name: &str) -> String {
        let value = std::env::var(name)
            .unwrap_or_else(|_| panic!("required Task 9 E2E setting {name} is missing"));
        if name == "TASK9_API_URL" {
            assert!(
                task9_api_url_is_owned_loopback_origin(&value),
                "TASK9_API_URL must be a canonical HTTP origin on the owned 127.0.0.1 port"
            );
        }
        if name == "TASK9_WORLD_ID_FILE" {
            let (codex_root, _) = validated_task9_source_roots();
            let source_root = codex_root
                .parent()
                .expect("the Codex collector root must have a source directory");
            validated_task9_world_id_file(source_root, &value);
        }
        value
    }

    fn task9_proxy_environment_is_safe(
        entries: &[(std::ffi::OsString, std::ffi::OsString)],
    ) -> bool {
        fn exactly_one_value(
            entries: &[(std::ffi::OsString, std::ffi::OsString)],
            name: &str,
            expected: &str,
        ) -> bool {
            let mut matching = entries.iter().filter(|(key, _)| key == name);
            matching.next().is_some_and(|(_, value)| value == expected) && matching.next().is_none()
        }

        fn absent_or_empty(
            entries: &[(std::ffi::OsString, std::ffi::OsString)],
            name: &str,
        ) -> bool {
            let mut matching = entries.iter().filter(|(key, _)| key == name);
            matching.next().is_none_or(|(_, value)| value.is_empty()) && matching.next().is_none()
        }

        exactly_one_value(entries, "NO_PROXY", "*")
            && exactly_one_value(entries, "no_proxy", "*")
            && [
                "http_proxy",
                "HTTP_PROXY",
                "https_proxy",
                "HTTPS_PROXY",
                "all_proxy",
                "ALL_PROXY",
            ]
            .iter()
            .all(|name| absent_or_empty(entries, name))
    }

    fn assert_task9_proxy_environment() {
        let entries = std::env::vars_os().collect::<Vec<_>>();
        assert!(
            task9_proxy_environment_is_safe(&entries),
            "Task 9 E2E requires wildcard no-proxy settings and no configured proxies"
        );
    }

    #[cfg(unix)]
    fn task9_current_uid() -> u32 {
        let output = std::process::Command::new("/usr/bin/id")
            .arg("-u")
            .output()
            .expect("the current user id must be available for Task 9 validation");
        assert!(output.status.success(), "id -u must succeed");
        String::from_utf8(output.stdout)
            .expect("id -u must return UTF-8")
            .trim()
            .parse()
            .expect("id -u must return a numeric uid")
    }

    #[cfg(unix)]
    fn validated_task9_proxy_port(source_root: &Path) -> u16 {
        use std::os::unix::fs::MetadataExt;

        let proxy_port_path = source_root
            .parent()
            .expect("the Task 9 source root must have a run directory")
            .join("proxy.port");
        let metadata = std::fs::symlink_metadata(&proxy_port_path).unwrap_or_else(|error| {
            panic!(
                "the owned Task 9 proxy port file {} must exist: {error}",
                proxy_port_path.display()
            )
        });
        assert!(
            metadata.file_type().is_file(),
            "the owned Task 9 proxy port must be a regular non-symlink file"
        );
        assert_eq!(
            metadata.uid(),
            task9_current_uid(),
            "the owned Task 9 proxy port file must belong to the current user"
        );
        assert_eq!(
            metadata.mode() & 0o777,
            0o600,
            "the owned Task 9 proxy port file must have mode 0600"
        );
        assert!(
            (5..=6).contains(&metadata.len()),
            "the proxy port file must contain one port"
        );

        let contents = std::fs::read(&proxy_port_path)
            .expect("the owned Task 9 proxy port file must be readable");
        let digits = contents.strip_suffix(b"\n").unwrap_or(&contents);
        assert!(
            digits.len() == 5 && digits.iter().all(u8::is_ascii_digit),
            "the owned Task 9 proxy port file must contain one canonical decimal port"
        );
        let text = std::str::from_utf8(digits).expect("the proxy port must be ASCII decimal");
        let port = text
            .parse::<u16>()
            .expect("the owned Task 9 proxy port must fit in u16");
        assert!(
            (49152..=65535).contains(&port) && port.to_string() == text,
            "the owned Task 9 proxy port must be canonical and within the approved range"
        );
        port
    }

    #[cfg(unix)]
    fn validated_task9_world_id_file(source_root: &Path, value: &str) -> PathBuf {
        use std::os::unix::fs::MetadataExt;

        let expected = source_root
            .parent()
            .expect("the Task 9 source root must have a run directory")
            .join("world-id.txt");
        assert_eq!(
            value,
            expected
                .to_str()
                .expect("the Task 9 run path must be UTF-8"),
            "TASK9_WORLD_ID_FILE must be the exact owned run's world-id.txt path"
        );
        let metadata = std::fs::symlink_metadata(&expected).unwrap_or_else(|error| {
            panic!(
                "the owned Task 9 world-id file {} must exist: {error}",
                expected.display()
            )
        });
        assert!(
            metadata.file_type().is_file(),
            "the owned Task 9 world-id file must be a regular non-symlink file"
        );
        assert_eq!(
            metadata.uid(),
            task9_current_uid(),
            "the owned Task 9 world-id file must belong to the current user"
        );
        assert_eq!(
            metadata.mode() & 0o777,
            0o600,
            "the owned Task 9 world-id file must have mode 0600"
        );
        assert_eq!(
            metadata.len(),
            0,
            "the owned Task 9 world-id file must be empty before world creation"
        );
        expected
    }

    #[cfg(not(unix))]
    fn validated_task9_world_id_file(_source_root: &Path, _value: &str) -> PathBuf {
        panic!("the Task 9 local API probe requires a Unix-owned world-id file")
    }

    #[cfg(not(unix))]
    fn validated_task9_proxy_port(_source_root: &Path) -> u16 {
        panic!("the Task 9 local API probe requires a Unix-owned proxy port file")
    }

    #[cfg(unix)]
    fn validated_task9_source_roots() -> (PathBuf, PathBuf) {
        use std::{fs, os::unix::fs::MetadataExt};

        fn assert_no_symlink_components(path: &Path) {
            let mut prefix = PathBuf::new();
            for component in path.components() {
                assert!(
                    !matches!(
                        component,
                        std::path::Component::CurDir | std::path::Component::ParentDir
                    ),
                    "Task 9 source path must not contain dot components"
                );
                prefix.push(component.as_os_str());
                let metadata = fs::symlink_metadata(&prefix).unwrap_or_else(|error| {
                    panic!(
                        "Task 9 source path component {} must exist: {error}",
                        prefix.display()
                    )
                });
                assert!(
                    !metadata.file_type().is_symlink(),
                    "Task 9 source path component {} must not be a symlink",
                    prefix.display()
                );
            }
        }

        fn assert_private_directory(path: &Path, uid: u32) {
            let metadata = fs::symlink_metadata(path).unwrap_or_else(|error| {
                panic!(
                    "Task 9 source directory {} must exist: {error}",
                    path.display()
                )
            });
            assert!(metadata.is_dir(), "{} must be a directory", path.display());
            assert_eq!(
                metadata.uid(),
                uid,
                "Task 9 source directory {} must belong to the current user",
                path.display()
            );
            assert_eq!(
                metadata.mode() & 0o777,
                0o700,
                "Task 9 source directory {} must have mode 0700",
                path.display()
            );
        }

        fn assert_empty_directory(path: &Path) {
            let mut entries = fs::read_dir(path).unwrap_or_else(|error| {
                panic!(
                    "Task 9 collector root {} must be readable: {error}",
                    path.display()
                )
            });
            assert!(
                entries.next().is_none(),
                "Task 9 collector root {} must be empty",
                path.display()
            );
        }

        let source_root = PathBuf::from(required_task9_env("TASK9_SOURCE_ROOT"));
        assert!(
            source_root.is_absolute(),
            "TASK9_SOURCE_ROOT must be absolute"
        );
        let run_root = source_root
            .parent()
            .expect("TASK9_SOURCE_ROOT must have a run-specific parent");
        let tmp_root = Path::new("/private/tmp");
        assert_eq!(
            source_root.file_name().and_then(|name| name.to_str()),
            Some("sources"),
            "TASK9_SOURCE_ROOT must end in /sources"
        );
        assert_eq!(
            run_root.parent(),
            Some(tmp_root),
            "TASK9_SOURCE_ROOT must be directly under its /private/tmp run directory"
        );
        let run_name = run_root
            .file_name()
            .and_then(|name| name.to_str())
            .expect("the Task 9 run directory must have a UTF-8 name");
        let run_id = run_name
            .strip_prefix("shop-guest-import-v2-e2e.")
            .expect("the Task 9 run directory must use the approved name");
        assert!(
            run_id.len() == 24
                && run_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
            "the Task 9 run directory suffix must be 24 lowercase hexadecimal characters"
        );
        let expected_source_root = tmp_root.join(run_name).join("sources");
        assert_eq!(
            source_root.as_os_str(),
            expected_source_root.as_os_str(),
            "TASK9_SOURCE_ROOT must use its canonical run-specific path"
        );

        assert_no_symlink_components(&source_root);
        let canonical_source_root = fs::canonicalize(&source_root)
            .expect("TASK9_SOURCE_ROOT must resolve to an existing directory");
        assert_eq!(
            canonical_source_root.as_os_str(),
            source_root.as_os_str(),
            "TASK9_SOURCE_ROOT must already be canonical"
        );

        let codex_root = source_root.join("emptycodex");
        let claude_root = source_root.join("emptyclaude");
        let uid = task9_current_uid();
        for path in [
            run_root,
            source_root.as_path(),
            codex_root.as_path(),
            claude_root.as_path(),
        ] {
            assert_private_directory(path, uid);
        }
        let mut source_entries = fs::read_dir(&source_root)
            .expect("the Task 9 source directory must be readable")
            .map(|entry| {
                entry
                    .expect("the Task 9 source directory entries must be readable")
                    .file_name()
                    .to_string_lossy()
                    .into_owned()
            })
            .collect::<Vec<_>>();
        source_entries.sort();
        assert_eq!(
            source_entries,
            ["emptyclaude", "emptycodex"],
            "the Task 9 source directory must contain only its two isolated collector roots"
        );
        assert_empty_directory(&codex_root);
        assert_empty_directory(&claude_root);

        (codex_root, claude_root)
    }

    #[cfg(not(unix))]
    fn validated_task9_source_roots() -> (PathBuf, PathBuf) {
        panic!("the Task 9 local API probe requires Unix-owned isolated source directories")
    }

    fn probe_state(ledger: Ledger) -> AppState {
        let (codex_root, claude_root) = validated_task9_source_roots();
        probe_state_with_roots(ledger, codex_root, claude_root)
    }

    fn probe_state_with_roots(
        ledger: Ledger,
        codex_root: PathBuf,
        claude_root: PathBuf,
    ) -> AppState {
        let state = AppState {
            config: Mutex::new(SourceConfig {
                codex_root,
                claude_root,
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
        };
        state
            .scan()
            .expect("the test-only empty collector roots must initialize the native snapshot");
        state
    }

    #[cfg(unix)]
    mod source_root_isolation_tests {
        use super::*;
        use std::{
            ffi::OsString,
            fs,
            os::unix::fs::{symlink, PermissionsExt},
            panic::{catch_unwind, AssertUnwindSafe},
            path::PathBuf,
        };

        static TASK9_SOURCE_ENV_LOCK: Mutex<()> = Mutex::new(());

        struct SourceFixture {
            run_root: PathBuf,
        }

        impl SourceFixture {
            fn new(run_name: &str) -> Self {
                let run_root = Path::new("/private/tmp").join(run_name);
                fs::create_dir(&run_root).expect("the isolated Task 9 run directory must be new");
                set_private_directory_mode(&run_root);

                let sources = run_root.join("sources");
                fs::create_dir(&sources).expect("the isolated source directory must be new");
                set_private_directory_mode(&sources);
                for name in ["emptycodex", "emptyclaude"] {
                    let root = sources.join(name);
                    fs::create_dir(&root).expect("each isolated collector root must be new");
                    set_private_directory_mode(&root);
                }
                let proxy_port = run_root.join("proxy.port");
                fs::write(&proxy_port, b"50259\n")
                    .expect("the isolated proxy port record must be created");
                fs::set_permissions(&proxy_port, fs::Permissions::from_mode(0o600))
                    .expect("the isolated proxy port record must have mode 0600");
                let world_id = run_root.join("world-id.txt");
                fs::write(&world_id, b"")
                    .expect("the isolated world-id cleanup record must be empty");
                fs::set_permissions(&world_id, fs::Permissions::from_mode(0o600))
                    .expect("the isolated world-id cleanup record must have mode 0600");

                Self { run_root }
            }

            fn source_root(&self) -> PathBuf {
                self.run_root.join("sources")
            }

            fn codex_root(&self) -> PathBuf {
                self.source_root().join("emptycodex")
            }

            fn unique() -> Self {
                let id = uuid::Uuid::new_v4().simple().to_string();
                Self::new(&format!("shop-guest-import-v2-e2e.{}", &id[..24]))
            }

            fn foreign() -> Self {
                let id = uuid::Uuid::new_v4().simple().to_string();
                Self::new(&format!("task9-foreign-source.{}", &id[..24]))
            }
        }

        impl Drop for SourceFixture {
            fn drop(&mut self) {
                let _ = fs::remove_dir_all(&self.run_root);
            }
        }

        fn set_private_directory_mode(path: &Path) {
            fs::set_permissions(path, fs::Permissions::from_mode(0o700))
                .expect("isolated source directories must have mode 0700");
        }

        struct SourceRootEnv(Option<OsString>);

        impl SourceRootEnv {
            fn set(path: &Path) -> Self {
                let previous = std::env::var_os("TASK9_SOURCE_ROOT");
                std::env::set_var("TASK9_SOURCE_ROOT", path);
                Self(previous)
            }
        }

        impl Drop for SourceRootEnv {
            fn drop(&mut self) {
                if let Some(previous) = self.0.take() {
                    std::env::set_var("TASK9_SOURCE_ROOT", previous);
                } else {
                    std::env::remove_var("TASK9_SOURCE_ROOT");
                }
            }
        }

        fn probe_state_for(source_root: &Path) -> AppState {
            let _source_root = SourceRootEnv::set(source_root);
            let mut ledger = guest_import_v2_capture_tests::prepared_first_reset_ledger();
            ledger
                .set_agent_enabled(Agent::Codex, false)
                .expect("Codex must stay disabled while validating isolated source paths");
            ledger
                .set_agent_enabled(Agent::ClaudeCode, false)
                .expect("Claude must stay disabled while validating isolated source paths");
            probe_state(ledger)
        }

        fn assert_probe_rejects(source_root: &Path) {
            let result = catch_unwind(AssertUnwindSafe(|| probe_state_for(source_root)));
            assert!(
                result.is_err(),
                "probe_state must reject unsafe Task 9 source root {}",
                source_root.display()
            );
        }

        fn assert_proxy_port_rejected(source_root: &Path) {
            let result = catch_unwind(AssertUnwindSafe(|| validated_task9_proxy_port(source_root)));
            assert!(
                result.is_err(),
                "unsafe Task 9 proxy port record was accepted under {}",
                source_root.display()
            );
        }

        struct Task9WorldIdEnv {
            previous_source_root: Option<OsString>,
            previous_world_id_file: Option<OsString>,
        }

        impl Task9WorldIdEnv {
            fn set(source_root: &Path, world_id_file: &Path) -> Self {
                let previous_source_root = std::env::var_os("TASK9_SOURCE_ROOT");
                let previous_world_id_file = std::env::var_os("TASK9_WORLD_ID_FILE");
                std::env::set_var("TASK9_SOURCE_ROOT", source_root);
                std::env::set_var("TASK9_WORLD_ID_FILE", world_id_file);
                Self {
                    previous_source_root,
                    previous_world_id_file,
                }
            }
        }

        impl Drop for Task9WorldIdEnv {
            fn drop(&mut self) {
                if let Some(previous) = self.previous_source_root.take() {
                    std::env::set_var("TASK9_SOURCE_ROOT", previous);
                } else {
                    std::env::remove_var("TASK9_SOURCE_ROOT");
                }
                if let Some(previous) = self.previous_world_id_file.take() {
                    std::env::set_var("TASK9_WORLD_ID_FILE", previous);
                } else {
                    std::env::remove_var("TASK9_WORLD_ID_FILE");
                }
            }
        }

        fn assert_world_id_file_rejected(source_root: &Path, world_id_file: &Path) {
            let _env = Task9WorldIdEnv::set(source_root, world_id_file);
            let result = catch_unwind(AssertUnwindSafe(|| {
                required_task9_env("TASK9_WORLD_ID_FILE")
            }));
            assert!(
                result.is_err(),
                "unsafe Task 9 world-id cleanup file was accepted: {}",
                world_id_file.display()
            );
        }

        #[test]
        fn task9_probe_state_uses_owned_empty_run_specific_roots() {
            let _env_lock = TASK9_SOURCE_ENV_LOCK.lock().unwrap();
            let fixture = SourceFixture::unique();
            let source_root = fixture.source_root();
            let state = probe_state_for(&source_root);
            let config = state.config.lock().unwrap().clone();

            assert_eq!(config.codex_root, fixture.codex_root());
            assert_eq!(config.claude_root, source_root.join("emptyclaude"));
        }

        #[test]
        fn task9_probe_state_rejects_foreign_noncanonical_symlink_and_nonempty_roots() {
            let _env_lock = TASK9_SOURCE_ENV_LOCK.lock().unwrap();
            let foreign = SourceFixture::foreign();
            assert_probe_rejects(&foreign.source_root());

            let noncanonical = SourceFixture::unique();
            let noncanonical_root = noncanonical.source_root().join("..").join("sources");
            assert_probe_rejects(&noncanonical_root);

            let link_target = SourceFixture::unique();
            let linked = SourceFixture::unique();
            fs::remove_dir_all(linked.source_root())
                .expect("the real source directory must be removed before adding its symlink");
            symlink(link_target.source_root(), linked.source_root())
                .expect("the test source-root symlink must be created");
            assert_probe_rejects(&linked.source_root());

            let nonempty = SourceFixture::unique();
            fs::write(nonempty.codex_root().join("foreign-usage.jsonl"), b"{}").unwrap();
            assert_probe_rejects(&nonempty.source_root());
        }

        #[test]
        fn task9_owned_proxy_port_requires_current_user_private_single_port_file() {
            let fixture = SourceFixture::unique();
            let source_root = fixture.source_root();
            let port_file = fixture.run_root.join("proxy.port");
            assert_eq!(validated_task9_proxy_port(&source_root), 50259);

            fs::set_permissions(&port_file, fs::Permissions::from_mode(0o644)).unwrap();
            assert_proxy_port_rejected(&source_root);
            fs::set_permissions(&port_file, fs::Permissions::from_mode(0o600)).unwrap();

            for contents in [
                b"49151\n".as_slice(),
                b"65536\n",
                b"050259\n",
                b"50259\n50260\n",
            ] {
                fs::write(&port_file, contents).unwrap();
                assert_proxy_port_rejected(&source_root);
            }

            fs::write(&port_file, b"50259\n").unwrap();
            let target = fixture.run_root.join("proxy-port-target");
            fs::write(&target, b"50259\n").unwrap();
            fs::remove_file(&port_file).unwrap();
            symlink(&target, &port_file).expect("the proxy port symlink must be created");
            assert_proxy_port_rejected(&source_root);
        }

        #[test]
        fn task9_world_id_cleanup_requires_exact_owned_empty_private_file() {
            let _env_lock = TASK9_SOURCE_ENV_LOCK.lock().unwrap();
            let fixture = SourceFixture::unique();
            let source_root = fixture.source_root();
            let expected = fixture.run_root.join("world-id.txt");
            let _env = Task9WorldIdEnv::set(&source_root, &expected);
            assert_eq!(
                required_task9_env("TASK9_WORLD_ID_FILE"),
                expected.to_str().unwrap()
            );
            drop(_env);

            let wrong_path = fixture.run_root.join("world-id-other.txt");
            fs::write(&wrong_path, b"").unwrap();
            fs::set_permissions(&wrong_path, fs::Permissions::from_mode(0o600)).unwrap();
            assert_world_id_file_rejected(&source_root, &wrong_path);

            fs::set_permissions(&expected, fs::Permissions::from_mode(0o644)).unwrap();
            assert_world_id_file_rejected(&source_root, &expected);
            fs::set_permissions(&expected, fs::Permissions::from_mode(0o600)).unwrap();

            fs::write(&expected, b"foreign-world-id").unwrap();
            assert_world_id_file_rejected(&source_root, &expected);
            fs::write(&expected, b"").unwrap();

            let symlink_target = fixture.run_root.join("world-id-target.txt");
            fs::write(&symlink_target, b"").unwrap();
            fs::set_permissions(&symlink_target, fs::Permissions::from_mode(0o600)).unwrap();
            fs::remove_file(&expected).unwrap();
            symlink(&symlink_target, &expected).unwrap();
            assert_world_id_file_rejected(&source_root, &expected);
        }
    }

    #[cfg(unix)]
    mod task9_api_url_tests {
        use super::*;
        use std::{
            ffi::OsString,
            panic::{catch_unwind, AssertUnwindSafe},
        };

        static TASK9_API_URL_ENV_LOCK: Mutex<()> = Mutex::new(());

        struct Task9ApiUrlEnv(Option<OsString>);

        impl Task9ApiUrlEnv {
            fn set(value: &str) -> Self {
                let previous = std::env::var_os("TASK9_API_URL");
                std::env::set_var("TASK9_API_URL", value);
                Self(previous)
            }
        }

        impl Drop for Task9ApiUrlEnv {
            fn drop(&mut self) {
                if let Some(previous) = self.0.take() {
                    std::env::set_var("TASK9_API_URL", previous);
                } else {
                    std::env::remove_var("TASK9_API_URL");
                }
            }
        }

        fn assert_rejected(value: &str) {
            let _url = Task9ApiUrlEnv::set(value);
            let result = catch_unwind(AssertUnwindSafe(|| required_task9_env("TASK9_API_URL")));
            assert!(
                result.is_err(),
                "unsafe Task 9 API URL was accepted: {value}"
            );
        }

        #[test]
        fn task9_api_url_allows_only_canonical_owned_loopback_origin() {
            let _env_lock = TASK9_API_URL_ENV_LOCK.lock().unwrap();

            for value in [
                "http://127.0.0.1:49152",
                "http://127.0.0.1:50259",
                "http://127.0.0.1:65535",
            ] {
                let _url = Task9ApiUrlEnv::set(value);
                assert_eq!(required_task9_env("TASK9_API_URL"), value);
            }

            for value in [
                "https://api.example.com:50259",
                "http://api.example.com:50259",
                "https://127.0.0.1:50259",
                "http://localhost:50259",
                "http://user:password@127.0.0.1:50259",
                "http://127.0.0.1:50259/path",
                "http://127.0.0.1:50259/",
                "http://127.0.0.1:50259?query=1",
                "http://127.0.0.1:50259#fragment",
                "http://127.0.0.2:50259",
                "http://192.168.1.10:50259",
                "http://127.0.0.1:49151",
                "http://127.0.0.1:65536",
                "http://127.0.0.1:0",
                "http://127.0.0.1",
                "http://127.0.0.1:050259",
            ] {
                assert_rejected(value);
            }
        }
    }

    mod task9_proxy_environment_tests {
        use super::*;
        use std::ffi::OsString;

        fn environment(entries: &[(&str, &str)]) -> Vec<(OsString, OsString)> {
            entries
                .iter()
                .map(|(name, value)| (OsString::from(name), OsString::from(value)))
                .collect()
        }

        #[test]
        fn task9_proxy_environment_requires_loopback_bypass_and_no_configured_proxies() {
            assert!(task9_proxy_environment_is_safe(&environment(&[
                ("NO_PROXY", "*"),
                ("no_proxy", "*"),
                ("http_proxy", ""),
                ("HTTPS_PROXY", ""),
            ])));

            for name in [
                "http_proxy",
                "HTTP_PROXY",
                "https_proxy",
                "HTTPS_PROXY",
                "all_proxy",
                "ALL_PROXY",
            ] {
                assert!(!task9_proxy_environment_is_safe(&environment(&[
                    ("NO_PROXY", "*"),
                    ("no_proxy", "*"),
                    (name, "http://proxy.example:8080"),
                ])));
            }

            for entries in [
                environment(&[("no_proxy", "*")]),
                environment(&[("NO_PROXY", "*")]),
                environment(&[("NO_PROXY", "127.0.0.1"), ("no_proxy", "*")]),
                environment(&[("NO_PROXY", "*"), ("no_proxy", "127.0.0.1")]),
                environment(&[("No_Proxy", "*"), ("no_proxy", "*")]),
            ] {
                assert!(!task9_proxy_environment_is_safe(&entries));
            }
        }
    }

    fn upload_pending_growth_journal(
        state: &AppState,
        client: &SupabaseSyncClient,
        access_token: &str,
    ) {
        let remote = tauri::async_runtime::block_on(client.growth_journal(access_token))
            .expect("the real local API must return the account growth journal");
        let (journal, pending) = {
            let mut ledger = state.ledger.lock().unwrap();
            ledger
                .apply_growth_journal_state(&remote)
                .expect("the native cache must merge the remote journal");
            ledger
                .prepare_growth_journal()
                .expect("the native cache must prepare newly appended journal entries");
            (
                ledger.growth_journal().unwrap(),
                ledger.pending_growth_journal_entries().unwrap(),
            )
        };
        assert!(
            !pending.is_empty(),
            "the appended revision must be pending for upload"
        );
        let timezone = journal
            .timezone
            .as_deref()
            .expect("the captured native journal has a timezone");
        let canonical = tauri::async_runtime::block_on(client.upsert_growth_journal(
            access_token,
            journal.generation,
            timezone,
            &journal.cycles,
            &pending,
        ))
        .expect("the real local API must accept the pending journal revision");
        let mut ledger = state.ledger.lock().unwrap();
        ledger
            .apply_growth_journal_state(&canonical)
            .expect("the native cache must apply the actual server ACK");
        ledger
            .prepare_growth_journal()
            .expect("the native cache must retain only genuinely newer revisions as pending");
    }

    fn find_forbidden_public_key(value: &serde_json::Value) -> Option<String> {
        const FORBIDDEN_KEYS: &[&str] = &[
            "wallet_balance",
            "wallet_credits",
            "available_balance",
            "removal_debits",
            "removal_proofs",
            "purchase_proofs",
            "purchase_id",
            "purchases",
            "reward_timezone",
            "era_progress",
            "game_rewards",
            "last_reset_at_utc",
            "reset_available_at_utc",
            "reset_receipt",
            "reset_settlement_proofs",
            "import_id",
            "request_id",
            "expected_cycle_id",
            "source_fingerprint",
            "source_account_id",
            "source_path",
            "prefix_fingerprint",
            "lineage_id",
            "occurrence_id",
            "occurrences",
            "planet_device_id",
            "device_id",
            "canonical_version",
            "canonical_payload",
            "canonical_contribution",
            "ack",
            "journal_confirmation",
            "effect_revision",
            "cycle_id",
            "current_cycle_id",
            "old_cycle_id",
            "previous_cycle_id",
            "new_cycle_id",
            "next_cycle_id",
            "effect_history",
            "effect_timeline",
            "effect_contributions",
            "provenance",
            "agent",
            "reward_date",
            "raw_tokens",
            "prompt",
            "log",
        ];

        match value {
            serde_json::Value::Object(fields) => fields.iter().find_map(|(key, child)| {
                if FORBIDDEN_KEYS.contains(&key.as_str()) {
                    Some(key.clone())
                } else {
                    find_forbidden_public_key(child)
                }
            }),
            serde_json::Value::Array(items) => items.iter().find_map(find_forbidden_public_key),
            _ => None,
        }
    }

    #[test]
    #[ignore = "run only from shop_guest_import_v2_e2e.sh against its owned local API"]
    fn task9_local_api_import_worker_cache_and_explicit_world_visibility() {
        let (codex_root, claude_root) = validated_task9_source_roots();
        let source_root = codex_root
            .parent()
            .expect("the Codex collector root must have a source directory");
        let owned_proxy_port = validated_task9_proxy_port(source_root);
        let api_url = required_task9_env("TASK9_API_URL");
        assert_eq!(
            crate::sync::client::task9_local_api_origin_port(&api_url),
            Some(owned_proxy_port),
            "TASK9_API_URL must use the native runner's owned proxy port"
        );
        assert_task9_proxy_environment();
        let world_id_path = required_task9_env("TASK9_WORLD_ID_FILE");
        let anon_key = required_task9_env("TASK9_ANON_KEY");
        let user_id = required_task9_env("TASK9_USER_ID");
        let user_token = required_task9_env("TASK9_USER_ACCESS_TOKEN");
        let viewer_id = required_task9_env("TASK9_VIEWER_USER_ID");
        let viewer_token = required_task9_env("TASK9_VIEWER_ACCESS_TOKEN");
        assert_ne!(
            user_id, viewer_id,
            "owner and viewer fixtures must be distinct"
        );

        let target = format!("account:{user_id}");
        let mut ledger = guest_import_v2_capture_tests::prepared_first_reset_ledger();
        let request = ledger
            .capture_guest_shop_import_request(&target)
            .expect("the real first-reset fixture must capture a native request");
        let crate::storage::PendingGuestShopImport::V2(captured) = request else {
            panic!("the E2E fixture must produce a schema-2 request");
        };
        let request = captured.request;
        let reset_at = chrono::DateTime::parse_from_rfc3339(
            &request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .reset_at_utc,
        )
        .expect("the captured reset receipt has a valid UTC instant")
        .with_timezone(&chrono::Utc);
        const CURRENT_EVENT_KEY: &str = "task9-current-after-capture";
        const CURRENT_TOKENS: u64 = 17;
        const LATE_EVENT_KEY: &str = "task9-late-old-after-capture";
        const LATE_TOKENS: u64 = 7;
        let current_at = chrono::Utc::now();
        let current_bucket_date = current_at.format("%Y-%m-%d").to_string();
        append_tokens(&mut ledger, CURRENT_EVENT_KEY, current_at, CURRENT_TOKENS);
        ledger
            .set_selected_auth_account(&target)
            .expect("the synthetic owner must be the selected native account");
        let pending_before_import = ledger
            .pending_guest_shop_import_request(&target)
            .unwrap()
            .expect("the original captured request must remain pending after the raw append");
        let crate::storage::PendingGuestShopImport::V2(pending_before_import) =
            pending_before_import
        else {
            panic!("the raw append must not replace the schema-2 request");
        };
        assert_eq!(pending_before_import.phase, GuestImportPhase::Captured);
        assert_eq!(pending_before_import.request, request);
        assert_eq!(
            pending_before_import
                .request
                .snapshot
                .canonical_payload
                .canonical_version,
            request.snapshot.canonical_payload.canonical_version,
            "the append must not rebuild or advance the captured canonical version"
        );
        assert_eq!(
            pending_before_import
                .request
                .snapshot
                .provenance
                .prefix_fingerprint,
            request.snapshot.provenance.prefix_fingerprint,
            "the append must not change the immutable captured prefix fingerprint"
        );
        assert_eq!(
            pending_before_import
                .request
                .snapshot
                .canonical_payload
                .raw
                .current_planet_tokens,
            0,
            "the captured request must still describe the prefix before the append"
        );
        let current_raw_count: i64 = ledger
            .connection
            .query_row(
                "SELECT count(*) FROM usage_record WHERE event_key=?1",
                [CURRENT_EVENT_KEY],
                |row| row.get(0),
            )
            .unwrap();
        assert_eq!(current_raw_count, 1);
        let state = probe_state_with_roots(ledger, codex_root, claude_root);
        let client =
            SupabaseSyncClient::new_task9_local_api_e2e(&api_url, &anon_key, owned_proxy_port);
        let last_client_error = Mutex::new(None);
        let diagnostic_client = DiagnosticTransport(&client, &last_client_error);

        let outcome = tauri::async_runtime::block_on(sync_pending_guest_shop_import(
            &state,
            &diagnostic_client,
            &user_token,
            &target,
        ))
        .unwrap_or_else(|error| {
            let client_diagnostic = last_client_error.lock().ok().and_then(|error| *error);
            panic!("the real native client and worker failed: {error}; client result: {client_diagnostic:?}");
        });
        assert_eq!(outcome, GuestImportSyncOutcome::Imported);

        let local_result: GuestShopImportV2Result = {
            let ledger = state.ledger.lock().unwrap();
            let status = ledger
                .pending_guest_shop_import_request(&target)
                .unwrap()
                .expect("the import receipt remains queryable after completion");
            let crate::storage::PendingGuestShopImport::V2(status) = status else {
                panic!("the local import must retain its schema-2 status");
            };
            assert_eq!(status.phase, GuestImportPhase::Imported);
            assert_eq!(
                status.source_relation,
                crate::domain::guest_shop_import::GuestImportSourceRelation::AppendOnly,
                "the post-capture usage events must remain an append-only delta"
            );
            assert_eq!(status.request, request);

            let appended_raw_count: i64 = ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM usage_record WHERE event_key=?1",
                    [CURRENT_EVENT_KEY],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(
                appended_raw_count, 1,
                "the current-cycle raw append must survive completion"
            );

            let result_json: String = ledger
                .connection
                .query_row(
                    "SELECT result_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                    [&target],
                    |row| row.get(0),
                )
                .unwrap();
            serde_json::from_str(&result_json).unwrap()
        };
        assert_eq!(local_result.import_id, request.snapshot.import_id);
        assert_eq!(local_result.account_id, target);
        assert_eq!(
            local_result.status,
            crate::domain::guest_shop_import::GuestImportStatus::Imported
        );
        assert_eq!(
            local_result.canonical_contribution.as_ref(),
            Some(&request.snapshot.canonical_payload),
            "the RPC's canonical contribution must match the captured native payload"
        );
        let ack = local_result
            .ack
            .as_ref()
            .expect("the RPC must return an ACK");
        assert_eq!(ack.lineage_id, request.snapshot.provenance.lineage_id);
        assert_eq!(ack.device_id, request.snapshot.provenance.device_id);
        assert_eq!(
            ack.ingest_watermark,
            request.snapshot.provenance.ingest_watermark
        );
        assert_eq!(
            ack.occurrence_count,
            request.snapshot.provenance.occurrence_count
        );
        assert_eq!(
            ack.prefix_fingerprint,
            request.snapshot.provenance.prefix_fingerprint
        );

        let world = tauri::async_runtime::block_on(client.create_world(
            &user_token,
            &user_id,
            "Task 9 local E2E",
            "UTC",
        ))
        .expect("the imported owner must share through the existing world API");
        std::fs::write(&world_id_path, &world.id)
            .expect("the owned runner needs the world id for targeted cleanup");
        let member_code = tauri::async_runtime::block_on(client.my_member_code(&user_token))
            .expect("the owner must receive the existing world member code");
        let joined = tauri::async_runtime::block_on(
            client.join_world_by_member_code(&viewer_token, &member_code),
        )
        .expect("the distinct viewer must join through the existing world API");
        assert_eq!(joined.world_id, world.id);

        let hidden = tauri::async_runtime::block_on(client.world_planets(&viewer_token, &world.id))
            .expect("the viewer must be able to read the existing public world projection");
        assert_eq!(
            hidden.len(),
            2,
            "the pre-share scene must contain only the imported owner and fresh viewer"
        );
        assert!(
            hidden.iter().all(|planet| {
                planet.nickname == "행성 동기화 대기"
                    && planet.current_planet_tokens == 0
                    && planet.lifetime_tokens == 0
                    && planet.growth_credit == 0.0
                    && planet.objects.is_empty()
            }),
            "before sharing, the owner must stay hidden and the fresh viewer must remain a zero-state placeholder"
        );

        let expected_nickname = request
            .snapshot
            .data
            .profile
            .as_ref()
            .map(|profile| profile.nickname.as_str())
            .unwrap_or("행성 동기화 대기");
        assert!(
            tauri::async_runtime::block_on(client.resume_my_sync(&user_token, &world.id))
                .expect("explicit sharing must resume through the existing RPC")
        );
        let shared = tauri::async_runtime::block_on(client.world_planets(&viewer_token, &world.id))
            .expect("the viewer must read the explicitly shared scene");
        assert_eq!(
            shared.len(),
            2,
            "sharing must preserve the two-member scene"
        );
        let expected_shared_lifetime = request
            .snapshot
            .provenance
            .reset_receipt
            .result
            .credited_tokens;
        let shared_owner =
            task9_unique_public_scene_row(&shared, "the shared imported owner", |planet| {
                planet.nickname == expected_nickname
                    && planet.current_planet_tokens == 0
                    && planet.lifetime_tokens == expected_shared_lifetime
                    && planet.growth_credit == 0.0
                    && planet.objects.is_empty()
            });
        let shared_viewer = task9_unique_public_scene_row(
            &shared,
            "the fresh viewer's zero-state placeholder after sharing",
            |planet| {
                planet.nickname == "행성 동기화 대기"
                    && planet.current_planet_tokens == 0
                    && planet.lifetime_tokens == 0
                    && planet.growth_credit == 0.0
                    && planet.objects.is_empty()
            },
        );
        assert_eq!(shared_owner.current_planet_tokens, 0);
        assert_eq!(shared_owner.lifetime_tokens, expected_shared_lifetime);
        assert_eq!(shared_owner.growth_credit, 0.0);
        assert!(shared_owner.objects.is_empty());
        assert!(shared_owner.token_rank > 0);
        assert!(shared_owner.civilization_rank > 0);
        assert_eq!(shared_viewer.current_planet_tokens, 0);
        assert_eq!(shared_viewer.lifetime_tokens, 0);
        assert_eq!(shared_viewer.growth_credit, 0.0);
        assert!(shared_viewer.objects.is_empty());

        {
            let mut ledger = state.ledger.lock().unwrap();
            ledger
                .rebuild_shop_contributions()
                .expect("the imported owner may now rebuild the current-cycle append");
            ledger
                .prepare_growth_journal()
                .expect("the imported owner's append must remain pending for journal upload");
        }

        let private_sync =
            tauri::async_runtime::block_on(crate::sync::worker::sync_private_effect_contribution(
                &state,
                &user_id,
                &user_token,
                &client,
                false,
            ))
            .expect("the native worker must upload the post-capture current-cycle contribution");
        assert!(matches!(
            private_sync,
            crate::sync::worker::PrivateEffectSyncOutcome::Uploaded { .. }
        ));
        let owner_after_current =
            tauri::async_runtime::block_on(client.my_planet_state(&user_token))
                .expect(
                    "the owner must read the canonical state after the native contribution upload",
                )
                .expect("the imported account must have canonical planet state");
        assert_eq!(owner_after_current.current_planet_tokens, CURRENT_TOKENS);
        assert_eq!(
            owner_after_current.lifetime_tokens,
            request.snapshot.canonical_payload.raw.lifetime_tokens + CURRENT_TOKENS
        );
        assert_eq!(
            owner_after_current.wallet_balance,
            request
                .snapshot
                .provenance
                .reset_receipt
                .result
                .credited_tokens
        );
        assert_eq!(owner_after_current.wallet_credits.len(), 1);

        let shared_after_current =
            tauri::async_runtime::block_on(client.world_planets(&viewer_token, &world.id))
                .expect("the viewer must read the public scene after the current-cycle upload");
        assert_eq!(
            shared_after_current.len(),
            2,
            "the current-cycle upload must preserve the two-member scene"
        );
        let expected_current_lifetime =
            request.snapshot.canonical_payload.raw.lifetime_tokens + CURRENT_TOKENS;
        let expected_current_growth_credit = (1.0_f64 + CURRENT_TOKENS as f64 / 100_000.0).log2();
        let shared_owner_after_current = task9_unique_public_scene_row(
            &shared_after_current,
            "the shared imported owner after the current-cycle upload",
            |planet| {
                planet.nickname == expected_nickname
                    && planet.current_planet_tokens == CURRENT_TOKENS
                    && planet.lifetime_tokens == expected_current_lifetime
                    && task9_growth_credit_matches(
                        planet.growth_credit,
                        expected_current_growth_credit,
                    )
                    && planet.objects.is_empty()
            },
        );
        let viewer_after_current = task9_unique_public_scene_row(
            &shared_after_current,
            "the fresh viewer after the current-cycle upload",
            |planet| {
                planet.nickname == "행성 동기화 대기"
                    && planet.current_planet_tokens == 0
                    && planet.lifetime_tokens == 0
                    && planet.growth_credit == 0.0
                    && planet.objects.is_empty()
            },
        );
        assert_eq!(
            shared_owner_after_current.current_planet_tokens,
            CURRENT_TOKENS
        );
        assert_eq!(
            shared_owner_after_current.lifetime_tokens,
            expected_current_lifetime
        );
        assert!(task9_growth_credit_matches(
            shared_owner_after_current.growth_credit,
            expected_current_growth_credit
        ));
        assert_eq!(viewer_after_current.current_planet_tokens, 0);
        assert_eq!(viewer_after_current.lifetime_tokens, 0);
        assert_eq!(viewer_after_current.growth_credit, 0.0);
        assert!(viewer_after_current.objects.is_empty());

        upload_pending_growth_journal(&state, &client, &user_token);
        {
            let ledger = state.ledger.lock().unwrap();
            let (revision, acknowledged_revision): (i64, i64) = ledger
                .connection
                .query_row(
                    "SELECT revision,acknowledged_revision FROM growth_journal_entry
                     WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3
                       AND bucket_date=?4 AND agent='codex'",
                    rusqlite::params![
                        target,
                        request.snapshot.provenance.device_id,
                        request
                            .snapshot
                            .provenance
                            .reset_receipt
                            .result
                            .new_cycle_id,
                        current_bucket_date,
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("the current-cycle append must have a journal row");
            assert_eq!(revision, acknowledged_revision);
        }

        let old_cycle_id = request
            .snapshot
            .provenance
            .reset_receipt
            .request
            .cycle_id
            .clone();
        let late_at = reset_at - chrono::Duration::milliseconds(1);
        let late_bucket_date = late_at.format("%Y-%m-%d").to_string();
        {
            let mut ledger = state.ledger.lock().unwrap();
            append_tokens(&mut ledger, LATE_EVENT_KEY, late_at, LATE_TOKENS);
            ledger
                .rebuild_shop_contributions()
                .expect("late old-cycle usage must rebuild the local canonical contribution");
            ledger
                .prepare_growth_journal()
                .expect("late old-cycle usage must create a pending journal revision");
            let contribution = ledger
                .shop_device_contribution(false)
                .expect("the late usage must produce a current contribution snapshot");
            assert_eq!(contribution.raw.current_planet_tokens, CURRENT_TOKENS);
            assert_eq!(
                contribution.raw.lifetime_tokens,
                request.snapshot.canonical_payload.raw.lifetime_tokens
                    + CURRENT_TOKENS
                    + LATE_TOKENS
            );
            let pending = ledger.pending_growth_journal_entries().unwrap();
            assert!(pending.iter().any(|entry| {
                entry.cycle_id == old_cycle_id && entry.bucket_date == late_bucket_date
            }));
            let late_raw_count: i64 = ledger
                .connection
                .query_row(
                    "SELECT count(*) FROM usage_record WHERE event_key=?1",
                    [LATE_EVENT_KEY],
                    |row| row.get(0),
                )
                .unwrap();
            assert_eq!(late_raw_count, 1);
        }
        state
            .scan()
            .expect("the native usage snapshot must include the late old-cycle event");

        let late_private_sync =
            tauri::async_runtime::block_on(crate::sync::worker::sync_private_effect_contribution(
                &state,
                &user_id,
                &user_token,
                &client,
                false,
            ))
            .expect("the real worker must upload late old-cycle lifetime usage");
        assert!(matches!(
            late_private_sync,
            crate::sync::worker::PrivateEffectSyncOutcome::Uploaded { .. }
        ));
        let owner_after_late = tauri::async_runtime::block_on(client.my_planet_state(&user_token))
            .expect("the owner must read canonical state after late usage upload")
            .expect("the imported account must keep canonical planet state");
        assert_eq!(owner_after_late.current_planet_tokens, CURRENT_TOKENS);
        assert_eq!(
            owner_after_late.lifetime_tokens,
            owner_after_current.lifetime_tokens + LATE_TOKENS
        );
        assert_eq!(
            owner_after_late.wallet_balance,
            owner_after_current.wallet_balance
        );
        assert_eq!(
            owner_after_late.wallet_credits,
            owner_after_current.wallet_credits
        );
        assert_eq!(owner_after_late.wallet_credits.len(), 1);

        let shared_after_late =
            tauri::async_runtime::block_on(client.world_planets(&viewer_token, &world.id))
                .expect("the viewer must read public state after late usage upload");
        assert_eq!(
            shared_after_late.len(),
            2,
            "late usage must preserve the two-member scene"
        );
        let expected_late_lifetime = owner_after_current.lifetime_tokens + LATE_TOKENS;
        let expected_late_growth_credit = expected_current_growth_credit;
        let shared_owner_after_late = task9_unique_public_scene_row(
            &shared_after_late,
            "the shared imported owner after late usage upload",
            |planet| {
                planet.nickname == expected_nickname
                    && planet.current_planet_tokens == CURRENT_TOKENS
                    && planet.lifetime_tokens == expected_late_lifetime
                    && task9_growth_credit_matches(
                        planet.growth_credit,
                        expected_late_growth_credit,
                    )
                    && planet.objects.is_empty()
            },
        );
        let viewer_after_late = task9_unique_public_scene_row(
            &shared_after_late,
            "the fresh viewer after late usage upload",
            |planet| {
                planet.nickname == "행성 동기화 대기"
                    && planet.current_planet_tokens == 0
                    && planet.lifetime_tokens == 0
                    && planet.growth_credit == 0.0
                    && planet.objects.is_empty()
            },
        );
        assert_eq!(
            shared_owner_after_late.current_planet_tokens,
            CURRENT_TOKENS
        );
        assert_eq!(
            shared_owner_after_late.lifetime_tokens,
            expected_late_lifetime
        );
        assert!(task9_growth_credit_matches(
            shared_owner_after_late.growth_credit,
            expected_late_growth_credit
        ));
        assert_eq!(viewer_after_late.current_planet_tokens, 0);
        assert_eq!(viewer_after_late.lifetime_tokens, 0);
        assert_eq!(viewer_after_late.growth_credit, 0.0);
        assert!(viewer_after_late.objects.is_empty());

        upload_pending_growth_journal(&state, &client, &user_token);
        {
            let ledger = state.ledger.lock().unwrap();
            let (revision, acknowledged_revision): (i64, i64) = ledger
                .connection
                .query_row(
                    "SELECT revision,acknowledged_revision FROM growth_journal_entry
                     WHERE account_id=?1 AND device_id=?2 AND cycle_id=?3
                       AND bucket_date=?4 AND agent='codex'",
                    rusqlite::params![
                        target,
                        request.snapshot.provenance.device_id,
                        old_cycle_id,
                        late_bucket_date,
                    ],
                    |row| Ok((row.get(0)?, row.get(1)?)),
                )
                .expect("the late old-cycle event must remain journaled");
            assert_eq!(revision, acknowledged_revision);
            let result_json: String = ledger
                .connection
                .query_row(
                    "SELECT result_json FROM guest_shop_import_v2_capture WHERE target_account_id=?1",
                    [&target],
                    |row| row.get(0),
                )
                .unwrap();
            let final_import_result: GuestShopImportV2Result =
                serde_json::from_str(&result_json).unwrap();
            assert_eq!(
                final_import_result.ack, local_result.ack,
                "later journal uploads must not expand or replace the captured-prefix import ACK"
            );
        }

        let raw_public_scene: serde_json::Value = tauri::async_runtime::block_on(async {
            crate::sync::client::task9_local_api_http_client()
                .post(format!(
                    "{}/rest/v1/rpc/get_world_planets",
                    api_url.trim_end_matches('/')
                ))
                .header("apikey", &anon_key)
                .bearer_auth(&viewer_token)
                .json(&serde_json::json!({ "p_world_id": world.id }))
                .send()
                .await
                .expect("the native probe must reach the real public-scene RPC")
                .error_for_status()
                .expect("the public-scene RPC must authorize the viewer")
                .json()
                .await
                .expect("the public-scene RPC must return JSON")
        });
        assert_eq!(
            find_forbidden_public_key(&raw_public_scene),
            None,
            "the real public-scene JSON must omit private guest-import, reset, device, and cycle data"
        );
    }
}
