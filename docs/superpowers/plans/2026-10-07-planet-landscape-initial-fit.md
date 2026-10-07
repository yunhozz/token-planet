# 행성 상세 풍경 기본 전체 보기 구현 계획

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking. Yunho Harness assigns implementation to a Coder and keeps coordination and acceptance with the CEO.

**Goal:** 행성 상세에 처음 들어가거나 행성 컨텍스트가 바뀌면 풍경 전체가 기본 화면에 보이게 합니다.

**Architecture:** `initialLandscapeCamera(bounds)`가 기존 `fitLandscape(bounds)` 결과를 사용해 경계 중앙과 `zoom: 1`을 반환합니다. 현재 화면 크기에 맞춘 `landscapeViewBox` 계산은 그대로 두며, 같은 컨텍스트에서 저장된 탐사 상태와 선택 상태도 기존 App 흐름대로 유지합니다.

**Tech Stack:** React 19, TypeScript, Vitest, React Testing Library, npm.

**Spec:** 별도 문서 사양은 없습니다. 2026-10-07에 승인된 대화형 설계: 최초 진입과 계정·월드·사이클 컨텍스트 초기화는 전체 보기로 시작하고, 같은 컨텍스트의 탐사 상태는 보존합니다.

## 공통 제약

- `initialLandscapeCamera(bounds: LandscapeBounds): LandscapeCamera`의 인터페이스를 유지합니다.
- 전체 보기 경계에는 기존 `planetLandscapeBounds`의 하늘 영역을 포함합니다.
- 새 확대 UI, 상세 재진입마다 강제 초기화, 탐사 상태 구조 변경은 범위에 포함하지 않습니다.
- CEO는 구현 파일을 직접 수정하지 않고 지정 Coder가 구현 및 테스트 코드를 수정합니다.

## 검토 초점

1. 높이가 큰 풍경도 기본 확대율로 경계 전체가 보이는지 확인합니다. 카메라 단위 테스트에서 다룹니다.
2. 넓은 화면, 정사각형 화면, 좁은 화면에서도 풍경 경계를 포함하고 비율을 유지하는지 확인합니다. 카메라 단위 테스트에서 다룹니다.
3. 비어 있거나 유효하지 않은 풍경 경계에서도 초기 카메라가 유한한 중심과 `zoom: 1`을 반환하는지 확인합니다. 카메라 단위 테스트에서 다룹니다.
4. 행성 상세 첫 진입과 컨텍스트 초기화에서 전체 보기가 적용되는지 확인합니다. App 회귀 테스트에서 다룹니다.
5. 같은 컨텍스트의 상점 왕복에서 선택 상태와 카메라 상태가 보존되고, 초기 배율 1의 이동·리사이즈 제한이 유지되는지 확인합니다. App 및 기존 풍경 컴포넌트 테스트에서 다룹니다.

---

### 작업 1: 초기 카메라를 전체 보기로 변경하고 회귀 기대값 정렬

**수정 파일:**

- `apps/desktop/src/components/planetLandscapeCamera.ts`
- `apps/desktop/src/components/__tests__/planetLandscapeCamera.test.ts`
- `apps/desktop/src/__tests__/App.test.tsx`

**검증용 기존 파일:**

- `apps/desktop/src/components/__tests__/PlanetLandscape.test.tsx`
- `apps/desktop/src/components/PlanetLandscape.tsx`
- `apps/desktop/src/App.tsx`

**인터페이스:**

- 입력: `initialLandscapeCamera(bounds: LandscapeBounds): LandscapeCamera`
- 재사용 함수: `fitLandscape(bounds: LandscapeBounds): LandscapeCamera`
- 화면 경계 계산: `landscapeViewBox(bounds, viewport, camera): LandscapeBounds`
- 결과: 정규화된 경계 중앙 및 `zoom: 1`; App의 기존 호출부와 컨텍스트 상태 흐름을 유지합니다.

- [ ] **1단계: 큰 풍경을 대상으로 실패하는 카메라 테스트 작성**

  `planetLandscapeCamera.test.ts`에서 `initialLandscapeCamera`를 가져옵니다. 풍경 경계 `{ x: 0, y: -220, width: 2400, height: 820 }`에 대해 카메라가 `{ centerX: 1200, centerY: 190, zoom: 1 }`인지 확인합니다. 다음 화면 크기 각각에 `landscapeViewBox`를 계산해 상하좌우가 풍경 경계를 모두 포함하고, 뷰박스와 화면의 종횡비가 일치하는지 확인합니다.

  - `{ width: 1200, height: 420 }`
  - `{ width: 600, height: 600 }`
  - `{ width: 360, height: 500 }`

  추가로 유효하지 않은 경계 `{ x: NaN, y: Infinity, width: 0, height: NaN }`에서 `{ centerX: 300, centerY: 160, zoom: 1 }`을 확인합니다.

