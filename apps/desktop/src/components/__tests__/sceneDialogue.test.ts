import { describe, expect, it } from "vitest";
import { pickDialogue } from "../sceneDialogue";

describe("pickDialogue", () => {
  it("announces a newly added object with a fixed kind name", () => {
    expect(pickDialogue({ target: "planet", stage: 0, progress: 0, newObjectKind: "tree", publicOnly: false }, () => 0))
      .toBe("새로운 나무가 생겼어!");
    expect(pickDialogue({ target: "planet", stage: 0, progress: 0, newObjectKind: "untrusted-kind", publicOnly: false }, () => 0))
      .toBe("새로운 오브젝트가 생겼어!");
  });

  it("uses normal progress dialogue for an incomplete personal collection", () => {
    expect(pickDialogue({ target: "planet", stage: 1, progress: 0.95, incomplete: true, publicOnly: false }, () => 0))
      .toBe("다음 시대가 가까워!");
  });

  it("announces a nearby next stage before the current stage", () => {
    expect(pickDialogue({ target: "planet", stage: 1, progress: 0.85, publicOnly: false }, () => 0))
      .toContain("다음 시대");
    expect(pickDialogue({ target: "planet", stage: 1, progress: 0.84, publicOnly: false }, () => 0))
      .toBe("작은 정착지가 생겼어.");
  });

  it("has a fixed line for every planet stage", () => {
    const expected = ["이곳에서 첫발을 떼자.", "작은 정착지가 생겼어.", "마을이 제법 커졌어.", "도시가 분주해졌어.", "우주까지 닿았어."];
    expected.forEach((line, stage) => {
      expect(pickDialogue({ target: "planet", stage, progress: 0, publicOnly: false }, () => 0)).toBe(line);
    });
  });

  it("uses avatar-specific default lines", () => {
    expect(pickDialogue({ target: "avatar", stage: 0, progress: 0, publicOnly: false }, () => 0))
      .toBe("오늘은 어디를 둘러볼까?");
  });

  it("avoids the previous line when the selected pool has another choice", () => {
    expect(pickDialogue({ target: "avatar", stage: 0, progress: 0, publicOnly: false, previous: "오늘은 어디를 둘러볼까?" }, () => 0))
      .toBe("잠깐 쉬었다 가자.");
  });

  it("ignores personal-only context in public scenes", () => {
    const line = pickDialogue({ target: "planet", stage: 1, progress: 0.9, publicOnly: true, incomplete: true, newObjectKind: "tree" }, () => 0);
    expect(line).toContain("다음 시대");
    expect(line).not.toContain("기록");
    expect(line).not.toContain("나무");
  });
});
