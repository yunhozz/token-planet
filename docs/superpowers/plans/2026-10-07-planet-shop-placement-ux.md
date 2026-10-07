# 행성 상점 구매 후 배치 UX 구현 계획

> **작업 진행:** CEO가 Yunho Harness에 따라 승인된 단계를 순차적으로 Coder에게 맡기고, 회귀 테스트와 최종 검토를 조율합니다. 아래 단계는 완료 여부를 확인할 수 있도록 체크박스로 나눴습니다.

**목표:** 구매한 장식을 명확히 보여주고, 클릭·드래그·키보드로 위치를 고른 뒤 확정하며, 선택·보관 UI를 행성 화면과 자연스럽게 연결한다.

**구성:** 구매 인스턴스 식별과 배치 제한 사유를 순수 함수로 분리한다. `ShopPanel`은 구매 결과 카드와 정확한 선택을 맡고, `PlanetLandscape`는 위치 draft와 명시적 저장을 맡는다. `App`의 기존 선택 콜백과 계정·행성 문맥 검사를 재사용한다.

**기술:** React 19, TypeScript 6, Vitest 5, React Testing Library, Tauri 2. 저장소 기준 Node.js `22.21.0`, npm `10.9.4`.

**설계:** `docs/superpowers/specs/2026-10-07-planet-shop-placement-ux-design.md`

## Global Constraints

- 작업 루트는 `/Users/ma-24-007/Desktop/workspace/token-planet`, npm 명령 실행 위치는 `apps/desktop`이다.
- 구현·테스트 코드 수정은 Coder가 맡고 CEO는 조정·수용을 맡는다.
- 기존 `ShopRequest`, `ShopActionResult`, Rust 저장·동기화 계약을 바꾸지 않는다.
- 구매 응답의 동일 SKU 신규 인스턴스가 baseline과 비교해 정확히 하나일 때만 구매 CTA와 연결한다. 식별할 수 없으면 임의로 선택하지 않고 보유함에서 직접 선택하도록 안내한다.
- 최초 구매 요청의 baseline을 불확실한 요청의 동일 ID 재시도까지 유지한다. 재마운트 등으로 baseline이 없으면 인스턴스를 추측하지 않는다.
- 위치 선택은 draft만 변경한다. `배치 확정` 또는 Enter만 저장 요청을 보내고 `취소` 또는 Esc는 저장하지 않는다.
- 기존 배치 구역, 통행 구역 제한, 좌표 변환, request ID, `expected_version`, 계정·주기·중복 요청 검사를 보존한다.
- 사용자 승인에 따라 회귀 테스트를 먼저 작성하고 실패를 확인한 다음 구현한다. 기존 요청 계약과 Tauri 저장 코드는 변경하지 않는다.
- 새 의존성은 추가하지 않는다.

## Review Focus

1. **불확실한 구매 재시도:** 원래 baseline을 유지하고, 동일 SKU 신규 후보가 없거나 여러 개면 자동 연결하지 않는다. Task 2 테스트에서 확인한다.
2. **확대·축소와 뷰포트 크기 변화:** 미리보기 좌표와 제출 좌표가 같은 세계 좌표를 가리킨다. Task 3 테스트에서 확인한다.
3. **유효하지 않은 배치 위치:** 구역 밖, 예약 통행 구역, 유한하지 않은 좌표의 이유를 표시하고 draft를 유지한다. Task 1·3 테스트에서 확인한다.
4. **포인터 취소와 선택 해제:** 취소·Esc·pointer cancel은 place 요청을 보내지 않고 드래그 상태를 남기지 않는다. Task 3 테스트에서 확인한다.
5. **저장 중 입력, 계정·주기·버전 변경:** 중복 저장을 막고 이전 문맥의 draft를 제출하지 않는다. Task 3·4 테스트에서 확인한다.

---

## 파일별 책임

