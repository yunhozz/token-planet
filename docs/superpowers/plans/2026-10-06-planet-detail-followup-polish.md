# 행성 상세 후속 UI 다듬기 Implementation Plan

> **For agentic workers:** 승인된 범위에 따라 CEO가 Coder 작업을 조정하고, 독립 QA 뒤 CEO가 수용한다. 체크박스로 각 결과를 추적한다.

**Goal (목표):** 상세 화면과 연결 화면의 버튼을 정돈하고, 성장 일지 외 토큰을 축약 표기하며, 행성 풍경·도로·자연 오브젝트를 개선하고 hosted 앱에서 실제 화면을 확인한다.

**Architecture (구조):** 기존 React 화면과 SVG 좌표 체계를 유지한다. 토큰 전용 공통 formatter와 표시 컴포넌트를 만들어 금액 표시에 연결하고, 성장 일지의 기존 정확한 숫자 표기는 유지한다. 버튼은 상세 화면과 feature 화면의 각 앱 루트 아래에서 테마를 맞춘다. 풍경과 오브젝트 그림은 기존 장식·스프라이트를 보완한다.

**Tech Stack (기술):** React 19, TypeScript, CSS, SVG, Vitest, Testing Library, Tauri 2. 새 의존성은 추가하지 않는다.

**Spec (설계):** 이 대화에서 승인된 bounded 디자인.

**작업 공간:** /Users/yunho/Desktop/project/token-planet
**실행 경로:** /Users/yunho/Desktop/project/token-planet/apps/desktop

## Global Constraints (공통 제약)

- K, M, B, T는 각각 10³, 10⁶, 10⁹, 10¹² 단위다. 최대 소수점 두 자리까지 표시하고 뒤의 0은 생략한다.
- 반올림 결과가 1000K, 1000M, 1000B가 되면 다음 단위로 승격한다.
- 성장 일지의 토큰 수치는 기존 쉼표 표기를 유지한다. 성장 점수, 크레딧, 퍼센트, 날짜는 토큰 formatter에 연결하지 않는다.
- 계산, 할인, 잔액 검사, 구매·제거 요청은 원본 수치로 처리한다.
- 버튼 handler, semantic button, current/selected/pressed/disabled 및 loading 상태를 유지한다.
- 버튼의 최소 조작 높이는 44px, 나란히 놓인 버튼 사이 간격은 8px 기준을 유지한다.
- 관측소 테마 색상과 기존 시대별 풍경 팔레트를 유지한다.
- SVG 수정은 kind, seed, 좌표, footprint, hit area, 카메라, 저장 데이터, 자연물 제거 의미를 바꾸지 않는다.
- hosted UI 검수는 읽기 전용 화면 이동과 관찰만 수행한다. 구매, 장착, 제거, 행성 초기화 및 서버 데이터 변경은 하지 않는다.

## Review Focus (검토 초점)

| 조건 | 기대 결과 | 계획된 검증 |
|---|---|---|
| 단위 경계와 큰 값 | 최대 두 자리, 끝자리 0 생략, 1000K 방지 | 작업 1 테스트 |
| 음수와 비유한 숫자 | 부호 유지, NaN/Infinity는 안전한 대시 표기 | 작업 1 테스트 |
| 화면별 숫자 종류 혼재 | 토큰만 축약하고 성장 점수·퍼센트·일지는 유지 | 작업 2 테스트 |
| 축약 가격과 원본 요청 금액 | 표시만 축약하고 quote·구매·잔액 계산은 정확 | 작업 2 테스트 |
| 좁은 화면·포커스·장면 상호작용 | 버튼 잘림 없이 접근 가능하고 오브젝트 hit area 유지 | 작업 5 실제 화면 및 기존 회귀 |

---

## 파일 책임