- [ ] **2단계: 카메라 테스트가 현재 구현에서 실패하는지 확인**

  실행 위치: `apps/desktop`

  ```bash
  npm test -- src/components/__tests__/planetLandscapeCamera.test.ts
  ```

  예상 결과: 새 큰 풍경 테스트가 기존 초기 확대율과 경계 잘림으로 실패합니다. 유효하지 않은 경계 테스트가 실패하지 않아도 괜찮으며, 환경 오류나 무관한 실패는 별도로 보고합니다.

- [ ] **3단계: App 진입 및 컨텍스트 회귀 기대값 작성**

  `App.test.tsx`에서 `planetLandscapeBounds`를 가져와 `opens the cosmetic shop in the current window and restores exploration on return` 테스트의 스냅샷 오브젝트로 경계를 계산합니다. 상세 진입 직후 SVG `viewBox`의 네 값이 풍경 경계 전체를 포함하고 `data-camera-zoom`이 `1`인지 확인합니다. 실제 상세 화면 렌더링에서 카메라 기본값이 전달되는지 확인합니다.

  초기 `ArrowDown` 이동 후 뷰박스가 바뀐다고 가정하는 다음 회귀 사례는 전체 보기에서는 이동 가능한 여유가 없다는 기대에 맞춥니다. 진입·컨텍스트 초기화 뒤 배율이 `1`인지, 같은 컨텍스트 상점 왕복 뒤 선택 상태와 뷰박스가 유지되는지는 계속 확인합니다. 이 테스트는 전체 보기 배율에서 상태 왕복을 확인하며, 확대된 화면의 이동은 기존 컴포넌트 테스트가 담당합니다.

  - `opens the cosmetic shop in the current window and restores exploration on return`
  - `resets exploration and keeps the shop open when %s changes`
  - `clears exploration immediately while a world context is locked`

- [ ] **4단계: App 회귀 테스트가 구현 전에 실패하는지 확인**

  실행 위치: `apps/desktop`

  ```bash
  npm test -- src/components/__tests__/planetLandscapeCamera.test.ts src/__tests__/App.test.tsx
  ```

  예상 결과: 초기 전체 보기 계약을 아직 만족하지 않아 새 카메라 및 App 기대값 중 적어도 하나가 실패합니다. RED 결과를 확인하고 실패가 구현 부재 때문인지 구분합니다.

- [ ] **5단계: 초기 카메라 구현**

  `planetLandscapeCamera.ts`에서 `initialLandscapeCamera(bounds)`가 `fitLandscape(bounds)`를 반환하도록 변경합니다. 기존 `readableViewHeight = 500`, 중심 Y 제한 `185`, 초기 확대 계산은 제거합니다. `App.tsx`와 `PlanetLandscape.tsx`의 상태 저장, 입력 이벤트, 리사이즈 로직은 바꾸지 않습니다.

- [ ] **6단계: 관련 회귀 테스트 통과 확인**

  실행 위치: `apps/desktop`

  ```bash
  npm test -- src/components/__tests__/planetLandscapeCamera.test.ts src/components/__tests__/PlanetLandscape.test.tsx src/__tests__/App.test.tsx
  npm test
  ```

  예상 결과: 세 대상 파일과 전체 Vitest 스위트가 통과합니다. 특히 화면 크기별 전체 경계 포함, 빈 행성, 컨텍스트 초기화·잠금, 상점 왕복 선택 보존, `initialZoom={2}`에서의 기존 포인터·키보드 이동 및 리사이즈 제한을 확인합니다. 테스트 변경이 기존 선택·컨텍스트 검증을 제거하지 않았는지 diff도 확인합니다.

## 계획 자가검토

승인된 설계의 초기 전체 보기, 화면 비율별 경계 포함, 컨텍스트 변경 때의 초기화, 같은 컨텍스트 상태 보존을 모두 작업 1에 연결했습니다. 실패 테스트는 현재 초기 확대 정책으로 인해 큰 풍경의 전체 경계를 포함하지 못하는 사례를 직접 재현합니다. App 테스트는 전체 보기 기본값을 확인하면서 선택·컨텍스트 보존 검증을 남깁니다. 인터페이스와 타입은 기존 코드와 맞으며, 구현은 기존 `fitLandscape` 재사용 한 곳으로 한정됩니다.
