# 지형 전체 아바타 이동과 장식 겹침 구현 계획

**상태:** 사용자 지시에 따라 확정

**목표:** 전체 지면을 훑는 아바타 경로를 만들고, 통행 셀과 겹치는 상점 장식도 preview·저장 모두 허용한다. 아바타는 장식보다 앞에 표시하고 장식 입력은 가로채지 않는다.

**설계:** `docs/superpowers/specs/2026-10-07-planet-shop-walkthrough-layering-design.md`

## 제약

- 구현·테스트 파일 수정은 Coder가 맡고 CEO는 조정·수용한다.
- 회귀 테스트를 먼저 추가하고 기존 실패를 확인한 뒤 구현한다. 테스트 작성·실행은 사용자 승인에 포함되어 있다.
- 아바타 경로·TypeScript 미리보기·Rust 저장 검증을 모두 변경한다.
- 자연물 레이아웃 예약 셀, 전체 footprint bounds 검사, sky 규칙, 상점 transport 및 DB 계약은 유지한다.
- `AvatarSprite`의 현재 CSS 표시 크기 20×25와 부모 scale 1.2에서 논리 footprint 24×30을 유지한다.
- 일정한 지그재그 경로의 끝점에서만 방향을 반전해 모든 점을 순서대로 방문한다. terrain 변경 시 구 경로 타이머를 정리한다.

## 파일별 책임

| 파일 | 책임 |
|---|---|
| `apps/desktop/src/components/landscapeAvatarRoute.ts` (신규) | bounds 전체를 덮는 경로 생성 및 점수·진행 방향 계산 |
| `apps/desktop/src/components/__tests__/landscapeAvatarRoute.test.ts` (신규) | footprint 경계, 지형 크기/원점, 간격, serpentine 연결, 왕복 진행 |
| `apps/desktop/src/components/PlanetLandscape.tsx` | 동적 경로 진행·timer lifecycle, 기존 경로 제한 제거, avatar foreground와 pointer pass-through |
| `apps/desktop/src/components/landscapeEditing.ts` | 상점 장식 통행 셀 거절 제거, zone/bounds 검사 유지 |
| `apps/desktop/src/components/__tests__/landscapeEditing.test.ts` | 경로와 예약 셀 위치 허용 및 지형 경계 회귀 |
| `apps/desktop/src/components/__tests__/PlanetLandscape.test.tsx` | 경로 위 draft·확정과 avatar layer/pointer 속성 회귀 |
| `apps/desktop/src-tauri/src/domain/landscape_geometry.rs` | 저장 단계 통행 셀 검사 제거, 자연물 layout reservation 유지 |
| `apps/desktop/src-tauri/src/storage/cosmetic_shop.rs` | 경로와 겹치는 좌표의 저장 성공 통합 검증 |

## Task 1: 전체 지면 경로 기하와 진행 규칙

1. 순수 함수 테스트를 먼저 작성한다. 기본 terrain, 원점 이동, 큰/비정수 bounds, 24×30 최소 크기, 무효 bounds를 포함한다. 점 footprint는 지형 안에 있고, 인접점은 직교 연결이며, 가로 간격 ≤48·행 간격 ≤30, 시작·끝·모든 행과 모서리를 포함해야 한다.
2. `advanceLandscapeAvatarRoute` 진행 테스트를 추가한다. 경로 끝에서만 방향이 반전되고 모든 점이 순회되며 첫 점으로 순간 이동하지 않아야 한다.
3. `landscapeAvatarRoute.ts`를 구현한다. 경로는 `x=terrain.x…right−24`, `y=terrain.y…bottom−30`에서 시작·끝을 포함한 균일 정지점을 사용한다. 짝수 행은 왼쪽→오른쪽, 홀수 행은 오른쪽→왼쪽으로 이어 점 간격 제한을 지킨다.
4. 대상 Vitest로 경로 helper를 검증한다.

## Task 2: PlanetLandscape 아바타 lifecycle과 레이어

1. timer 테스트에서 일정한 진행 방향과 endpoint reversal을 확인한다. 위치·걷기 상태·facing·비활성/reduced motion/unmount cleanup을 검증한다.
2. 기존 고정 U자 경로와 매 구간 무작위 방향 선택을 route helper와 방향 ref 기반 진행으로 바꾼다. 매 걷기 묶음은 기존처럼 1–5개의 연속점을 사용할 수 있지만 경로 끝에서만 방향을 바꾼다.
3. terrain bounds 변경 때 이전 movement/blink 타이머를 정리하고, 현재 avatar 좌표에서 새 경로의 가장 가까운 점으로 상태를 맞춘다. 경로가 비정상 또는 한 점이면 timer를 반복 예약하지 않는다.
4. 아바타 그룹을 자연물·상점 장식·draft 뒤에 둔 SVG 순서를 유지하고 `pointerEvents="none"`을 설정한다. DOM 순서와 포인터 속성 회귀를 추가한다.

## Task 3: 배치 판정과 frontend 회귀

1. TypeScript 배치 테스트에서 옛 통행 fixture `(160,140)`, `(1100,200)`, `(50,200)`, `(1300,200)`이 허용되어야 함을 먼저 추가하고 기존 실패를 확인한다. 영역 밖, invalid input, sky 규칙은 유지한다.
2. `landscapeEditing.ts`에서 walkway 셀 상수·교차 검사를 제거하고 `reserved_walkway` 실패 유형을 없앤다. 풍경 안내에서 통행 오류 문구를 제거한다.
3. scene 테스트에서 경로 위 preview가 유효하고, 확정 시 좌표가 정확하며 action이 한 번 호출되는지 확인한다. SVG에서 아바타가 자연물·shop object·draft 뒤에 있고 pointer-events가 none인지 확인한다.
4. 대상 `landscapeEditing` 및 `PlanetLandscape` 테스트를 실행한다.

## Task 4: Rust 저장 판정 동기화

1. Rust geometry와 storage 테스트에서 TypeScript와 동일한 경로 좌표 허용 fixture를 추가하고 기존 invalid placement 응답을 재현한다.
2. Rust `validate_placement`에서 통행 충돌 helper 호출을 제거한다. `reserved_cell`은 자연물 `terrain_bounds` 계산에서 유지한다.
3. 구매 장식을 실제 경로와 겹치는 지면 좌표에 배치하고 status, 저장 좌표, version을 검증한다.
4. geometry와 shop storage 대상 Cargo 테스트를 실행한다.

## Task 5: 전체 검증과 독립 수용

1. `apps/desktop`에서 전체 `npm test`와 `npm run build`, 저장소에서 Tauri 전체 `cargo test`를 실행한다.
2. 독립 QA가 경로 coverage, 수명 경계, 통행 fixture, 저장 동기화, UI layer/input 수용 기준을 확인한다.
3. Reviewer가 route progression, bounds 변경 시 stale timer, frontend/backend 계약 및 렌더 순서를 독립 검토한다.
4. 실행 가능한 desktop이 있으면 아바타가 상점 장식 앞을 지나고 겹친 장식 클릭이 작동하는지 확인한다. 불가하면 시각/hit-test 검증 경계로 보고한다.

## 의존 관계

`Task 1 → Task 2 → Task 3 → Task 4 → Task 5` 순으로 진행한다. 경로 기하와 진행 규칙을 확정한 뒤 component lifecycle을 연결하고, 상점 preview와 Rust 저장 판정을 함께 완화한다.
