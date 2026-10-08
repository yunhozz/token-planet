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
  "이곳에서 첫발을 떼자.",
  "작은 정착지가 생겼어.",
  "마을이 제법 커졌어.",
  "도시가 분주해졌어.",
  "우주까지 닿았어.",
];

const DEFAULT_LINES: Record<DialogueTarget, string[]> = {
  avatar: ["오늘은 어디를 둘러볼까?", "잠깐 쉬었다 가자."],
  planet: ["행성을 한 바퀴 돌아볼까?", "새로운 풍경을 찾아보자."],
};

function sample(random: () => number) {
  const value = random();
  return Math.max(0, Math.min(1 - Number.EPSILON, Number.isFinite(value) ? value : 0));
}

export function pickDialogue(context: DialogueContext, random: () => number): string {
  const newKind = context.publicOnly ? null : context.newObjectKind;
  let pool: string[];

  if (newKind) {
    pool = [`새로운 ${KIND_NAMES[newKind] ?? "오브젝트"}가 생겼어!`];
  } else if (context.stage < STAGE_LINES.length - 1 && context.progress >= 0.85) {
    pool = ["다음 시대가 가까워!"];
  } else if (context.target === "planet" && STAGE_LINES[context.stage]) {
    pool = [STAGE_LINES[context.stage], ...DEFAULT_LINES.planet];
  } else {
    pool = DEFAULT_LINES[context.target];
  }

  const alternatives = pool.filter((line) => line !== context.previous);
  const choices = alternatives.length > 0 ? alternatives : pool;
  return choices[Math.floor(sample(random) * choices.length)];
}
