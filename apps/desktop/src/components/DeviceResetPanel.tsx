import { useEffect, useRef, useState } from "react";
import { acceptResetView, bootstrapLocalLifecycle, invokeLocal, type DeviceResetView, localGeneration, suspendLocalActions } from "../lib/localLifecycle";
export function DeviceResetPanel({onChanged, view}: {onChanged: () => void | Promise<void>; view?: DeviceResetView | null}) {
  const trigger = useRef<HTMLButtonElement | null>(null);
  const dialog = useRef<HTMLDivElement | null>(null);
  const [confirming, setConfirming] = useState(false);
  const [busy, setBusy] = useState(false);
  const running = useRef(false);
  const [current, setCurrent] = useState<DeviceResetView | null>(view ?? null);
  const [message, setMessage] = useState("");
  const [refreshNeeded, setRefreshNeeded] = useState(false);
  useEffect(() => { if (view) setCurrent(view); }, [view]);
  useEffect(() => {
    if (confirming) dialog.current?.querySelector<HTMLButtonElement>("button")?.focus();
  }, [confirming]);
  function cancel() { setConfirming(false); trigger.current?.focus(); }
  async function run() {
    if (running.current) return;
    running.current = true; setBusy(true); setConfirming(false); setMessage("");
    suspendLocalActions();
    try {
      await invokeLocal("reset_device_data", {});
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "초기화를 복구해야 합니다.");
    } finally {
      try {
        const next = await bootstrapLocalLifecycle(); setCurrent(next); acceptResetView(next);
        if (next.state.phase === "completed") {
          setMessage("이 기기의 데이터 초기화와 로그아웃이 완료되었습니다. 새 로컬 행성을 시작할 수 있습니다.");
          try { await onChanged(); setRefreshNeeded(false); } catch { setRefreshNeeded(true); }
        }
      } catch { setMessage("초기화 상태를 확인할 수 없습니다. 상태를 다시 확인해 주세요."); }
      setBusy(false); running.current = false;
    }
  }
  async function retry() {
    if (running.current || !current?.state.request_id) return;
    running.current = true; setBusy(true); setMessage("");
    try {
      await invokeLocal("retry_device_reset",{requestId:current.state.request_id});
      const next = await bootstrapLocalLifecycle(); setCurrent(next);
      if (next.state.phase === "completed") {
        setMessage("이 기기의 데이터 초기화와 로그아웃이 완료되었습니다. 새 로컬 행성을 시작할 수 있습니다.");
        try { await onChanged(); } catch { setRefreshNeeded(true); }
      }
    } catch (error) {
      setMessage(error instanceof Error ? error.message : "초기화를 복구해야 합니다.");
      try { setCurrent(await bootstrapLocalLifecycle()); } catch { /* keep the saved request for retry */ }
    } finally { setBusy(false); running.current = false; }
  }
  const recovering = current?.actions_blocked;
  return <section className="device-reset-panel" aria-label="기기 데이터 설정">
    <button ref={trigger} type="button" disabled={busy || localGeneration() === null} onClick={() => recovering ? void retry() : setConfirming(true)}>{busy ? "기기 초기화 처리 중" : recovering ? "진행 중인 초기화 복구" : "기기 데이터 전체 초기화"}</button>
    {confirming && <div ref={dialog} className="device-reset-dialog" role="dialog" aria-modal="true" aria-label="기기 데이터 전체 초기화 확인" onKeyDown={event => {
      if (event.key === "Escape") { event.preventDefault(); event.stopPropagation(); cancel(); }
      if (event.key === "Tab") {
        const buttons = dialog.current?.querySelectorAll<HTMLButtonElement>("button");
        if (!buttons?.length) return;
        const first = buttons[0], last = buttons[buttons.length - 1];
        if (event.shiftKey && document.activeElement === first) { event.preventDefault(); last.focus(); }
        else if (!event.shiftKey && document.activeElement === last) { event.preventDefault(); first.focus(); }
      }
    }}>
      <p>이 기기의 Token Planet 데이터를 모두 초기화하고 로그아웃합니다. 로컬 행성·지갑·구매·일지·사용량 저장 데이터와 소스 설정이 삭제됩니다. 원장 시간대는 유지됩니다. 서버와 공동 세계의 데이터, Codex·Claude 원본 파일은 남습니다. 현재 익명 계정은 이 앱에서 다시 접근하지 못할 수 있습니다. 계속할까요?</p>
      <p>원본 로그를 다시 읽으면 사용량 집계가 다시 표시될 수 있습니다.</p>
      <button type="button" onClick={() => void run()}>전체 초기화 및 로그아웃</button>
      <button type="button" onClick={cancel}>취소</button>
    </div>}
    {busy && <p role="status">로그아웃과 기기 데이터 초기화를 처리하고 있습니다.</p>}
    {current?.state.phase === "local_committed" && <p role="status">데이터 삭제는 완료됐습니다. 새 행성 화면과 설정을 복구해 주세요.</p>}
    {message && <p role="status">{message}</p>}
    {refreshNeeded && <button type="button" onClick={() => void Promise.resolve().then(onChanged).then(() => setRefreshNeeded(false)).catch(() => setRefreshNeeded(true))}>완료된 화면 다시 확인</button>}
  </section>;
}
