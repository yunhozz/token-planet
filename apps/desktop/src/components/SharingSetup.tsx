import { useRef, useState, type FormEvent } from "react";

type Props = {
  phase: "signed_out" | "signed_in";
  initialNickname: string;
  busy?: boolean;
  onStartAnonymousSession: () => void | Promise<void>;
  onCreateWorld: (name: string, nickname: string) => void | Promise<void>;
  onJoinWorld: (code: string, nickname: string) => void | Promise<void>;
};

export function SharingSetup({ phase, initialNickname, busy, onStartAnonymousSession, onCreateWorld, onJoinWorld }: Props) {
  const [nickname, setNickname] = useState(initialNickname);
  const [worldName, setWorldName] = useState("");
  const [memberCode, setMemberCode] = useState("");
  const [submitting, setSubmitting] = useState(false);
  const submitRef = useRef(false);
  const disabled = busy || submitting;
  async function submit(action: () => void | Promise<void>) {
    if (busy || submitRef.current) return;
    submitRef.current = true; setSubmitting(true);
    try { await action(); } finally { submitRef.current = false; setSubmitting(false); }
  }

  if (phase === "signed_out") {
    return <section className="sharing-panel" aria-label="공동 세계 시작">
      <h2>나만의 세계에서 함께 만드는 세계로</h2>
      <p>공유를 시작하면 이 기기에 익명 계정을 만들고 보안 저장소에 보관합니다. 계정은 이 기기에 묶이며, 앱 데이터를 잃으면 복구할 수 없습니다. 다른 기기에서는 별도의 사용자가 됩니다.</p>
      <button className="sharing-action" type="button" onClick={() => void submit(onStartAnonymousSession)} disabled={disabled}>
        {busy ? "공유 계정을 만드는 중…" : "공유 시작하기"}
      </button>
    </section>;
  }

  const trimmedNickname = nickname.trim();
  const normalizedCode = memberCode.trim().toLowerCase();
  const validCode = memberCode.length <= 256 && /^[0-9a-f]{64}$/.test(normalizedCode);
  return <section className="sharing-panel" aria-label="공동 세계 선택">
    <h2>공동 세계를 시작하세요</h2>
    <p>이 기기의 사용자 이름입니다. 같은 닉네임을 여러 사람이 사용할 수 있습니다. 계정당 한 공동 세계에 참여합니다.</p>
    <label htmlFor="sharing-nickname">공동 행성에서 사용할 닉네임</label>
    <input
      id="sharing-nickname"
      className="pixel-input"
      value={nickname}
      onChange={(event) => setNickname(event.target.value)}
      maxLength={24}
      autoComplete="nickname"
      required
    />
    <form onSubmit={(event: FormEvent) => { event.preventDefault(); if (worldName.trim() && trimmedNickname) void submit(() => onCreateWorld(worldName.trim(), trimmedNickname)); }}>
      <label htmlFor="world-name">세계 이름</label>
      <div className="sharing-inline"><input id="world-name" value={worldName} onChange={(event) => setWorldName(event.target.value)} required maxLength={80} placeholder="우리의 작은 궤도" /><button type="submit" disabled={disabled || !trimmedNickname}>세계 만들기</button></div>
    </form>
    <form onSubmit={(event: FormEvent) => { event.preventDefault(); if (validCode && trimmedNickname) void submit(() => onJoinWorld(normalizedCode, trimmedNickname)); }}>
      <label htmlFor="member-code">소유자가 알려준 64자리 초대 코드</label>
      <div className="sharing-inline"><input id="member-code" value={memberCode} onChange={(event) => setMemberCode(event.target.value)} required maxLength={256} autoCapitalize="none" autoComplete="off" spellCheck={false} /><button type="submit" disabled={disabled || !trimmedNickname || !validCode}>코드로 참여</button></div>
    </form>
    <p>7일 안에 한 번 사용할 수 있습니다. 가입 응답을 받지 못했다면 같은 코드로 다시 시도하세요.</p>
  </section>;
}
