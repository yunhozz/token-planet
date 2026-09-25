import type { InviteInfo, InviteLink } from "../lib/sharing";

type Props = {
  memberCount: number;
  isOwner: boolean;
  invite: InviteLink | null;
  invites: InviteInfo[];
  onCreate: () => void | Promise<void>;
  onRevoke: (inviteId: string) => void | Promise<void>;
};

export function InvitePanel({ memberCount, isOwner, invite, invites, onCreate, onRevoke }: Props) {
  return <section className="sharing-panel" aria-label="세계 초대">
    <div className="sharing-heading"><h2>함께 자라는 세계</h2><span>{memberCount}명 / 10명</span></div>
    <p>친구의 사용량은 합계에만 반영됩니다. 각자의 정확한 사용량은 본인 기기에서만 볼 수 있습니다.</p>
    {isOwner && <>
      <button className="sharing-action" type="button" onClick={() => void onCreate()} disabled={memberCount >= 10}>초대 코드 만들기</button>
      {invite && <div className="invite-current"><span>지금 만든 코드 · 7일 동안 1회 사용</span><code>{invite.code}</code><button type="button" onClick={() => void navigator.clipboard.writeText(invite.code)}>코드 복사</button></div>}
      {invites.filter((item) => !item.used_at && !item.revoked_at && new Date(item.expires_at) > new Date()).map((item) => <div className="invite-row" key={item.invite_id}><span>{new Date(item.expires_at).toLocaleDateString()} 만료</span><button type="button" onClick={() => void onRevoke(item.invite_id)}>초대 취소</button></div>)}
    </>}
  </section>;
}