| 파일 | 책임 |
|---|---|
| 새 apps/desktop/src/lib/tokenFormatting.ts | formatTokenAmount(value: number): string |
| 새 apps/desktop/src/components/FormattedTokens.tsx | 토큰 값을 공통 규칙으로 렌더링 |
| 새 apps/desktop/src/components/__tests__/FormattedTokens.test.tsx | 토큰 표시 컴포넌트 렌더링 테스트 |
| 새 apps/desktop/src/lib/__tests__/tokenFormatting.test.ts | 단위·반올림·경계 테스트 |
| apps/desktop/src/App.tsx | 이번 행성·누적 토큰·지갑·상점 보상 표시 |
| apps/desktop/src/components/UsageSummary.tsx, SourceStatus.tsx, WorldCommunity.tsx | 사용량·소스별·그룹 토큰 표시 |
| apps/desktop/src/components/ShopPanel.tsx, CosmeticShop.tsx, NaturalRemovalDialog.tsx | 가격·잔액·보상·결과 문구 |
| apps/desktop/src/App.css | 상세 및 feature 화면 버튼 스타일 |
| apps/desktop/src/components/PlanetLandscapeDecorations.tsx | 하늘·원경·지면·기본 도로 |
| apps/desktop/src/components/PlanetObjectSprite.tsx | fern/path/road 스프라이트 |
| 관련 기존 테스트 파일 | 표시·금액 계산·기존 상호작용 회귀 |

## 작업 1: 공통 토큰 formatter

**생산 인터페이스**

- formatTokenAmount(value: number): string — 단위가 붙은 토큰 숫자 문자열을 반환한다.
- FormattedTokens({ value }: { value: number }) — 기존 호출부가 토큰 단어를 붙일 수 있도록 숫자 부분만 렌더링한다.

- [ ] 먼저 tokenFormatting.test.ts에 formats_token_units_with_trimmed_two_decimals 테스트를 작성한다. 0→0, 999→999, 1000→1K, 13200→13.2K, 239824→239.82K, 999994→999.99K, 999999→1M, 1000000→1M, 1000000000→1B, 1000000000000→1T를 확인한다.
- [ ] FormattedTokens.test.tsx에 renders_compact_token_amount 테스트를 추가해 239824가 화면에 239.82K로 렌더되는지 확인한다.
- [ ] promotes_each_rounded_unit_boundary 테스트에서 999999999→1B와 999999999999→1T를 확인한다.
- [ ] handles_negative_and_non_finite_values_safely 테스트에서 -13200→-13.2K, NaN/Infinity/-Infinity→—를 확인한다. supported_T_unit_keeps_scaling 테스트에서 7400000000000000→7400T를 확인한다.
- [ ] apps/desktop에서 npm test -- src/lib/__tests__/tokenFormatting.test.ts src/components/__tests__/FormattedTokens.test.tsx를 실행해 구현 전 신규 테스트가 기대대로 실패하는지 확인한다.
- [ ] tokenFormatting.ts와 FormattedTokens.tsx를 구현한다. 반올림 후 단위 값이 1000이 되면 다음 단위로 승격하고, 후행 0은 제거한다.
- [ ] npm test -- src/lib/__tests__/tokenFormatting.test.ts src/components/__tests__/FormattedTokens.test.tsx src/components/__tests__/FormattedNumber.test.tsx를 실행해 신규 formatter와 정확한 기존 숫자 렌더링이 함께 통과하는지 확인한다.

## 작업 2: 성장 일지 외 토큰 사용처 연결

**소비 인터페이스:** 작업 1의 formatTokenAmount와 FormattedTokens.

