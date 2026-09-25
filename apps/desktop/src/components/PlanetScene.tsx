const stageNames = ["성운의 핵", "첫 지각", "바다와 대륙", "대기의 숨", "궤도의 달"];

function layer(stage: number, threshold: number, progress: number) {
  return stage > threshold ? 1 : stage === threshold ? progress : 0;
}

export function PlanetScene({ stage, progress }: { stage: number; progress: number }) {
  const crust = layer(stage, 0, progress);
  const ocean = layer(stage, 1, progress);
  const atmosphere = layer(stage, 2, progress);
  const moon = layer(stage, 3, progress);
  return (
    <figure className="planet-figure">
      <svg className="planet-svg" viewBox="0 0 420 340" role="img" aria-label={`행성 성장 단계: ${stageNames[stage] ?? stageNames[4]}`}>
        <defs>
          <radialGradient id="world-core" cx="32%" cy="28%" r="72%">
            <stop offset="0" stopColor="#8599c2" />
            <stop offset="0.52" stopColor="#506482" />
            <stop offset="1" stopColor="#27354f" />
          </radialGradient>
          <linearGradient id="world-sea" x1="0" y1="0" x2="1" y2="1">
            <stop offset="0" stopColor="#91c6c4" />
            <stop offset="1" stopColor="#377a8b" />
          </linearGradient>
          <clipPath id="world-disc"><circle cx="210" cy="171" r="119" /></clipPath>
        </defs>
        <circle cx="210" cy="171" r="150" fill="#7a91bb" opacity="0.06" />
        <circle cx="210" cy="171" r="134" fill="#91a6d8" opacity={0.1 + atmosphere * 0.17} />
        <ellipse cx="210" cy="171" rx="176" ry="49" fill="none" stroke="#b6c8dc" strokeWidth="2" opacity={moon * 0.54} transform="rotate(-17 210 171)" />
        <circle cx="210" cy="171" r="119" fill="url(#world-core)" />
        <g clipPath="url(#world-disc)">
          <g opacity={crust} fill="#d9a17e">
            <path d="M91 128 128 83 167 104 177 142 148 163 107 157Z" />
            <path d="m183 68 44-24 43 27-6 39-46 23-31-21Z" />
            <path d="m274 120 55-20 37 46-15 50-51 17-34-40Z" />
            <path d="m135 209 51-29 55 22 11 48-43 57-63-15Z" />
          </g>
          <g opacity={ocean}>
            <path d="M75 185c39-23 63-17 92 4 30 21 56 25 86 5 24-16 65-19 99-4v116H75Z" fill="url(#world-sea)" />
            <path d="m109 139 24-10 20 13-14 14-22-4Zm170-42 27-5 13 18-11 15-30-7Zm-71 139 28-11 23 16-11 19-35-1Z" fill="#d9a17e" />
          </g>
          <g opacity={atmosphere} fill="none" stroke="#dce9eb" strokeLinecap="round">
            <path d="M95 150c34-16 57-14 83-5m58-59c32-3 54 1 78 14M114 244c30-10 61-9 91 1m48 20c23-1 44-7 62-19" strokeWidth="7" opacity="0.66" />
            <path d="M89 171c28-9 53-8 76-4m94-30c22-5 47-3 70 6" strokeWidth="3" opacity="0.6" />
          </g>
          <path d="M209 52a119 119 0 0 1 0 238c53-47 76-165 0-238Z" fill="#18263d" opacity="0.21" />
        </g>
        <circle cx="210" cy="171" r="119" fill="none" stroke="#b5c8d6" strokeWidth="1.5" opacity="0.48" />
        <circle cx="356" cy="116" r="17" fill="#e1d8be" opacity={moon} />
        <circle cx="350" cy="111" r="5" fill="#c0b7a6" opacity={moon * 0.6} />
      </svg>
      <figcaption className="planet-caption">{stageNames[stage] ?? stageNames[4]}</figcaption>
    </figure>
  );
}
