export type DialogueTarget = "avatar" | "planet";

export type DialogueContext = {
  target: DialogueTarget;
  stage: number;
  progress: number;
  incomplete?: boolean;
  newObjectKind?: string | null;
  publicOnly: boolean;
  previous?: string | null;
};

const KIND_NAMES: Record<string, string> = {
  tree: "나무",
  rock: "바위",
  creature: "생물",
};

const STAGE_LINES = [
  [
    "이곳에서 첫발을 떼자.",
    "첫 풍경을 천천히 둘러보자.",
    "어디서부터 시작해 볼까?",
    "우리의 시작을 여기에 남기자.",
  ],
  [
    "작은 정착지가 생겼어.",
    "정착지를 한 바퀴 둘러볼까?",
    "이곳에 조금씩 익숙해져 가자.",
    "작은 시작도 찬찬히 살펴보자.",
  ],
  [
    "마을이 제법 커졌어.",
    "마을 풍경을 구석구석 살펴보자.",
    "이 마을에서 어디가 가장 마음에 들어?",
    "마을을 돌아보며 잠깐 쉬어 가자.",
  ],
  [
    "도시가 분주해졌어.",
    "도시 풍경을 천천히 담아 보자.",
    "도시를 한 바퀴 둘러볼까?",
    "도시에서도 작은 풍경을 찾아보자.",
  ],
  [
    "우주까지 닿았어.",
    "우주에서 우리 행성을 바라볼까?",
    "더 넓은 풍경을 상상해 보자.",
    "여기까지 온 길을 돌아보자.",
  ],
];

const DEFAULT_LINES: Record<DialogueTarget, string[]> = {
  avatar: [
    "오늘은 어디를 둘러볼까?",
    "잠깐 쉬었다 가자.",
    "천천히 걸어도 괜찮아.",
    "마음에 드는 곳에서 잠깐 멈춰 볼까?",
    "다음엔 어느 쪽으로 가 볼까?",
    "작은 풍경도 놓치지 말자.",
  ],
  planet: [
    "행성을 한 바퀴 돌아볼까?",
    "새로운 풍경을 찾아보자.",
    "어느 쪽부터 살펴볼까?",
    "마음에 드는 풍경을 골라 보자.",
    "구석구석 천천히 둘러보자.",
    "다른 각도에서 바라볼까?",
  ],
};

const NEAR_STAGE_LINES = [
  "다음 시대가 가까워!",
  "다음 시대의 풍경이 궁금해.",
  "다음 시대를 함께 기다려 보자.",
];

function sample(random: () => number) {
  const value = random();
  return Math.max(0, Math.min(1 - Number.EPSILON, Number.isFinite(value) ? value : 0));
}

export function pickDialogue(context: DialogueContext, random: () => number): string {
  const newKind = context.publicOnly ? null : context.newObjectKind;
  let pool: string[];

  if (newKind) {
    const kindName = KIND_NAMES[newKind] ?? "오브젝트";
    pool = [
      `새로운 ${kindName}가 생겼어!`,
      `새로 생긴 ${kindName}, 같이 살펴볼까?`,
      `풍경에 더해진 ${kindName}도 눈여겨보자.`,
    ];
  } else if (context.stage < STAGE_LINES.length - 1 && context.progress >= 0.85) {
    pool = NEAR_STAGE_LINES;
  } else if (context.target === "planet" && STAGE_LINES[context.stage]) {
    pool = [...STAGE_LINES[context.stage], ...DEFAULT_LINES.planet];
  } else {
    pool = DEFAULT_LINES[context.target];
  }

  const alternatives = pool.filter((line) => line !== context.previous);
  const choices = alternatives.length > 0 ? alternatives : pool;
  return choices[Math.floor(sample(random) * choices.length)];
}