- [ ] 기존 테스트에 실패 우선 사례를 추가한다. App에서는 이번 행성 13200→13.2K, 누적 239824→239.82K, 사용량 1000000→1M과 성장 점수의 정확한 표현을 확인한다.
- [ ] ShopPanel에서 239824가 239.82K로 보이더라도 구매 요청의 quote 가격은 239824인 것을 검증하는 실패 우선 테스트를 추가한다.
- [ ] CosmeticShop의 성공·잔액 부족 문구가 축약되고, NaturalRemovalDialog의 239823 잔액으로 239824 제거 비용을 결제할 수 없는 상태가 유지되는 실패 우선 테스트를 추가한다.
- [ ] 구현 전에 관련 테스트를 실행해 새 표시 기대값이 기존 코드에서 실패하는지 확인한다.
- [ ] App, UsageSummary, SourceStatus, WorldCommunity, ShopPanel, CosmeticShop, NaturalRemovalDialog의 토큰 JSX와 문자열 메시지를 공통 formatter에 연결한다. 기존 숫자 계산과 요청 payload는 포맷된 문자열을 사용하지 않는다.
- [ ] 성장 일지 내 지갑 적립과 일별·서비스별 숫자의 쉼표 표기, 성장 점수 및 퍼센트 표시는 변경하지 않는다. 기존 테스트 기대값은 이 범위에 맞게만 갱신한다.
- [ ] 관련 테스트를 실행한다: App.test.tsx, GrowthJournal.test.tsx, ShopPanel.test.tsx, CosmeticShop.test.tsx, NaturalRemovalDialog.test.tsx, SharingPanels.test.tsx, FormattedNumber.test.tsx, tokenFormatting.test.ts.
- [ ] 남은 toLocaleString, FormattedNumber, formatTokens 사용처를 검색해 토큰 표시 누락과 비토큰 숫자 변경이 없는지 점검한다.

## 작업 3: 상세·feature 화면 버튼 스타일 통일

**스타일 범위:** .app-shell--detail과 .app-shell--feature-screen. 상점·성장 일지는 feature-screen 루트를 사용하므로 두 루트를 함께 다룬다.

- [ ] App.tsx와 App.css의 상단 도구, 상세 복귀, feature 복귀, 상점 카테고리·동작, 일지 pager, 상세 액션과 확인 버튼을 확인한다.
- [ ] CSS 수정 전에 화면별 기존 selected/current, disabled, pending, 위험 상태 및 좁은 창 표시를 검수 체크로 기록한다.
- [ ] 관측소 테마에 맞게 네모 버튼을 둥근 모서리, 얇은 테두리와 적절한 표면으로 조정한다. 주요 동작은 mint, 보조 동작은 조용한 표면, 위험 동작은 기존 coral 의미를 유지한다.
- [ ] 링크형·투명 아이콘 버튼과 이미 정돈된 탭은 성격에 맞는 형태를 유지한다. 전 버튼에 동일한 채움색을 적용하지 않는다.
- [ ] 44px 조작 높이와 8px 간격을 유지하고 hover 없이도 current/selected/pending/disabled 상태를 알아볼 수 있게 한다. 키보드 포커스 링을 보존한다.
- [ ] App의 상점·일지 이동·복귀 테스트와 NaturalRemovalDialog의 확인·pending·Escape·focus 회귀를 실행한다. 시각 평가는 작업 5에서 수행한다.

## 작업 4: 행성 풍경·도로·자연 오브젝트 보완

**소비 인터페이스:** PlanetLandscapeDecorations의 기존 props와 PlanetObjectSprite의 기존 props.

- [ ] PlanetLandscapeDecorations의 5개 시대 장면, 자연 시대 수면, 시대별 도로 및 기존 땅 장식을 기준으로 시각 검수 지점을 정한다.
- [ ] SVG 픽셀 스타일을 유지하면서 하늘·원경·중경·지면에 제한된 명암과 질감 층을 보강한다. 배경 세부는 성장 오브젝트와 겹쳐 시선을 빼앗지 않게 한다.
- [ ] 기본 도로의 노면·가장자리·교차부를 연결된 그림으로 정돈한다. 도시 차선이 도로 표면 안에 놓이도록 하고 지선이 끊기거나 떨어져 보이지 않게 한다.
- [ ] 반복 장식은 기존 world 좌표 기반의 결정적 배치를 유지해 카메라 pan/zoom에서 위치가 튀지 않게 한다.
- [ ] fern의 잎 실루엣과 명암, path/road의 노면·가장자리 표현을 보완한다. 실제 오브젝트의 위치·scale·kind·motion class·hit area는 보존한다.
- [ ] PlanetObjectSprite, PlanetLandscape, planetLandscapeLayout, planetLandscapeCamera, PlanetScene의 기존 테스트를 실행한다. 실제 작은 크기와 선택 확대 그림은 작업 5에서 확인한다.

