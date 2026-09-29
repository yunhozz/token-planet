import { useRef, useState, type KeyboardEvent as ReactKeyboardEvent, type ReactNode } from "react";
import { AvatarSprite } from "./AvatarSprite";
import { FormattedNumber } from "./FormattedNumber";
import { PlanetScene } from "./PlanetScene";
import type { WorldPlanet } from "../types/usage";

const stages = ["자연 생태계", "정착·농경", "마을·초기 도시", "산업 문명", "첨단·우주 문명"];

function Ranking({ title, metric, members, value }: { title: string; metric: string; members: WorldPlanet[]; value: (member: WorldPlanet) => ReactNode }) {
  return (
    <section className="leaderboard" aria-label={title}>
      <div className="leaderboard-title"><h3>{title}</h3><span>{metric}</span></div>
      <ol>
        {members.map((member, index) => (
          <li key={index}>
            <span className="rank-number">{metric === "누적 토큰" ? member.token_rank : member.civilization_rank}</span>
            <AvatarSprite avatar={member.avatar} />
            <span className="rank-name">{member.nickname}</span>
            <strong>{value(member)}</strong>
          </li>
        ))}
      </ol>
    </section>
  );
}

export function WorldCommunity({ name, members }: { name: string; members: WorldPlanet[] }) {
  const [selectedIndex, setSelectedIndex] = useState(0);
  const memberButtons = useRef<(HTMLButtonElement | null)[]>([]);
  const selected = members[selectedIndex] ?? members[0] ?? null;
  const selectedMemberIndex = selected ? members.indexOf(selected) : -1;
  const tokenRank = [...members].sort((a, b) => a.token_rank - b.token_rank);
  const civilizationRank = [...members].sort((a, b) => a.civilization_rank - b.civilization_rank);

  function handleMemberKeyDown(event: ReactKeyboardEvent<HTMLButtonElement>, index: number) {
    if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
    event.preventDefault();
    const direction = event.key === "ArrowRight" ? 1 : -1;
    const nextIndex = (index + direction + members.length) % members.length;
    setSelectedIndex(nextIndex);
    memberButtons.current[nextIndex]?.focus();
  }

  return (
    <section className="world-community" aria-label="그룹 행성과 순위표">
      <header className="community-heading">
        <div><p className="pixel-kicker">함께 보는 세계</p><h2>{name}</h2></div>
        <span>{members.length}명</span>
      </header>
      {members.length === 0 ? <p className="community-empty">멤버의 행성을 동기화하고 있습니다.</p> : <>
        {selected && <section className="selected-planet-detail" aria-label={`${selected.nickname}의 행성 자세히 보기`}>
          <div className="selected-planet-heading"><strong>{selected.nickname}의 행성</strong></div>
          <PlanetScene key={`${selectedMemberIndex}-${selected.nickname}`} stage={selected.stage} progress={selected.progress_to_next} avatar={selected.avatar} objects={selected.objects} equippedCosmetics={selected.equipped_cosmetics ?? []} animate interactive publicOnly />
          <div className="selected-planet-stats"><span>현재 행성</span><strong><FormattedNumber value={selected.current_planet_tokens} /> 토큰</strong><span>누적 사용량</span><strong><FormattedNumber value={selected.lifetime_tokens} /> 토큰</strong><span>문명 발전</span><strong><FormattedNumber value={selected.growth_credit} maximumFractionDigits={12} /> 크레딧</strong></div>
        </section>}
        <div className="planet-gallery">
          {members.map((member, index) => (
            <button ref={(node) => { memberButtons.current[index] = node; }} className="planet-card" type="button" key={index} aria-pressed={selectedMemberIndex === index} aria-label={`${member.nickname}의 행성 크게 보기`} onClick={() => setSelectedIndex(index)} onKeyDown={(event) => handleMemberKeyDown(event, index)}>
              <div className="planet-card-top"><AvatarSprite avatar={member.avatar} /><span>{stages[member.stage] ?? stages[4]}</span></div>
              <PlanetScene stage={member.stage} progress={member.progress_to_next} avatar={member.avatar} objects={member.objects} equippedCosmetics={member.equipped_cosmetics ?? []} compact />
              <h3>{member.nickname}</h3>
              <div className="planet-card-stats">
                <span>이번 행성</span><strong><FormattedNumber value={member.current_planet_tokens} /></strong>
                <span>누적 사용량</span><strong><FormattedNumber value={member.lifetime_tokens} /></strong>
                <span>발전 점수</span><strong><FormattedNumber value={member.growth_credit} maximumFractionDigits={12} /></strong>
              </div>
              {member.incomplete && <p className="member-incomplete">일부 사용량 확인 중</p>}
              <span className="planet-card-open">{selectedMemberIndex === index ? "선택됨 · 자세히 보는 중" : "행성 크게 보기"}</span>
            </button>
          ))}
        </div>
        <div className="leaderboard-grid">
          <Ranking title="누적 토큰 사용량" metric="누적 토큰" members={tokenRank} value={(member) => <><FormattedNumber value={member.lifetime_tokens} /> 토큰</>} />
          <Ranking title="문명 발전" metric="현재 행성" members={civilizationRank} value={(member) => <><FormattedNumber value={member.growth_credit} maximumFractionDigits={12} /> 크레딧</>} />
        </div>
      </>}
    </section>
  );
}