| 파일 | 변경 | 책임 |
|---|---|---|
| `apps/desktop/src/components/shopPurchaseSelection.ts` | 생성 | 요청 전 baseline과 구매 응답에서 새 인스턴스를 보수적으로 식별 |
| `apps/desktop/src/components/__tests__/shopPurchaseSelection.test.ts` | 생성 | 후보 수, 요청 ID, SKU, 계정·행성 문맥 검증 |
| `apps/desktop/src/components/landscapeEditing.ts` | 수정 | 기존 배치 판정과 실패 사유 제공 |
| `apps/desktop/src/components/__tests__/landscapeEditing.test.ts` | 수정 | 구역 경계, 입력, 통행 제한 판정 |
| `apps/desktop/src/components/ShopPanel.tsx` | 수정 | 구매 완료 카드, baseline 유지, CTA와 모호한 결과 fallback |
| `apps/desktop/src/components/__tests__/ShopPanel.test.tsx` | 수정 | 구매 표시, 정확한 선택, 불확실 재시도와 fallback |
| `apps/desktop/src/components/PlanetLandscape.tsx` | 수정 | 배치 draft 시작·이동·확정·취소와 선택 카드 |
| `apps/desktop/src/components/__tests__/PlanetLandscape.test.tsx` | 수정 | 클릭·드래그·키보드, 요청 경계, 문맥 변경 |
| `apps/desktop/src/App.css` | 수정 | 상점 구매 카드와 풍경 선택 카드의 테마·좁은 폭 레이아웃 |
| `apps/desktop/src/__tests__/App.test.tsx` | 수정 | 구매 → 선택 → 배치 → 보관 통합 회귀 |

## Task 1: 구매 인스턴스 식별과 배치 실패 사유

**파일:** 위 `shopPurchaseSelection.ts`, 해당 테스트 파일, `landscapeEditing.ts`, 해당 테스트 파일.

**인터페이스:**

```ts
export type LandscapePurchaseBaseline = Readonly<{
  requestId: string;
  accountId: string;
  cycleId: string;
  sku: string;
  instanceIds: readonly string[];
}>;

export function resolvePurchasedLandscapeInstance(
  baseline: LandscapePurchaseBaseline,
  result: ShopActionResult,
): LandscapeInstance | null;

export type PlacementFailure = "invalid_input" | "outside_zone" | "reserved_walkway";

export function placementFailure(
  product: Pick<ShopProduct, "placement_zone">,
  point: LandscapePoint,
  terrain: LandscapeBounds,
): PlacementFailure | null;
```

기존 `validatePlacement(...)` boolean 인터페이스는 유지하고 `placementFailure(...) === null`을 반환하도록 연결한다.

- [ ] **1단계: 실패하는 테스트를 작성한다.** 함수가 아직 없는 상태를 동작 실패로 확인할 수 있도록 빈 `shopPurchaseSelection.ts` 모듈을 준비한 뒤 테스트를 작성한다. baseline에 있던 ID는 제외하고 동일 SKU의 새 인스턴스가 하나일 때만 반환하는 경우를 검증한다. 후보 0개·2개, 요청 ID·SKU·계정·주기 불일치, 성공이 아닌 상태는 `null`이어야 한다. 배치 실패 유형도 테스트한다.
- [ ] **2단계: RED를 확인한다.** `apps/desktop`에서 `npm test -- src/components/__tests__/shopPurchaseSelection.test.ts src/components/__tests__/landscapeEditing.test.ts`를 실행한다. 새 동작이 아직 없어 실패해야 한다.
- [ ] **3단계: 최소 구현한다.** 요청 baseline 차이와 현재 계약의 상태 필드를 사용하고 순서·UUID·변형 번호로 신규성을 추측하지 않는다. 기존 배치 영역과 통행 제한 판정은 유지한다.
- [ ] **4단계: GREEN과 회귀를 확인한다.** 같은 대상 테스트와 전체 `npm test`를 실행한다. 기존 배치 테스트와 새 실패 사유 테스트가 모두 통과해야 한다.

## Task 2: 구매 완료 카드와 정확한 인스턴스 선택

**파일:** `ShopPanel.tsx`, `ShopPanel.test.tsx`.

