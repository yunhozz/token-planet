# 세부 행성 풍경 개선 구현 계획

> **실행 지침:** CEO가 순차 조정한다. Coder는 담당 파일에 테스트를 먼저 추가하고, 구현 후 계획된 검증을 수행한다. 체크박스로 각 단계를 추적한다.

**목표:** 세부 행성 화면의 무작위 산책, 공통 하단 선택 카드, 픽셀 풍경을 개선하고 확대·축소·전체 보기 및 자동 배율 변경을 제거한다.

**구조:** 기존 React/SVG 풍경을 유지한다. 캐릭터는 지면 격자의 인접 8방향을 무작위로 이동한다. 자연물과 구매 장식은 선택을 상호 배타적으로 연결하고, 출처별 내용과 동작은 같은 하단 카드 프레임에 표시한다. 카메라의 초기 프레이밍과 패닝은 유지한다.

**기술:** React 19, TypeScript, SVG, CSS, Vitest, React Testing Library. 기존 Node.js `22.21.0`, npm 환경을 사용한다.

**설계 근거:** 2026-10-07 사용자가 승인한 설계. 변경 범위가 기존 풍경 안에 한정된 bounded 작업이므로 별도 서면 spec은 만들지 않는다.

## 공통 제약

- 작업 경로는 `/Users/ma-24-007/Desktop/workspace/token-planet`이다.
- 새 패키지, 저장 스키마, Rust 배치 판정 및 상점 API 계약을 변경하지 않는다.
- 아바타의 `24×30` footprint와 최대 `3px` 아래쪽 bob 전체를 지면 안에 둔다.
- 8방향 이웃 중 무작위 이동을 선택하고, 다른 후보가 있으면 직전 점으로 즉시 돌아가지 않는다.
- 기존 1–5점 이동 묶음, 걷기·휴식·깜박임, 화면 비활성 및 reduced-motion 수명을 유지한다.
- 자연물 제거의 기존 callback, 권한 및 cycle key 검사를 유지한다.
- 기본/구매 오브젝트 선택은 상호 배타적으로 처리한다. 선택 카드 프레임과 위치는 같게 하고, 배치된 구매 장식에만 `위치 이동`과 `보관함으로`를 표시한다.
- 미배치 구매 장식과 draft의 `배치 시작`, `배치 확정`, `취소`, 오류 및 pending 흐름을 유지한다.
- 확대·축소·전체 보기 UI와 자동 배율 변경은 제거한다. 기존 초기 프레이밍, 드래그 패닝, 방향키 패닝은 유지한다.
- 픽셀 아트 원칙은 SVG로 새로 그린다. 조사한 작가의 작품·에셋을 복사하지 않는다.
- `docs/superpowers/specs/2026-10-07-planet-shop-walkthrough-layering-design.md`의 고정 지그재그와 끝점 반전 요구, `docs/superpowers/specs/2026-09-30-planet-landscape-exploration-design.md`의 확대·축소·전체 보기 및 선택 시 자동 확대 요구는 이 승인 범위로 대체한다.

## 검토 초점

1. 작은 지형에서 이동점이 0개·1개이거나 선택 가능한 이웃이 하나뿐일 때 안전하게 멈추고, 필요한 경우에만 직전 점으로 돌아가는지 확인한다.
2. 지형 bounds 변경, 화면 숨김, reduced motion, unmount 뒤에도 이전 타이머가 남지 않고 캐릭터가 새 지면 밖에 나타나지 않는지 확인한다.
3. 상점 요청 pending 중 선택 전환이 draft와 요청 결과를 다른 오브젝트에 연결하지 않는지 확인한다.
4. 선택 대상 소멸, account/cycle/version 변경 뒤 오래된 카드·draft·identity가 남지 않는지 확인한다.
5. 패닝·좁은 화면에서 배경 질감이 world 좌표에 고정되고 포인터 입력을 가로채지 않으며 카드 조작이 접근 가능한지 확인한다.

## 파일 책임

