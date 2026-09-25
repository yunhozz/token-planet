type Props = {
  status: "local" | "queued" | "syncing" | "synced" | "paused" | "failed";
  pending: number;
  lastSyncedAt?: string | null;
  onPause: () => void | Promise<void>;
  onResume: () => void | Promise<void>;
};

const message = {
  local: "이 기기에만 저장 중",
  queued: "연결되면 집계 전송",
  syncing: "일별 집계 전송 중",
  synced: "공동 세계와 동기화됨",
  paused: "동기화 일시정지 · 로컬 수집은 계속됨",
  failed: "동기화 실패 · 연결을 확인하세요",
};

export function SyncStatus({ status, pending, lastSyncedAt, onPause, onResume }: Props) {
  return <div className="sync-status" role="status">
    <span className={`sync-dot sync-dot--${status}`} aria-hidden="true" />
    <span>{message[status]}{pending > 0 && ` · ${pending}개 대기`}{lastSyncedAt && status === "synced" && ` · ${new Date(lastSyncedAt).toLocaleString()}`}</span>
    {status === "paused" ? <button type="button" onClick={() => void onResume()}>동기화 재개</button> : status !== "local" && <button type="button" onClick={() => void onPause()}>일시정지</button>}
  </div>;
}
