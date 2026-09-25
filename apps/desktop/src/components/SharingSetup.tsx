import { useState, type FormEvent } from "react";

type Props = {
  phase: "signed_out" | "signed_in";
  busy?: boolean;
  onRequestCode: (email: string) => void | Promise<void>;
  onVerifyCode: (email: string, code: string) => void | Promise<void>;
  onCreateWorld: (name: string) => void | Promise<void>;
  onJoinWorld: (code: string) => void | Promise<void>;
};

export function SharingSetup({ phase, busy, onRequestCode, onVerifyCode, onCreateWorld, onJoinWorld }: Props) {
  const [email, setEmail] = useState("");
  const [code, setCode] = useState("");
  const [codeRequested, setCodeRequested] = useState(false);
  const [worldName, setWorldName] = useState("");
  const [inviteCode, setInviteCode] = useState("");

  function request(event: FormEvent) {
    event.preventDefault();
    void Promise.resolve(onRequestCode(email.trim())).then(() => setCodeRequested(true)).catch(() => {});
  }

  if (phase === "signed_out") {
    return <section className="sharing-panel" aria-label="공동 세계 시작">
      <h2>나만의 세계에서 함께 만드는 세계로</h2>
      <p>로그인 전에도 사용량과 행성은 이 기기에서 계속 자랍니다. 공유를 시작하면 일별 집계만 전송합니다.</p>
      <form onSubmit={request}>
        <label htmlFor="sharing-email">이메일</label>
        <div className="sharing-inline"><input id="sharing-email" type="email" value={email} onChange={(event) => setEmail(event.target.value)} required autoComplete="email" /><button type="submit" disabled={busy}>인증코드 받기</button></div>
      </form>
      {codeRequested && <form onSubmit={(event) => { event.preventDefault(); void onVerifyCode(email.trim(), code.trim()); }}>
        <label htmlFor="sharing-code">이메일 인증코드</label>
        <div className="sharing-inline"><input id="sharing-code" value={code} onChange={(event) => setCode(event.target.value)} required inputMode="numeric" autoComplete="one-time-code" /><button type="submit" disabled={busy}>코드 확인</button></div>
      </form>}
    </section>;
  }

  return <section className="sharing-panel" aria-label="공동 세계 선택">
    <h2>공동 세계를 시작하세요</h2>
    <p>새 세계를 만들거나 친구가 보낸 초대 코드로 참여할 수 있습니다. 계정당 한 세계에 참여합니다.</p>
    <form onSubmit={(event) => { event.preventDefault(); void onCreateWorld(worldName.trim()); }}>
      <label htmlFor="world-name">세계 이름</label>
      <div className="sharing-inline"><input id="world-name" value={worldName} onChange={(event) => setWorldName(event.target.value)} required maxLength={80} placeholder="우리의 작은 궤도" /><button type="submit" disabled={busy}>세계 만들기</button></div>
    </form>
    <form onSubmit={(event) => { event.preventDefault(); void onJoinWorld(inviteCode.trim()); }}>
      <label htmlFor="invite-code">받은 초대 코드</label>
      <div className="sharing-inline"><input id="invite-code" value={inviteCode} onChange={(event) => setInviteCode(event.target.value)} required /><button type="submit" disabled={busy}>초대 코드로 참여</button></div>
    </form>
  </section>;
}
