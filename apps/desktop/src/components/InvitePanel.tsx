import { useEffect, useRef, useState } from "react";
import { sharing, type CreatedWorldInvite, type WorldInvite } from "../lib/sharing";

type Props = { memberCount: number; isOwner: boolean; contextKey: string; busy?: boolean };
const labels = { active: "사용 가능", used: "사용됨", revoked: "철회됨", expired: "만료됨" };

export function InvitePanel({ memberCount, isOwner, contextKey, busy }: Props) {
  const [created, setCreated] = useState<{ context: string; value: CreatedWorldInvite } | null>(null);
  const [list, setList] = useState<{ context: string; value: WorldInvite[] } | null>(null);
  const [pending, setPending] = useState(false);
  const [error, setError] = useState("");
  const [notice, setNotice] = useState("");
  const epoch = useRef(0);
  const mutationContext = useRef(0);
  const listSequence = useRef(0);
  const submitting = useRef(false);
  const context = JSON.stringify([contextKey, isOwner]);
  const current = created?.context === context ? created.value : null;
  const invites = list?.context === context ? list.value : [];

  useEffect(() => {
    const generation = ++epoch.current;
    ++mutationContext.current;
    submitting.current = false;
    setCreated(null); setList(null); setError(""); setNotice(""); setPending(false);
    if (isOwner) void loadList(generation, "초대 목록을 불러오지 못했습니다. 새로고침하세요.");
    const hide = () => {
      if (document.visibilityState !== "hidden") return;
      // Hide sensitive responses, but keep an in-flight mutation locked until it settles.
      ++epoch.current;
      setCreated(null); setNotice("");
    };
    document.addEventListener("visibilitychange", hide);
    return () => { ++epoch.current; ++mutationContext.current; document.removeEventListener("visibilitychange", hide); };
  }, [context, isOwner]);

  async function manage(action: () => Promise<void>) {
    if (!isOwner || busy || submitting.current) return;
    const generation = mutationContext.current;
    // A previous list request cannot replace this action's recovery guidance.
    ++listSequence.current;
    submitting.current = true; setPending(true); setError(""); setNotice("");
    try { await action(); }
    finally { if (generation === mutationContext.current) { submitting.current = false; setPending(false); } }
  }

  async function loadList(generation: number, failureMessage: string) {
    const sequence = ++listSequence.current;
    try {
      const value = await sharing.listInvites();
      if (generation === epoch.current && sequence === listSequence.current) {
        setList({ context, value });
        setError("");
      }
    } catch {
      if (generation === epoch.current && sequence === listSequence.current) setError(failureMessage);
    }
  }

  async function issue() {
    const generation = epoch.current;
    try {
      const value = await sharing.createInvite();
      if (generation !== epoch.current) return;
      setCreated({ context, value });
      await loadList(generation, "초대 목록을 확인하지 못했습니다. 목록을 새로고침하세요.");
    } catch {
      if (generation === epoch.current) setError("발급 응답 또는 목록을 확인하지 못했습니다. 목록을 새로고침해 초대를 철회한 뒤 다시 발급하세요.");
    }
  }

  async function revoke(inviteId: string) {
    const generation = epoch.current;
    try {
      const result = await sharing.revokeInvite(inviteId);
      if (generation !== epoch.current) return;
      setCreated((entry) => entry?.value.invite_id === inviteId ? null : entry);
      setNotice(result.status === "used" ? "이미 사용된 초대입니다. 기존 멤버는 유지됩니다." : "초대를 철회했습니다.");
      await loadList(generation, "철회 후 목록을 확인하지 못했습니다. 새로고침해 상태를 확인하세요.");
    } catch {
      if (generation === epoch.current) setError("철회 결과 또는 목록을 확인하지 못했습니다. 새로고침해 상태를 확인하세요.");
    }
  }

  async function refresh() {
    await loadList(epoch.current, "초대 목록을 불러오지 못했습니다.");
  }

  async function copy() {
    if (!current) return;
    const generation = epoch.current;
    try {
      await navigator.clipboard.writeText(current.code);
      if (generation === epoch.current) { setError(""); setNotice("초대 코드를 복사했습니다."); }
    } catch { if (generation === epoch.current) setError("초대 코드를 복사하지 못했습니다. 표시된 코드를 직접 복사하세요."); }
  }

  return <section className="sharing-panel" aria-label="세계 초대">
    <div className="sharing-heading"><h2>함께 자라는 세계</h2><span>{memberCount}명 / 10명</span></div>
    <p>멤버 모두 닉네임, 행성 모습, 개편 후 누적 토큰 사용량과 현재 문명 발전 점수의 정확한 값과 순위를 볼 수 있습니다.</p>
    {isOwner ? <>
      <p>초대는 발급 후 7일 동안 한 번의 가입에 사용할 수 있습니다. 코드는 발급 직후에만 표시되며 다시 조회할 수 없습니다.</p>
      <button className="sharing-action" type="button" disabled={busy || pending || memberCount >= 10} onClick={() => void manage(issue)}>초대 발급</button>
      {memberCount >= 10 && <p>정원 10명이 모두 찼습니다.</p>}
      {current && <div className="invite-current"><code>{current.code}</code><p>발급: {new Date(current.created_at).toLocaleString("ko-KR")}<br />만료: {new Date(current.expires_at).toLocaleString("ko-KR")}</p><button type="button" onClick={() => void copy()}>초대 코드 복사</button></div>}
      <button type="button" disabled={busy || pending} onClick={() => void manage(refresh)}>초대 목록 새로고침</button>
      <ul>{invites.map((invite) => <li key={invite.invite_id}>
        <span>{labels[invite.status]}</span> · 발급 {new Date(invite.created_at).toLocaleString("ko-KR")} · 만료 {new Date(invite.expires_at).toLocaleString("ko-KR")}
        {(invite.status === "active" || invite.status === "expired") && <button type="button" disabled={busy || pending} onClick={() => void manage(() => revoke(invite.invite_id))}>초대 철회</button>}
      </li>)}</ul>
      {list?.context === context && invites.length === 0 && <p>발급한 초대가 없습니다.</p>}
      {notice && <p role="status">{notice}</p>}
      {error && <p role="alert">{error}</p>}
    </> : <p>그룹 소유자에게 새 초대를 요청하세요.</p>}
  </section>;
}
