type Props = {
  memberCount: number;
  isOwner: boolean;
  memberCode: string | null;
  busy?: boolean;
  onRotate: () => void | Promise<void>;
};

export function InvitePanel({ memberCount, isOwner, memberCode, busy, onRotate }: Props) {
  return <section className="sharing-panel" aria-label="세계 초대">
    <div className="sharing-heading"><h2>함께 자라는 세계</h2><span>{memberCount}명 / 10명</span></div>
    <p>멤버 모두 닉네임, 행성 모습, 개편 후 누적 토큰 사용량과 현재 문명 발전 점수의 정확한 값과 순위를 볼 수 있습니다.</p>
    <p>{isOwner
      ? "내 개인 코드로 친구를 공동 행성에 초대할 수 있습니다. 다시 발급하면 이전 코드는 사용할 수 없습니다."
      : "각자 개인 코드가 있습니다. 공동 행성에 초대할 수 있는 코드는 소유자의 코드뿐입니다."}</p>
    {memberCode
      ? <div className="invite-current"><code>{memberCode}</code><button type="button" onClick={() => void navigator.clipboard.writeText(memberCode)}>내 코드 복사</button></div>
      : <p role="status">내 개인 코드를 불러오고 있습니다.</p>}
    <button className="sharing-action" type="button" onClick={() => void onRotate()} disabled={busy}>
      {busy ? "처리 중…" : "내 개인 코드 다시 발급"}
    </button>
  </section>;
}
