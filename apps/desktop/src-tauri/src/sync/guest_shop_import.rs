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
