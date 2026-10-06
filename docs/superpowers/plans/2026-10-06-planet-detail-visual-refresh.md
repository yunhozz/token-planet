# 행성 상세 화면과 픽셀 아트 개선 계획

## 목표

승인된 방향에 따라 행성 상세 화면의 시각 위계를 현대적으로 정돈하고, 풍경·아바타·오브젝트의 픽셀 그림을 더 알아보기 쉽게 세밀하게 만든다. 픽셀 스타일은 유지한다.

풍경을 우선하는 정돈된 정보 위계는 [Linear의 2026 UI 개편](https://linear.app/changelog/2026-03-12-ui-refresh), 작업 영역에 공간을 주고 도구를 모은 구성은 [Figma UI3 안내](https://www.figma.com/blog/making-the-move-to-ui3-a-guide-to-figmas-next-chapter/)를 참고한다. 반투명 소재는 풍경 위 조작부 등 기능 계층에만 제한하며, Apple의 [Materials 지침](https://developer.apple.com/design/human-interface-guidelines/materials)에 따라 장식 효과를 여러 패널에 반복하지 않는다. 이들은 참고 사례이지 제품 전반의 유행을 단정하는 기준은 아니다.

## 범위와 유지할 동작

- 상세 화면의 헤더, 탭, 풍경, 요약 정보와 보조 정보의 레이아웃·표면·색·타이포그래피를 개선한다.
- 실제 오브젝트는 현재처럼 풍경에서 선택한다. 별도 오브젝트 목록을 추가하지 않는다.
- 선택 패널과 확대 그림을 풍경 아래에 정리하되, 선택·닫기·제거 동작 및 선택에 따른 카메라 위치는 유지한다.
- 공용 아바타 스프라이트 개선은 홈·그룹·미리보기에도 반영될 수 있다.
- Tauri 창 전환, 카메라·탐사 상태, 구매·장착·배치·회수·자연물 제거, 계정 및 행성 컨텍스트 보호는 변경하지 않는다.
- API, 저장 데이터, 성장 계산, 신규 의존성은 이번 범위에서 제외한다.

## 파일 책임

| 파일 | 책임 |
|---|---|
| `apps/desktop/src/App.tsx` | 상세 화면의 기존 JSX와 정보 우선순위 정리 |
| `apps/desktop/src/App.css` | 상세 화면 전용 팔레트·표면·위계·반응형 규칙 |
| `apps/desktop/src/components/PlanetLandscape.tsx` | 풍경과 선택 정보·확대 그림의 배치 |
| `apps/desktop/src/components/PlanetLandscapeDecorations.tsx` | 시대별 배경, 원경·중경·전경 및 지면의 시각적 깊이 |
| `apps/desktop/src/components/PlanetObjectSprite.tsx` | 성장 오브젝트의 종류별 실루엣·재료·세부 묘사 |
| `apps/desktop/src/components/LandscapeObjectSprite.tsx` | 설치 장식과 상점 미리보기 그림 및 기존 footprint |
| `apps/desktop/src/components/AvatarSprite.tsx` | 아바타 기본 표정·머리·의상 그림 |
| `apps/desktop/src/components/AvatarEquipmentLayers.tsx` | 장비 슬롯의 세부 그림과 현재 레이어 순서 |

## 작업 순서

### 1. 상세 화면 레이아웃과 시각 언어

`App.tsx`, `App.css`, `PlanetLandscape.tsx`를 수정한다. 헤더와 탭을 정돈하고, 풍경을 주 콘텐츠로 유지한다. 행성 이름·시대·이번 행성 토큰·진행도를 한눈에 찾도록 묶고, 기록·사용량·상점 효과는 읽기 쉬운 보조 영역으로 정리한다. 선택 정보와 확대 그림은 풍경을 가리지 않도록 풍경 아래에 둔다. 선택 패널의 aria 정보, 닫기·Escape·제거 handler 및 pointer 이벤트 전파 차단은 보존한다.

상세 CSS에는 승인된 색 `#10151D`, `#1B2430`, `#F2F5F3`, `#A3B0BC`, `#80D6BA`, `#E8BF79`를 적용한다. 풍경 색은 시대별 팔레트를 유지한다. 본문은 시스템 산세리프, 기본 16px과 최소 44px 조작 영역을 기준으로 한다. 스타일 범위를 상세 화면에 한정해 팝오버를 바꾸지 않는다.

### 2. 아바타와 장비 그림

`AvatarSprite.tsx`, `AvatarEquipmentLayers.tsx`에서 기본 몸체와 모든 장비를 함께 조정한다. 얼굴 표정, 머리카락, 의상 구분, 손·신발과 장비 부품을 명암과 제한된 색으로 표현한다. 기존 컴포넌트 props, 외곽 크기, 장비 슬롯, 좌우 반전, 걷기·눈감기 연결은 유지한다.

### 3. 풍경과 오브젝트 그림

`PlanetObjectSprite.tsx`, `LandscapeObjectSprite.tsx`, `PlanetLandscapeDecorations.tsx`를 조정한다. 나무·바위·물·생물·건축물 등 기존 kind를 형태와 재질로 구분하고, 배경에는 원경·중경·전경의 명도 차와 정돈된 지면 세부를 더한다. 상점 장식의 배치 footprint와 확대 미리보기 표현을 맞춘다. 동일 입력은 동일한 배치와 그림을 만들고 장식은 성장 데이터 의미를 갖지 않게 한다.

### 4. 통합 확인

수용 기준에 따라 좁은 창·기본 상세 크기·긴 이름과 큰 수치·선택 패널·각 시대의 장면·여러 장비 조합을 확인한다. 키보드 조작, 최소 조작 크기, 포커스, reduced motion, 풍경 선택/줌/이동, 상점·일지 복귀 동작이 유지되는지 확인한다. 브라우저에서 본 결과와 Tauri에서 본 결과를 구분하고 수행하지 않은 환경은 미검증으로 보고한다.

## 수용 기준

1. 상세 화면에서 풍경이 시각적 중심이고 시대·토큰·진행도를 바로 찾을 수 있다.
2. 성장 오브젝트의 종류와 재료가 실제 풍경 크기와 확대 그림에서 구별된다.
3. 아바타의 표정·의상·장비를 알아볼 수 있으며 좌우 반전과 걷기에서 레이어가 어긋나지 않는다.
4. 좁은 창과 200% 텍스트 확대에서 가로 넘침, 잘림, 버튼 겹침이 없다.
5. 풍경 직접 선택, 기존 선택 정보·확대 그림, 확대·이동, 꾸미기, 제거와 복귀 동작이 유지된다. 선택 시 카메라는 자동 이동하지 않고 새 목록도 없다.
6. 기존 로딩·오류·확인 중 표시, 권한 검사, 계정·행성 컨텍스트 보호가 유지된다.
7. 키보드 포커스가 보이고, 동작 줄이기 설정에서 불필요한 애니메이션이 실행되지 않는다.

## 검증 범위

변경 전용 회귀 검토 대상으로 `apps/desktop/src/__tests__/App.test.tsx`, `components/__tests__/PlanetLandscape.test.tsx`, `components/__tests__/AvatarEquipmentLayers.test.tsx`, `components/__tests__/LandscapeObjectSprite.test.tsx`, `components/__tests__/PlanetScene.test.tsx`, `components/__tests__/ShopProductThumbnail.test.tsx`, `components/__tests__/planetLandscapeCamera.test.ts`가 있다. UI 작업은 정적 DOM 검사만으로 시각적 품질을 입증할 수 없으므로, 실제 화면의 시각 확인을 포함한다. 테스트 추가·실행, 빌드 및 UI 구동 확인은 사용자 허용 후 수행한다.

허용되면 `apps/desktop` 작업 경로에서 다음 기존 회귀와 빌드를 실행한다.

```bash
npm test -- src/__tests__/App.test.tsx src/components/__tests__/PlanetLandscape.test.tsx src/components/__tests__/AvatarEquipmentLayers.test.tsx src/components/__tests__/LandscapeObjectSprite.test.tsx src/components/__tests__/PlanetScene.test.tsx src/components/__tests__/ShopProductThumbnail.test.tsx src/components/__tests__/planetLandscapeCamera.test.ts
npm run build
```

이번 승인 범위는 시각 표현이다. 작업 중 기존 상호작용을 바꿔야 한다는 사실이 발견되면 코드를 추가하지 않고 CEO에게 범위와 필요한 test-first 변경을 보고해 재분류한다.

## 위험과 자체 검토

- 세밀한 스프라이트는 작은 크기에서 뭉개질 수 있어 상세 풍경뿐 아니라 홈·그룹·미리보기 크기도 확인한다.
- 아바타 몸체와 장비를 따로 수정하면 기준점이 어긋날 수 있으므로 같은 작업에서 맞춘다.
- 기존 저장·탐사·상점 동작과 과거 설계 문서의 차이가 있다. 현재 구현과 테스트가 확인하는 선택 시 카메라 유지·오브젝트 목록 부재를 기준으로 삼는다.
- 작업 범위는 상세 화면의 표현과 이미 있는 아트로 제한했다. 새 상호작용, 서버 계약, 계정 상태, native 창 크기 변경은 없다.
- 진행 전 저장소는 `/Users/ma-24-007/Desktop/workspace/token-planet`의 `master`이며 조사 시점에 변경 파일이 없었다. 이 저장소는 연결된 worktree가 아닌 일반 체크아웃이다. 격리 작업 공간 생성은 별도 사용자 동의 뒤 결정한다.
