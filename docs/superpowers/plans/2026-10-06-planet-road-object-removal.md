# 행성 화면 도로·고립 오브젝트 제거 계획

## 목표

모든 행성 화면에서 기본 시대별 도로와 성장 오브젝트 `road`, `path`, `rail`, `fern`을 표시하지 않는다. 기존 배경·버튼·토큰 UI 개선은 유지한다.

## 범위와 보존 조건

- 기본 도로, 지선, 차선을 전 시대 풍경 장식에서 제거한다. 자연 하천 등 도로가 아닌 풍경은 보존한다.
- 네 오브젝트 종류를 상세·홈·그룹 화면의 렌더링, hit area/선택, 개수 배지, 신규 오브젝트 애니메이션과 대화에서 제외한다.
- 원본 오브젝트를 사용해 layout과 bounds를 계산한 뒤 표시만 필터링한다. 다른 오브젝트의 좌표, 지형 크기, 카메라는 유지한다.
- 숨김 오브젝트가 선택된 상태라면 선택 ID만 정리하고 카메라를 바꾸지 않는다.
- Rust 및 서버의 canonical 생성, SQLite·snapshot 저장, 성장 수치와 기존 변경을 건드리지 않는다.
- 현재 `feat/planet-detail-visual-refresh` 브랜치에서 작업한다. 새 브랜치를 만들거나 전환하지 않는다.

## 구현과 검증 순서

### 1. 상세 풍경

1. `PlanetLandscape.test.tsx`에 제외 대상이 표시·선택되지 않고 다른 오브젝트 좌표와 viewBox가 유지되는 테스트를 추가한다.
2. 네 종류가 선택된 상태에서 선택만 정리되고 카메라는 유지되는 테스트를 추가한다.
3. 시대별 기본 도로가 모두 제거되고 비도로 배경 장식이 유지되는 테스트를 추가한다.
4. 신규 테스트가 현재 구현에서 실패하는 것을 확인한다.
5. 공통 표시 정책을 추가하고 `PlanetLandscape.tsx`에서 원본 layout 이후 렌더·선택 후보를 필터링한다. `PlanetLandscapeDecorations.tsx`의 도로 장식을 제거한다.

### 2. 홈·그룹 장면

1. `PlanetScene.test.tsx`에 네 종류가 그림과 overflow 개수에서 제외되는 테스트를 추가한다.
2. 숨김 오브젝트 추가로 애니메이션이나 새 오브젝트 대화가 발생하지 않는 테스트를 추가한다.
3. 테스트 실패를 확인한 뒤 공통 표시 정책을 tile 및 신규 오브젝트 대화 후보에 적용한다.

### 3. 통합 확인

- 상세 풍경, 장면, 카메라·배치·선택 관련 집중 테스트를 실행한다.
- `npm run build`로 타입 검사와 번들을 확인한다.
- `npm run dev:hosted`를 실행해 상세·홈·그룹 화면에서 도로와 네 오브젝트가 보이지 않는지 확인한다. 줌·이동·다른 오브젝트 선택도 검수한다.
- 독립 QA가 표시·선택·개수·대화 차단과 좌표 보존을 확인하고 Reviewer가 저장·생성 경로 변경 부재를 검토한다.

## 예상 파일

- `apps/desktop/src/components/planetObjectVisibility.ts` (공통 표시 정책)
- `apps/desktop/src/components/PlanetLandscape.tsx`
- `apps/desktop/src/components/PlanetLandscapeDecorations.tsx`
- `apps/desktop/src/components/PlanetScene.tsx`
- `apps/desktop/src/components/__tests__/PlanetLandscape.test.tsx`
- `apps/desktop/src/components/__tests__/PlanetScene.test.tsx`

`PlanetObjectSprite.tsx`의 기존 그림 개선은 유지하며, 화면 표시 정책으로 해당 종류의 사용을 차단한다.