**입력·출력:** Task 1의 baseline과 식별 함수를 사용한다. 기존 `onSelectLandscapeInstance(instanceId)`에는 식별된 인스턴스 ID만 전달한다. 최초 요청 전 baseline을 저장하고 동일 요청 재시도에서는 재사용한다.

- [ ] **1단계: 실패하는 테스트를 작성한다.** 구매 성공 카드에 구매한 상품명과 실제 변형 썸네일, `행성에 배치하기`, `계속 쇼핑`이 보이는지 검증한다. 배치 CTA를 눌렀을 때만 유일하게 식별된 새 ID가 callback에 전달돼야 한다.
- [ ] **2단계: RED를 확인한다.** `apps/desktop`에서 `npm test -- src/components/__tests__/ShopPanel.test.tsx`를 실행한다. 새 카드와 CTA 검증이 실패해야 한다.
- [ ] **3단계: 구매 결과 처리를 구현한다.** 최초 요청 baseline을 보존하고 구매 응답을 Task 1 함수로 판정한다. 성공 결과 카드에는 실제 인스턴스가 확인된 경우에만 그 변형 썸네일을 전달한다. 아바타 구매의 기존 동작은 유지한다.
- [ ] **4단계: 불확실 결과와 fallback 테스트를 추가해 통과시킨다.** 같은 ID로 재시도해 새 인스턴스가 하나면 연결하고, 후보가 없거나 여러 개거나 baseline이 없으면 보유함으로 이동시킨다. 계정·주기 변경 후의 늦은 응답은 구매 CTA로 노출하지 않는다. 대상 테스트와 전체 `npm test`가 통과해야 한다.

## Task 3: 명시적 배치 모드와 행성 선택 카드

**파일:** `PlanetLandscape.tsx`, `PlanetLandscape.test.tsx`.

**인터페이스:** 기존 `selectedLandscapeInstanceId`, `onSelectLandscapeInstance`, `onShopAction`을 유지한다. 배치 draft에는 계정·주기·인스턴스 ID·SKU·버전·세계 좌표·유효성이 포함된다. 미배치 장식 선택 시 draft를 시작하고, 배치된 장식은 `위치 이동`을 선택했을 때 현재 위치에서 draft를 시작한다.

- [ ] **1단계: 실패하는 테스트를 작성한다.** 선택 카드와 draft가 표시되고 유효한 위치에서 클릭하거나 드래그해도 place 요청은 아직 전송되지 않으며, `배치 확정` 또는 Enter만 좌표와 버전이 포함된 place 요청 한 건을 보내는지 검증한다.
- [ ] **2단계: RED를 확인한다.** `apps/desktop`에서 `npm test -- src/components/__tests__/PlanetLandscape.test.tsx`를 실행한다. 신규 확정 흐름 검증이 실패해야 한다.
- [ ] **3단계: draft 흐름을 구현한다.** 기존 화면-세계 좌표 변환을 재사용한다. 포인터 이동과 키보드 조정은 draft만 바꾸고, 클릭·pointer up으로 저장하지 않는다. 배치 확정 또는 Enter에서만 기존 비동기 요청 가드를 통해 저장한다.
- [ ] **4단계: 선택 카드와 취소·오류 상태 테스트를 추가한다.** 미배치/배치 상태에 썸네일, 상품명, 상태, 알맞은 조작을 보인다. 실패 위치는 원인과 함께 draft를 유지하고 확정을 막는다. 취소·Esc·pointer cancel은 저장하지 않는다. retrieve 성공 후 실제 상태를 보여준다.
- [ ] **5단계: 문맥·좌표 경계 테스트를 추가해 통과시킨다.** 확대·축소, 뷰포트 변경, 중복 확정, 요청 중 입력, 계정·주기·버전 변경, 기존 배치 이동 취소를 검증한다. 대상 테스트와 전체 `npm test`가 통과해야 한다.

## Task 4: 화면 통합과 테마

