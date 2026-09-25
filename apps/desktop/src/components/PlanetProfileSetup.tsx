import { useState, type FormEvent } from "react";
import { AvatarSprite } from "./AvatarSprite";
import type { PlanetAvatar } from "../types/usage";

export function PlanetProfileSetup({ busy, onSave }: { busy: boolean; onSave: (nickname: string, avatar: PlanetAvatar) => void | Promise<void> }) {
  const [nickname, setNickname] = useState("");
  const [avatar, setAvatar] = useState<PlanetAvatar>("masculine");

  function submit(event: FormEvent) {
    event.preventDefault();
    void onSave(nickname.trim(), avatar);
  }

  return (
    <main className="setup-screen">
      <div className="setup-mark" aria-hidden="true"><span /></div>
      <p className="pixel-kicker">나만의 작은 세계</p>
      <h1>행성의 첫 주민을 골라주세요</h1>
      <p className="setup-copy">아바타는 행성에 사는 모습만 바꿉니다. 성장과 토큰에는 영향을 주지 않습니다.</p>
      <form onSubmit={submit}>
        <fieldset className="avatar-options">
          <legend>주민 모습</legend>
          {(["masculine", "feminine"] as const).map((option) => (
            <button
              className={`avatar-option ${avatar === option ? "avatar-option--selected" : ""}`}
              type="button"
              key={option}
              aria-pressed={avatar === option}
              onClick={() => setAvatar(option)}
            >
              <AvatarSprite avatar={option} />
              <span>{option === "masculine" ? "남성" : "여성"}</span>
            </button>
          ))}
        </fieldset>
        <label className="field-label" htmlFor="planet-nickname">행성에서 사용할 닉네임</label>
        <input id="planet-nickname" className="pixel-input" value={nickname} onChange={(event) => setNickname(event.target.value)} maxLength={24} autoComplete="nickname" required />
        <button className="primary-action setup-submit" type="submit" disabled={busy || !nickname.trim()}>{busy ? "저장 중…" : "행성 시작하기"}</button>
      </form>
    </main>
  );
}