## 작업 5: 독립 QA와 hosted UI 검수

**선행 조건:** 작업 1–4의 구현과 집중 검증이 완료되어야 한다.

- [ ] Coder가 apps/desktop에서 전체 npm test와 npm run build를 실행한다. 실패 결과는 수정 또는 명시적 이슈로 남긴다.
- [ ] QA가 최종 변경 상태에서 전체 npm test를 독립적으로 실행하고 token formatting, quote/request 원본 값, Growth Journal 예외, 기존 화면 이동·풍경 상호작용의 결과를 보고한다.
- [ ] CEO가 사용자 요청대로 apps/desktop에서 npm run dev:hosted를 직접 실행한다. 이 기존 명령은 .env.local을 읽고 Tauri dev를 시작한다. 설정을 임의로 바꾸거나 비밀 값을 출력하지 않는다.
- [ ] 실제 앱에서 상세 기본 화면, 상세→상점→복귀, 상세→성장 일지→복귀를 읽기 전용으로 확인한다.
- [ ] 가능한 창 크기에서 긴 버튼 문구·금액의 겹침, 잘림, 가로 넘침을 확인한다. Tab 포커스와 current/disabled 상태가 식별되는지 본다.
- [ ] 풍경의 현재 시대에서 배경·노면·fern/path/road 오브젝트를 작은 크기와 선택 확대 보기로 확인하고, 오브젝트 선택·카메라 확대/이동이 유지되는지 확인한다.
- [ ] 현재 hosted 계정 상태에서 접근할 수 없는 화면·시대는 미검증으로 기록한다. 확인 목적으로 서버 상태를 변경하지 않는다.
- [ ] QA는 전체 npm test 결과와 토큰 경계·정확한 금액 계산·성장 일지 예외·기존 화면 이동/풍경 회귀를 기준별로 보고한다.
- [ ] CEO는 실제 화면의 실행 명령, 창 크기, 읽기 전용 상호작용, 예상·관찰 결과, 접근 가능한 화면 증거와 미검증 범위를 기록한다.
- [ ] 전체 검증에서 발견한 수정은 Coder에게 맡기고 영향 테스트와 필요한 화면만 다시 확인한다. CEO가 기준별 증거로 수용 여부를 정한다.

## 의존성과 위험

- 작업 2는 작업 1의 formatter 인터페이스에 의존한다.
- 작업 3과 작업 4는 서로 독립적이며 각각 기존 컴포넌트 props와 CSS 루트 계약을 보존한다.
- 작업 5는 모든 결과를 합친 뒤 수행한다.
- 표시 단위가 다른 실제 가격을 같게 보이게 할 수 있으므로, 모든 구매·제거 판단과 요청은 원본 숫자로 검증한다.
- PlanetObjectSprite는 상세 화면 외에도 작은 미리보기 등에 쓰이므로 크기가 작을 때 식별성도 검수한다.
- hosted 실행은 사용자별 설정·계정 및 현재 시대에 제한을 받는다. 실행 실패나 접근 불가 조건은 통과로 간주하지 않는다.

## 자체 검토

- 승인된 버튼, 토큰, 풍경, hosted 실행 요구가 작업 1–5에 모두 연결되어 있다.
- token formatter의 함수 서명과 구성 요소 사용처가 명확하며, 수치 계산은 원본에서 유지한다.
- Growth Journal, 성장 점수, 퍼센트 및 날짜가 formatter 적용 범위에서 제외된다.
- 999K 경계, 큰 값, 비유한 값, 실제 구매 quote, 제거 잔액 및 화면 이동을 각각 확인한다.
- 상세 화면과 별도 feature-screen CSS 루트를 모두 포함해 버튼 스타일이 상점·일지로 누락되지 않는다.
- SVG 변경은 그림 표현에 한정하고 저장·배치·선택·카메라 동작을 재설계하지 않는다.
- 완료 판단은 자동 테스트뿐 아니라 요청된 Tauri hosted 화면의 관찰 증거와 남은 미검증 조건을 포함한다.