**파일:** `App.css`, `App.test.tsx`; 연결상 필요한 경우에만 `App.tsx`.

**입력·출력:** 구매 카드 CTA는 기존 선택 콜백으로 행성 화면에 복귀하고 선택한 인스턴스는 Task 3 카드에 나타난다. 기존 계정·주기 검사와 선택 초기화를 유지한다.

- [ ] **1단계: 통합 실패 테스트를 작성한다.** 상점에서 구매 성공 → `행성에 배치하기` → 정확한 인스턴스 선택 → 배치 확정 → 보관함으로 흐름이 기존 상점 action에 올바른 ID와 좌표를 보내는지 검증한다.
- [ ] **2단계: RED를 확인한다.** `apps/desktop`에서 `npm test -- src/__tests__/App.test.tsx`를 실행한다. 새 구매 CTA 통합 검증이 실패해야 한다.
- [ ] **3단계: 스타일과 기존 화면 연결을 구현한다.** 카드 배경·테두리·여백을 행성 화면에 맞추고 좁은 창에서는 콘텐츠가 줄바꿈되게 한다. 버튼은 44px 이상의 조작 영역을 유지하고 상태·불가 사유를 텍스트로 제공한다. 기존 `App.tsx`의 선택 콜백과 문맥 검사를 재사용한다.
- [ ] **4단계: 회귀를 확인한다.** 모호한 구매 결과의 보유함 fallback과 명시 선택, 계정·주기 변경, 반복 구매, 아바타와 자연물 조작의 기존 동작을 확인한다. 대상 테스트와 전체 `npm test`가 통과해야 한다.
- [ ] **5단계: 전체 자동 검증을 실행한다.** `npm test`와 `npm run build`를 `apps/desktop`에서 실행한다. Vitest 전체와 TypeScript/Vite 빌드가 통과해야 한다.

## 의존성과 순서

`Task 1 → Task 2 → Task 3 → Task 4` 순서로 진행한다. Task 2·3은 Task 1의 함수를 사용하고 Task 4는 두 흐름이 완성된 뒤 통합한다. 각 동작 변경은 테스트를 먼저 작성하고 예상된 실패를 확인한 뒤 구현한다. 전체 변경은 구매·선택·저장 수명과 통합 순서에 걸치므로 significant risk로 취급한다. 최종 자동 검증 이후 QA가 설계 수용 기준별 증거를 확인하고 Reviewer가 전체 diff를 독립 검토한 다음 CEO가 수용한다.

## 인스턴스 식별의 한계

현재 `ShopActionResult`는 새 인스턴스 ID를 별도 영수증으로 반환하지 않는다. 따라서 구매 직전 보유 ID와 성공 응답 상태의 차이를 이용하되, 동일 SKU 신규 후보가 정확히 하나일 때만 CTA와 연결한다. 불확실 재시도는 같은 request ID와 원래 baseline을 사용한다. 후보 수가 0개 또는 여러 개거나 baseline이 유실되면 보유함에서 직접 선택하도록 안내한다. 더 강한 식별 보증을 위해 응답 계약을 변경해야 한다면 구현을 중지하고 범위 변경을 CEO에 보고한다.

## 자기 검토

- 사용자 승인 설계의 구매 확인, 자연스러운 보관 UI, 쉬운 위치 선택이 Tasks 2–4에 각각 포함됐다.
- 구매 ID의 불확실성, 재시도, 모호한 결과 fallback을 acceptance와 Task 2 테스트에 연결했다.
- 위치 제한과 좌표 처리, 확정·취소·문맥 변경 경계를 Task 1·3 테스트에 연결했다.
- 기존 앱 콜백과 저장 계약을 재사용하며, 새 백엔드 계약이나 배치 규칙 변경은 계획에 포함하지 않았다.
- 좁은 창 레이아웃은 스타일을 구현하지만, 자동 테스트는 브라우저에서의 실제 시각적 배치를 증명하지 않는다. UI 실행 환경이 준비되지 않으면 해당 시각 확인은 별도 미검증으로 보고한다.