| 파일 | 책임 |
|---|---|
| `apps/desktop/src/components/landscapeAvatarRoute.ts` | 지면 내 이동 격자와 무작위 인접점 선택 |
| `apps/desktop/src/components/PlanetLandscape.tsx` | 캐릭터 타이머, 선택 동기화, 공통 카드, 카메라 컨트롤 제거 |
| `apps/desktop/src/components/planetLandscapeCamera.ts` | 선택 focus에서 배율을 바꾸지 않는 카메라 계산 |
| `apps/desktop/src/components/PlanetLandscapeDecorations.tsx` | 시대별 배경·지면 픽셀 레이어와 결정적 world 질감 |
| `apps/desktop/src/App.css` | 공통 카드 프레임, 반응형 규칙, 장식 대비 |
| `apps/desktop/src/components/__tests__/landscapeAvatarRoute.test.ts` | bounds·이웃·난수·역행 조건 |
| `apps/desktop/src/components/__tests__/planetLandscapeCamera.test.ts` | focus 배율 보존 및 카메라 기하 |
| `apps/desktop/src/components/__tests__/PlanetLandscape.test.tsx` | 타이머·선택·배치·카메라·장식 회귀 |

## 조사한 픽셀 아트 기준

- Raymond Schlitter의 [Landscape Backgrounds](https://www.slynyrd.com/blog/2026/5/27/pixelblog-62-landscape-backgrounds)는 하늘·원경·중경·전경을 색 띠와 층으로 쌓고, 멀어질수록 채도·대비를 낮추며 하늘색 쪽으로 이동하는 대기 원근을 제시한다. 가까운 풀에는 더 큰 클러스터를, 먼 곳에는 작은 질감 또는 생략을 사용한다. 상호작용 스프라이트 가독성을 위해 배경을 단순화할 수 있다.
- [Side View Tiles](https://www.slynyrd.com/blog/2020/5/21/pixelblog-28-side-view-tiles)는 적은 색, 덩어리형 질감, 반복에 작은 변형을 주는 방식, 빈 공간 보존, 바닥의 기울어진 잔디 테두리를 권한다. 여러 단순한 배경 레이어가 세밀한 한두 레이어보다 읽기 쉬운 깊이를 만든다.
- 이를 시대별 기존 팔레트에 적용한다. 낮은 대비의 계단형 원경 2–3층, 명암이 있는 구름, 잔디 윗면과 어두운 흙 단면, 불규칙한 잔디·돌·흙 클러스터를 추가한다. 자연물·구매 장식·아바타 주변 대비를 보존하고 난수 기반 재렌더링은 하지 않는다.

## 작업 1: 지면 위 2차원 무작위 산책

**파일:** `landscapeAvatarRoute.ts`, `PlanetLandscape.tsx`, 관련 두 테스트.

**인터페이스:** 기존 `landscapeAvatarRoute(bounds): AvatarRoutePoint[]`는 유효한 아바타 위치 격자를 반환한다. 다음 helper를 추가한다.

```ts
export function nextLandscapeAvatarPoint(
  points: readonly AvatarRoutePoint[],
  currentIndex: number,
  previousIndex: number | null,
  random: () => number = Math.random,
): number;
```

빈 격자는 `0`, 한 점뿐인 격자는 `0`을 반환한다. 인접 후보는 X/Y 격자에서 한 칸 이내인 최대 8개다. `currentIndex`가 유효하지 않으면 `0`을 반환한다. RNG의 음수·비유한 값은 `0`, `1` 이상은 `1 - Number.EPSILON`으로 정규화한다.

- [ ] **실패 테스트 작성:** 지면 격자의 모든 위치 footprint, X/Y 간격, 원점 이동, 비정수 bounds 검증을 유지한다. 고정 전체 순회 테스트를 대체해 가운데 점에서 각 8방향을 재현하는 seeded callback 테스트, 직전점 제외, 유일한 후보, 빈/한 점, 잘못된 index가 `0`으로 복구되는지, RNG `0`·`1`·음수·`NaN`이 위 규칙대로 처리되는지 확인한다.
- [ ] **RED 확인:** `apps/desktop`에서 `npm test -- src/components/__tests__/landscapeAvatarRoute.test.ts`를 실행한다. 새 helper 또는 새 조건이 없어서 실패해야 한다.
- [ ] **최소 구현:** 유효한 8방향 이웃을 격자 index 순으로 정렬하고, 대체 후보가 있으면 previous index를 제외한다. RNG를 `[0, 1)` 범위로 정규화해 후보를 하나 선택한다. 고정 경로 진행 함수는 사용처를 확인한 뒤 불필요한 경우에만 제거한다.
- [ ] **타이머 실패 테스트 작성:** 고정 순서 기대를 바꾸어 수직·대각선 이동과 직전점 회피를 재현한다. 기존 1–5점 묶음, 방향, bounds 변경, 숨김, reduced motion, unmount 타이머 정리도 확인한다.
- [ ] **타이머 구현:** `PlanetLandscape`에서 각 이동 단계의 목적지를 helper로 고르고 현재/직전 index를 갱신한다. 이전/새 지면 좌표 간 이동 거리 비율에 맞춰 CSS transition과 타이머 간격을 동일하게 조정하고 최소 `1ms`를 유지한다. X 좌표가 바뀔 때만 facing을 갱신한다.
- [ ] **통과 확인:** `npm test -- src/components/__tests__/landscapeAvatarRoute.test.ts src/components/__tests__/PlanetLandscape.test.tsx`를 실행해 경로, 수명, 레이어 테스트가 통과하는지 확인한다.

## 작업 2: 기본/구매 오브젝트 공통 하단 선택 카드

**파일:** `PlanetLandscape.tsx`, `App.css`, `PlanetLandscape.test.tsx`.

**인터페이스:** `PlanetExplorationState`와 기존 상점 callback signatures를 유지한다. 자연물과 구매 장식 클릭·키보드 선택이 서로의 선택을 해제한다. 기본 오브젝트 카드와 구매 장식 카드는 `.planet-landscape-object-detail` 공통 문서 흐름 프레임 안에서 출처별 내용을 렌더링한다.

- [ ] **실패 테스트 작성:** 자연물 → 배치된 구매 장식 → 자연물 전환을 수행하는 상태 연결 wrapper를 둔다. 항상 선택 카드 프레임이 최대 하나이고 풍경 viewport 다음 하단 위치에 렌더링되는지 확인한다. 자연물에는 `위치 이동`·`보관함으로`가 없고, 배치된 구매물에만 두 버튼이 있으며 자연물 전용 제거 버튼은 기존 권한에 따라 동작하는지 확인한다.
- [ ] **RED 확인:** `npm test -- src/components/__tests__/PlanetLandscape.test.tsx`를 실행한다. 중복 카드 또는 분기 조건 때문에 새 검증이 실패해야 한다.
- [ ] **선택 구현:** 자연물 선택 시 배치 중이 아닌 기존 구매 선택과 임시 draft를 정리한다. 구매물 선택 시 `selectedObjectId`를 해제하고 기존 callback을 쓴다. 저장 요청/pending 중에는 선택 전환을 막는다. props로 주입되는 미배치 구매 선택에도 자연물 해제를 적용한다. 삭제된 대상 및 account/cycle/version 변경 후 stale 선택·draft를 유지하지 않는다.
- [ ] **공통 프레임 구현:** 자연물과 구매물의 제목·설명/상태·미리보기·닫기 및 행동 영역을 같은 위치와 규격으로 배치한다. 미리보기 칸은 동일한 `144px` 크기의 반응형 정사각형을 공유한다. 자연물에는 기존 시대·설명·조건부 `자연물 제거`, 구매물에는 기존 미배치/draft/배치 완료 행동을 보여준다. 배치 완료 구매물에만 `위치 이동`·`보관함으로`를 표시한다.
- [ ] **회귀 보강:** pending 선택 차단, draft 취소/확정, 오류, Enter/Escape, 자연물 제거, 구매물 retrieve, 미배치 구매 선택, 대상/context 소멸을 기존 테스트와 함께 확인한다.
- [ ] **통과 확인:** `npm test -- src/components/__tests__/PlanetLandscape.test.tsx`를 실행한다.

## 작업 3: 사용자 확대·축소 및 전체 보기 제거

**파일:** `planetLandscapeCamera.ts`, `PlanetLandscape.tsx`, 두 파일의 관련 테스트.

- [ ] **실패 테스트 작성:** `focusLandscape`는 선택 대상을 향해 중심만 이동하고 기존 `camera.zoom`을 보존하는지 확인한다. 풍경 UI에서 `확대`, `축소`, `전체 보기` 버튼은 없고, 화살표 키와 드래그 패닝은 중심을 바꾸는지 확인한다. 장식 미리보기 focus도 배율을 바꾸지 않아야 한다.
- [ ] **RED 확인:** `npm test -- src/components/__tests__/planetLandscapeCamera.test.ts src/components/__tests__/PlanetLandscape.test.tsx`를 실행한다.
- [ ] **최소 구현:** 풍경 내 확대/축소/전체 보기 버튼과 전용 CSS/import를 제거한다. `focusLandscape`는 현재 배율을 보존하도록 수정한다. 초기 프레이밍, 화면 크기 변경 시 clamp, 포인터 좌표 변환, 드래그/방향키 패닝은 유지한다. `zoomLandscape`는 제품 코드 참조를 확인하고 미사용이면 제거한다.
- [ ] **통과 확인:** 같은 명령으로 focus 배율 보존, 패닝, viewport resize, reduced motion 카메라 동작을 확인한다.

## 작업 4: 시대별 픽셀 배경·바닥 보강과 통합 검증

**파일:** `PlanetLandscapeDecorations.tsx`, `App.css`, `PlanetLandscape.test.tsx`.

**인터페이스:** 기존 `PlanetLandscapeDecorations` props를 유지한다. 배경 그룹은 `aria-hidden="true"`, `pointerEvents="none"`으로 오브젝트 선택 입력을 통과시킨다.

- [ ] **실패 테스트 작성:** 다섯 시대에서 새 배경층·지면층이 렌더링되는지, 같은 world 셀의 무늬가 패닝과 재렌더 뒤에도 고정되는지 검증한다. 보이는 화면과 가장자리 여유 셀만 렌더링하여 bounds 높이만 커졌을 때 무늬 노드 수가 지형 전체 크기에 따라 불어나지 않는지도 확인한다.
- [ ] **RED 확인:** `npm test -- src/components/__tests__/PlanetLandscape.test.tsx`를 실행한다.
- [ ] **최소 구현:** 기존 시대별 팔레트를 바탕으로 하늘·원경·중경·가까운 지형의 계단형 실루엣과 명도 차를 보강한다. 바닥은 잔디 윗면, 경계, 흙/암석 단면을 나누고 작은 결정적 클러스터로 풀·돌·흙 질감을 표시한다. X/Y world 셀에서 재현 가능한 패턴만 계산해 렌더마다 `Math.random()`을 쓰지 않는다. 오브젝트 클릭 영역과 구매 sky 장식의 대비를 유지한다.
- [ ] **국소 통과 확인:** 같은 PlanetLandscape 테스트에서 배경 안정성, 선택 hit target, 구매 장식 및 기존 decoration 테스트가 통과하는지 확인한다.
- [ ] **전체 테스트/빌드:** `apps/desktop`에서 `npm test`와 `npm run build`를 실행한다. 회귀 실패를 원인별로 분류하고 미해결 실패가 있으면 수용 전에 해결한다.
- [ ] **화면 확인:** 가능한 승인된 실행 환경에서 다섯 시대, 빈 행성, 겹친 오브젝트, 기본/구매 카드 전환, 좁은 화면, 수직·대각선 산책, 패닝을 확인한다. 실제 화면을 확인하지 못하면 DOM·단위 검사만으로 시각 레이아웃이나 hit-testing이 입증됐다고 보고하지 않는다.

## 순서와 수용

**의존 순서:** 작업 1 → 작업 2 → 작업 3 → 작업 4. `PlanetLandscape.tsx`와 그 테스트를 여러 단계에서 만지므로 Coder가 순차 반영한다. 전체 검증은 QA가 독립적으로 수행한다. 자연물 제거, 기존 배치·저장 상태 보호, 초기 프레이밍 및 패닝 회귀가 해결되고 모든 테스트/빌드 결과가 확인된 뒤 CEO가 수용한다.

구현 commit은 작업 중간에 만들지 않는다. QA와 수용 완료 후 CEO가 승인 범위의 변경만 모아 하나의 선택적 Conventional Commit으로 기록할 수 있다.

## 자체 검토

- **설계 범위:** 캐릭터 이동, 선택 카드, 풍경, zoom controls 제거가 모두 작업 1–4에 대응한다.
- **경계 사례:** 좁은 지형·난수 극값·이전 타이머·pending purchase·stale context·질감 비용을 검토 초점과 테스트에 연결했다.
- **기존 동작:** 자연물 제거, 미배치 구매물 및 draft 동작, 패닝, reduced motion과 초기 프레이밍을 보존한다.
- **최소 변경:** API/DB/Rust 변경과 신규 자산/의존성을 제외하고 기존 SVG를 확장한다.
- **조사 한계:** 픽셀 배경 원칙은 공개 작가 튜토리얼을 근거로 한 설계 가이드다. 특정 작품이나 타일 자산은 복제하지 않으며 실제 미관은 화면 확인으로 판단한다.
