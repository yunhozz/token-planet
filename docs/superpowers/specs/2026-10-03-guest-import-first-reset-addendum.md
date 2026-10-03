# 일반 게스트 첫 reset 가져오기 명세 추가안

상태: written spec 및 실행 계획 승인 완료. Tasks1–4의 로컬 구현·독립 QA·Reviewer gate 완료. 사용자의 최신 지시에 따라 Task4에서 중단한다. Tasks5–10의 writer·public RPC·native completion/worker·공개 장면은 미구현이다.
기준: PR #6, `feat/shop-system-revamp`, `f233b612773b8afda88528040bdd67886f9b0b06`.
상위 명세: [상점 개편 명세](2026-10-01-shop-system-revamp.md) §8.2–8.4.
관련 계획: [상점 개편 계획](../plans/2026-10-01-shop-system-revamp.md) Task8B–D.

## 1. 목적과 권위

실제 native 수집·게임 API가 생성한 nonempty 게스트 상태를 fresh 로그인 계정에 전체 원자 bootstrap 한다. local authenticated public RPC 성공 → transport/worker → 로컬 canonical cache·ownership → 허용 viewer의 공개 projection을 검증한다.

승인된 A 정책은 기존 로그인 업로드와 같다. 사용량은 **클라이언트 자기 신고 입력**이며 서버는 occurrence coverage·lineage·cycle·기본 효과·canonical payload·journal·reset·wallet을 독립 재구성하여 게임 credit을 확정한다. 클라이언트 잔액, disposition, authoritative flag, hash·UUID·앱 서명 자체는 지급 권위가 아니다.

외부 사용 발생의 진위·독립 attestation·변조 불가능성·복제 로그의 기기 간 전역 중복 제거를 보장하지 않는다. 독립 issuer는 추가하지 않는다. 원본 prompt·conversation·파일 경로·로그 내용은 전송하지 않는다. hosted/live 사용자 데이터·환경·배포는 이 명세의 실행 범위 밖이다.

## 2. 첫 성공 도메인

아래 조건을 모두 충족한다.

1. 새 provenance 계약 적용 후 새로 생성된 단일 guest lineage·단일 planet device다. 기존 자료에 버전 표시만 추가하지 않는다.
2. captured occurrence는 complete coverage 이고 수정·삭제·coverage 변경 이력이 없다.
3. activation 이후 ordinary 첫 reset이 정확히 한 번 성공했다. old cycle 하나와 current/new cycle 하나만 존재한다.
4. old-cycle raw tokens와 reset credit이 양수다. capture 시 current tokens·growth·stage·progress는 0 이다.
5. 모든 효과는 0 이다. 구매·소유 아이템·배치·장착·제거·pending 거래, game reward/bonus credit, era progress는 없다.
6. journal은 generation 0, deleted timestamp null 이다. legacy partial cosmetic/import가 pending이 아니다.
7. 서버가 실제 fresh 계정으로 확인한다.

활동 **증빙** `activity_days`와 활동 **보상**을 구분한다. 양수 usage 에서 활동 증빙은 생길 수 있으나 game_rewards·보상 credit은 비어 있어야 한다.

ordinary scan/reset은 기존 reward settlement를 실행해야 한다. 시대 bookkeeping을 생략하지 않는다. old-cycle 기본 growth 합계가 첫 threshold 5 미만이고 `era_progress` = []인 자료만 지원한다.

```text
daily_growth = log2(1 + daily_tokens / 100000)
old_growth = planet timezone 날짜별 daily_growth 합계
대표 사례: 한 growth date의 complete raw 1,000,000 → ordinary 첫 reset → capture
log2(11) ≈ 3.459 < 5
```

server numeric과 실제 native 계산에서 모두 threshold 아래이며 native `era_progress`가 없는지 검사한다. 계산 경계의 판정을 확정할 수 없으면 전체 보류한다. 임의 float epsilon 이나 marker 삭제로 성공시키지 않는다.

current 0은 capture 시점의 상태다. 이후 append delta가 성공 ACK 후 동기화되면 current/growth가 증가할 수 있다. 구매·아이템·양수 효과·보상·era progress·다중 reset·다중 기기·과거 proofless 자료 복구는 후속 성공 도메인이다. 전체 일반 가져오기 완료로 보고하지 않는다.

## 3. 버전과 기존 capture

새 성공 envelope는 schema_version=2, provenance.version=1, domain=`ordinary_first_reset_zero_effect_v1`이다. 별도 typed parser/validator를 사용한다.

schema 1 요청·저장된 capture·기존 held receipt는 수정하거나 동일 ID로 승격하지 않는다. 기존 strict normalizer를 완화하지 않고, 없는 timeline/bounds/revision·새 reset UUID를 제조하지 않는다. 같은 target의 재 capture는 기존 ID/payload를 보존한다. 기존 proofless raw 1m fixture는 held control로 유지한다.

## 4. 타입·시각·equality

| 타입 | 계약 |
|---|---|
| UUID | canonical lowercase 문자열, nil 금지 |
| Account | source=`local`, target은 정확히 `account:<auth.uid()>` |
| unsigned integer | JSON number 정수, 0..9223372036854775807 |
| positive integer | 위 범위에서 1 이상 |
| UTC timestamp | 유효한 달력 시각, UTC Z, YYYY-MM-DDTHH:MM:SS.ffffffZ |
| date | 유효한 YYYY-MM-DD |
| timezone | native/서버에서 인식하는 기존 IANA timezone |
| digest | lowercase SHA-256 hex 64 자리 |
| coverage | 정확히 complete |
| agent | 기존 지원 Agent wire enum |

provenance lifecycle 시각은 native 쓰기 transaction 에서 UTC microsecond 정밀도로 기록한다. 기존 canonical/journal DTO의 시각은 typed instant로 비교하고 해당 DTO의 기존 hash encoding을 유지한다. 정밀도를 맞춘다는 이유로 과거 capture를 재작성하지 않는다.

음수·소수·비유한 수·정수 범위 초과를 거절한다. Rust checked integer와 SQL numeric 중간 연산을 사용하고 bigint 저장 전 범위를 검사한다. integer/date/UUID/시각 비교에 epsilon을 사용하지 않는다.

Effects는 기존 ActiveEffects의 다음 7 필드가 모두 정수 0 인 object다.

```text
token_earning_bps, civilization_growth_bps, shop_discount_bps,
reset_cooldown_bps, natural_removal_discount_bps,
era_reward_tokens, streak_reward_tokens
```

필수/허용 키를 명시하고 unknown key·배열의 중복 logical key를 거절한다. raw JSON을 파싱하는 native 경계는 중복 객체 키를 거절한다. public RPC는 JSONB로 수신한 typed shape를 검증한다. JSONB 이전에 정규화된 raw duplicate key 까지 DB가 복원·검출한다고 보장하지 않는다.

canonical JSON은 object key 사전 정렬, 공백 없는 UTF-8, 정수 decimal 표현을 사용한다. 문자열 의미와 아래 배열 순서를 보존한다.

- occurrence: ingest_seq 오름차순
- cycle/baseline: 시작 시각 순서(old, current)
- segment: (cycle_id,date,effect_revision)
- activity: (reward_date,cycle_id)
- journal: (cycle_id,bucket_date,agent)
- 나머지 집계: typed logical key 사전 순서

hash는 서버가 typed normalization 후 재계산한다. replay/conflict는 저장된 **전체 JSONB payload equality**로 판정하며 hash 만 같다고 replay 하지 않는다.

## 5. schema 2 요청과 기존 data

```text
public.import_guest_shop(p_import_id uuid, p_request jsonb) -> jsonb
p_request  =  {
  schema_version: 2,
  snapshot: {
    import_id, target_account_id, source_account_id: "local",
    source_fingerprint, disposition, captured_at_utc,
    provenance, canonical_payload, data
  }
}
```

import_id=p_import_id다. disposition은 로컬 검사 결과이며 지급 권한이 아니다. `source_fingerprint`는 snapshot 에서 자신의 필드를 제외한 전체 canonical encoding의 SHA-256 이다.

data는 기존 GuestShopImportData의 명시 필드와 타입을 사용하고 schema 2 에서는 optional proof 배열도 명시적으로 포함한다. 모든 source를 새 provenance 및 서버 재구성과 대조한다. guest source는 다음 의미를 유지한다.

```text
effect_cycle_bounds_authoritative = false
effect_timeline_state = null
effect_history = []
```

data.effect_cycle_bounds는 provenance bounds와 동일하다. guest baseline/bounds를 검증하고 성공 후 서버가 채택한 canonical timeline을 별도로 반환한다. 클라이언트 authoritative=true로 서버 권위를 얻지 않는다.

## 6. provenance·occurrence·baseline

```text
provenance  =  {
  version: 1, domain: "ordinary_first_reset_zero_effect_v1",
  authority: "client_self_reported", lineage_id, device_id,
  activated_at_utc, clock_policy: "guest_utc_monotonic_v1",
  ingest_watermark, occurrence_count, prefix_fingerprint,
  occurrences, cycle_bounds, baselines, reset_receipt
}
occurrence  =  {
  occurrence_id, record_version, ingest_seq, device_id, agent,
  occurred_at_utc, ingested_at_utc, cycle_id, total_tokens,
  coverage: "complete"
}
cycle_bound  =  {cycle_id, started_at_utc, ended_at_utc}
baseline  =  {
  cycle_id, revision: 0, started_at_utc, ended_at_utc,
  active_instance_ids: [], effects: <all-zero ActiveEffects>
}
```

lineage/device UUID는 생성 시 영속화하며 device는 planet_device_id와 같다. occurrence UUID는 최초 수집 시 원래 native usage key에 mapping 하여 보존한다. wire 에는 경로나 원래 key의 민감 문자열을 넣지 않는다. 재수집 중복은 새 occurrence/sequence를 만들지 않는다.

첫 insertion `record_version` = 1 이다. lineage mutation sequence는 1 부터 단조 증가한다. 첫 성공 prefix는 수정·삭제가 없으므로 ingest_seq=1..watermark에 누락이 없고 `occurrence_count` = watermark다. key·UUID·sequence가 unique 여야 한다. correction은 별도 mutation 증거로 남긴다.

`prefix_fingerprint`는 `{version:1,lineage_id,device_id,ingest_watermark,occurrences}`의 canonical SHA-256 이다. 변경 검출/ACK correlation 이며 실제 사용 발생 인증이 아니다.

```text
activation = old.start
activation < reset_at <= captured_at <= server transaction_timestamp
old.end = new.start = reset_at
new.end = null
activation <= occurred_at <= ingested_at <= captured_at
old occurrence interval = [activation, reset_at)
new occurrence interval = [reset_at, captured_at]
```

lifecycle clock이 역행하면 지원 가능 provenance로 기록하지 않는다. 미래 capture/reset은 allowance 없이 hold 한다. replay는 저장 결과를 재생하며 현재 시각으로 재검증해 결과를 바꾸지 않는다. reset 정확한 시각의 occurrence는 new cycle 이고, 양수 new-cycle occurrence가 capture에 있으면 current 0 도메인 밖이다.

old/current bounds와 baseline은 각각 실제 cycle 생성 transaction 에서 기록하고 정확히 일치해야 한다. revision 0은 cycle 별 기본 효과이며 양수 interval을 합성하지 않는다.

## 7. 첫 reset 증빙·deadline

```text
reset_receipt  =  {
  request: {kind:"reset_planet", request_id, cycle_id: <old>},
  result: {
    status:"reset", request_id, previous_cycle_id, new_cycle_id,
    reset_at_utc, final_effect_revision: 0,
    final_active_instance_ids: [], final_effects: <all-zero>,
    frozen_deadline_before_reset_utc: null, reset_available_at_utc,
    raw_tokens, bonus_tokens: 0, credited_tokens, shop_state_revision
  }
}
```

실제 reset API 호출 전에 생성·영속한 동일 UUID가 request/action receipt/result/settlement를 연결한다. capture 에서 치환하지 않는다. 첫 reset 전 frozen deadline은 null 이며 activation 에서 임의 cooldown을 제조하지 않는다.

```text
raw_tokens = old occurrence 합계 > 0
credited_tokens = raw_tokens
reset_available_at = reset_at + 86400 seconds
```

후속 deadline은 reset transaction 에서 한 번 기록해 동결한다. import/capture 시각으로 다시 시작하지 않으며 오래된 capture의 deadline이 이미 지났을 수 있다.

data의 old/new bounds, last_reset_at/deadline, reset settlement proof UUID/cycle/time/final revision/effects, old bonus settlement 1 행 amount 0, old wallet claim 1 행 raw amount, old journal 종료/credit/time이 모두 receipt와 같아야 한다. claim은 비교 대상이다. 서버 credit INSERT 값은 occurrence 에서 재계산한다.

reset 준비·reward/contribution 정산과 provenance/credit 확정은 같은 논리적 게임 transaction 이다. credit 만 먼저 확정하는 부분 성공을 허용하지 않는다.

## 8. canonical payload·성장 재구성

capture의 `canonical_payload`는 실제 native `PlanetDeviceContributionSnapshot` builder 결과와 동일한 typed payload다.

```text
activity_days, canonical_version, current_cycle_id,
current_planet_tokens, daily_segments, daily_tokens,
device_id, incomplete, lifetime_tokens
```

`canonical_version`은 영속 양수 버전, incomplete = false, lifetime=전체 captured token 합계, current=new 합계 0, `daily_tokens` = {}다. `daily_segments`는 captured cycle/date/revision 0 별 합계이며 실제 DTO의 `(cycle_id,date,effect_revision,tokens)` 4 필드만 갖는다. data의 `GuestEffectContribution` growth_bps/wallet_bps는 0 이고 이를 canonical segment에 추가하지 않는다. `activity_days`는 기존 reward-timezone builder와 동일한 날짜별 증빙이다.

서버는 occurrence 에서 lifetime/cycle totals, planet-timezone segment, reward-timezone 활동 총량·최초 시각, agent/date/cycle usage와 journal, reset raw credit·bonus 0·wallet, current growth/stage/progress 0을 재구성한다. 실제 canonical payload와 typed equality를 확인한다.

old growth는 서버 numeric 로그 계산과 native 결과에서 모두 threshold 5 아래여야 하고 `era_progress` = []다. current growth 0은 먼저 integer total 0 으로 검증한다. float claim 으로 지급하거나 epsilon 으로 threshold를 통과시키지 않는다. 경계 불확실성은 보류한다.

서버 저장 후 동일 contribution DTO를 조회한 결과도 요청 canonical payload와 같다. import 전에 일반 upload로 fresh 게임 행을 만들지 않는다.

## 9. journal·wallet

journal state generation 0 / deleted null, old/new journal cycle=bounds다.

```text
old: ended_at = reset_at, wallet_credit = raw_tokens, wallet_credit_at = reset_at
new: ended_at = null, wallet_credit = null, wallet_credit_at = null
entry key = (device_id, cycle_id, bucket_date, agent, generation)
```

entry의 confirmed_tokens/coverage는 occurrence 합계와 같다. 누락·중복·tombstone·generation 변경은 전체 보류다. 영속 native builder의 entry revision을 보존하며 source `acknowledged_revision` = 0 이다. 서버 ACK는 검증 저장한 exact revision/hash 만 포함한다.

`payload_hash`는 기존 `GrowthJournalEntry::compute_hash`의 encoding과 9 필드 `(agent,bucket_date,confirmed_tokens,coverage,cycle_id,device_id,generation,present,revision)`를 사용한다. 새로운 encoding 으로 기존 journal hash를 재작성하지 않는다.

서버 wallet=독립 재구성 old raw reset credit+bonus 0+reward 0−purchase 0−removal 0다. journal credit 에서 wallet을 역산하지 않는다. 다른 credit/debit/reward가 있으면 전체 보류다. wallet claim/journal credit/settlement를 독립 결과와 대조한다.

## 10. immutable capture·append delta·precise ACK

SQLite 한 transaction 에서 import UUID/target, 전체 snapshot, provenance prefix/watermark, canonical payload/version, source/prefix fingerprint, phase = captured를 저장한다.

capture 후 구매·배치·장착·제거·reset·guest reward 확정을 제한한다. **원본 usage 수집·raw 저장은 계속한다.** scan 자체를 실패시키거나 생략하지 않는다. capture 재호출은 기존 ID/payload를 반환한다.

watermark 보다 큰 sequence의 새 key 만 append delta다. captured prefix key/version/content와 prefix fingerprint가 같아야 한다. event timestamp 만으로 prefix를 나누지 않는다.

```text
source_relation = exact | append_only | captured_prefix_changed | unverifiable
```

현재 전체 fingerprint가 달라도 prefix 불변 + 새 key 추가가 증명되면 append_only다. 기존 `source_matches_current` = false 만으로 이를 판단하거나 해당 flag를 무시하지 않는다. current에 양수 delta가 생겨도 capture payload를 바꾸지 않는다.

```text
ACK  =  {
  lineage_id, device_id, ingest_watermark, occurrence_count,
  prefix_fingerprint, canonical_version, canonical_payload_fingerprint,
  journal_entries: [{logical_key, revision, payload_hash}]
}
server receipt 검증
→ canonical cache/ownership/import marker/precise ACK 로컬 원자 commit
→ delta signed 재계산
→ 필요 시 새 canonical version 일반 업로드
```

ACK는 prefix 만 인정한다. watermark 이후 occurrence 나 이후 journal revision을 ACK 하지 않는다. delta 없으면 captured version을 pending upload로 다시 예약하지 않는다. delta가 있으면 cumulative payload에 prefix가 포함될 수 있으나 새 reset credit은 만들지 않는다. 뒤늦게 수집한 old-cycle occurrence는 lifetime 만 증가시키고 closed credit은 추가 지급하지 않는다.

## 11. captured-prefix correction

수정·삭제·coverage 변경은 첫 성공 도메인 밖이다. raw storage/mutation 증거를 보존한다.

| 발견 시점 | disposition |
|---|---|
| 전송 시도 전 | 전송 hold, immutable capture/원본/correction 보존 |
| 전송 시작 후 결과 불명 | same-ID/same-payload receipt 복구 우선 |
| 서버 held/conflict | ownership 변경 없이 correction hold |
| 서버 imported | 확정 snapshot/exact ACK 복구와 `correction_hold`를 같은 로컬 transaction에 기록; correction은 ACK 하지 않음 |
| imported 이후 correction | 해당 lineage 자동 correction/delta upload·게임 변경 hold |

서버 성공을 취소됐다고 표시하지 않는다. capture 재작성·자동 새 ID·credit 회수/재지급·역사 정정은 하지 않는다. `correction_hold` 에서 raw 수집·확정 서버 상태 조회는 계속된다. 해소/자동 재정산은 후속 설계이고 이번 성공 범위에는 포함하지 않는다. 정상 append-only는 `correction_hold`에 들어가면 안 된다.

## 12. 서버 인증·freshness·atomic bootstrap

null auth.uid, target 불일치, shape/ID 오류는 receipt 쓰기 전에 거절한다. `user_metadata`/authority flag는 권한이 아니다. 기존 public RPC의 제한된 privileged 경계를 유지하고 anon/PUBLIC 기본 EXECUTE revoke, authenticated 승인 RPC grant, private table/function 접근 차단·RLS·빈 `search_path`·schema-qualified 이름을 적용한다. `service_role` secret을 클라이언트에 추가하거나 helper 권한을 일반 확대하지 않는다.

처리 순서는 다음과 같다.

1. auth/envelope/import UUID/target 검사.
2. 게임 state를 초기화하지 않는 공통 account lock.
3. 해당 ID 기존 public/private receipt 조회: equal payload 저장 결과 replay, different payload conflict, held 무승격.
4. 실제 fresh 재확인.
5. 단일 `transaction_timestamp`로 전체 source/canonical/journal/wallet 검증 완료.
6. FK 순서 canonical 게임 저장.
7. 저장 상태 읽기 전용 대조, private result와 precise ACK 생성.
8. 원본 payload/normalized provenance/성공 result를 receipt에 마지막 저장.

source 검증 중 게임 행을 부분 생성하지 않는다. auth user·membership·account lock 만으로 active로 판정하지 않는다. 기존 freshness 검사의 profile/usage/planet/device/wallet/journal/shop/effect/거래/reset/reward/legacy import/success receipt를 유지한다. imported receipt 만 남아도 fresh 아님; held receipt 만으로 다른 valid ID를 영구차단하지 않는다. 미확정 source는 전체 보류, overwrite/merge/부분복사 없음이다.

성공 canonical 상태는 profile/new cycle/lifetime/current 0인 planet state, 고정 reward timezone/shop revision, 단일 device/exact contribution/version, old/new adopted baseline/contribution/activity, reset request/settlement/raw wallet credit, journal state/cycles/days, source/ACK/receipt다. item/purchase/removal/positive effect/reward 행은 만들지 않는다.

import가 world나 membership을 자동 생성하지 않는다. `shared_visible` = false(hidden 정책)를 기존 native-empty와 같이 초기화한다. current 0 자연 objects=[]이며 과거 자연 개체를 현재 장면에 복사하지 않는다.

constraint/INSERT/result 생성/final receipt 실패는 raise 하여 전체 statement(game/success receipt/new lock)를 rollback 한다. import×import와 import×초기 usage/game writer가 공통 account lock→게임 row lock 순서를 사용한다.

## 13. 응답

```text
correlation = {schema_version, import_id, account_id, source_fingerprint, status}
status = imported | active_account | source_unverifiable | request_conflict
```

schema 2 imported는 typed `ShopState`/`PlanetState`/`ShopEffectTimeline`, canonical contribution, journal confirmation, precise ACK를 모두 포함하며 receipt에 불변 저장한다. held/conflict 에는 imported state/성공 ACK를 넣지 않는다.

auth/shape/target 오류는 기존 error 계약이며 receipt 없음이다. well-formed unsupported source는 `source_unverifiable` held receipt 만 저장하고 게임 DML 0 이다. source reason은 private 명시 enum 으로 표시할 수 있지만 내부 SQL·경로·로그를 반환하지 않는다.

## 14. worker·캐시·ownership 복구

새 shop import pending을 legacy cosmetic import 및 ordinary usage/journal/shop upload 보다 먼저 검사한다. pending이 있으면 target을 active로 만드는 일반 upload를 차단하고 raw scan은 계속한다.

worker는 전송 전 exact/append-only·auth target 검사와 durable attempt_started를 저장한다. transport는 저장 payload를 그대로 보낸다. 응답 account/import/fingerprint, old→new cycle, states 간 device/cycle/revision, ACK prefix/version/journal hash, imported 필드 완비/held 성공 필드 부재를 대조한다.

canonical private caches·ownership·imported marker·exact ACK·pendingphase를 하나의 SQLite transaction 으로 완료한다. raw occurrence/delta/correction 증거는 삭제하지 않는다. commit 내부에서도 계정선택을 재확인한다.

response loss/decode/timeout/local commit failure는 same ID / same payload 복구다. 새 ID로 fresh를 재통과하지 않는다. 계정변경은 pending을 삭제하지 않으며 다른 token/늦은응답을 현재 cache에 적용하지 않는다. 원래 target 복귀 시 복구한다.

정상완료 뒤 게임제한을 해제한다. `correction_hold` 이면 제한 유지다. 화면 refresh 실패는 확정 marker를 되돌리지 않고 표시오류로 처리한다.

## 15. 공개 projection

기존 `WorldPlanet` 공개필드와 접근정책을 사용한다. private `ShopState`/timeline/receipt를 공유 DTO로 재사용하지 않는다. 첫 성공은 profile/current 0/growth·stage·progress 0/lifetime/빈 objects/기존 rank를 반영한다. private canonicalcycle 연결은 검사하되 공개 DTO에 cycle ID를 새로 추가하지 않는다.

wallet/credit/debit, purchase/reward/reset/import receipt, import ID/source/prefix fingerprint, lineage/occurrence/device ID, canonical version/payload/ACK, private effect/provenance/reward timezone, agent/date raw 증빙·원본 경로/prompt/log는 공개하지 않는다. 기존 public lifetime/growth/rank는 유지한다.

공개 조회는 baseline/settlement/revision/receipt/ACK를 쓰지 않는다. 기존 hidden placeholder, 허용 world viewer, outsider/다른 world/anon 거절을 유지한다. import 후 별도 명시적 sharing 설정으로 허용 viewer를 구성하여 검증한다. 테스트 membership 설정을 freshness와 혼동하지 않으며 target-owned world를 bootstrap 전에 생성하지 않는다.

## 16. 지원 불가 disposition

schema 1의 proofless capture 및 기존 held receipt, 계약 전 lineage, unknown cycle/bounds/baseline, incomplete coverage, prefix 누락 / 중복/correction/deletion, canonical 불일치, reset UUID/deadline/settlement 오류, item/effect/reward/era, 여러 reset/lineage/device, 시각 역행/미래 capture, active/미확정 target, journal deletion/generation/hash 오류, overflow/부정 거래/partial legacy import는 전체 보류·무적립·원본 보존이다.

partial state를 복사하거나 실패 source에 credit 0 imported를 반환하지 않는다. 지원 범위 확대·과거 자료 복구·correction 재정산은 후속 설계다.

## 17. 수용 기준과 증거

검증은 disposable SQLite/local DB/synthetic auth/world/mock에 한정한다. independent QA와 Reviewer가 필요하다.

### 17.1 actual native positive

새 lineage 에서 ordinary 수집→journal 준비→reward settlement→ordinary reset→capture를 실행하며 production 준비/정산을 생략하지 않는다. 한 growth date raw 1m fixture는 원본 native 직렬화에서 생성하여 JSON/inc 일치를 확인한다.

```text
raw credit = 1,000,000; bonus/reward/debit = 0; capture current/growth/stage/progress = 0
old/new bounds/baseline exact; final revision 0; old frozen deadline null
new deadline = reset+86400s; era_progress = []
```

old proofless raw 1m fixture는 변경 없이 held control 이다.

### 17.2 독립 failure mutations

prefix 누락 / 중복/device/key/version/sequence, bounds gap/overlap/reset boundary cycle, final baseline/revision/effects, UUID/cardinality/deadline, 미래/역행시각, incomplete coverage, canonical 합계/version/segment/activity, journal hash/revision/generation/credit, wallet claim/bonus/reward/debit/overflow, item/effect/era, fingerprint 오류를 각각 검증한다. 모든 실패 전체 보류/거절·게임 DML 0·success receipt 0 이며 다른 선행 오류가 의도한 검사를 가리는 fixture를 증거로 쓰지 않는다.

### 17.3 replay·rollback·race

다음 항목을 각각 검증한다.

- 같은 ID와 동일 payload: 동일 receipt 재생, 추가 지급 0.
- 같은 ID와 다른 payload: conflict, 기존 receipt 불변.
- imported 이후 새 ID: active hold.
- 기존 public/private held receipt: 성공 승격 없음.
- imported receipt만 남은 상태: fresh가 아님.
- 마지막 source 검증 오류: 게임 DML 0.
- journal·settlement·final receipt 저장 실패: 전체 rollback.
- import × import 경쟁: bootstrap 한 번만 성공.
- import × 초기 usage/game writer 경쟁: lock ordering 유지, overwrite 없음.

fault injection은 fixture-local trigger로만 수행한다. production RPC에 test mode 인자를 추가하지 않는다.

### 17.4 pending·delta·correction

- 게임 변경 제한 중에도 raw 수집을 계속한다.
- append delta는 동일 capture로 성공하며, ACK는 prefix만 인정한다.
- current delta는 새 canonical version으로 동기화한다.
- 늦은 old-cycle delta는 lifetime만 반영하고 closed credit 추가 지급은 0이다.
- response loss·reopen·local commit 실패·account switch의 복구를 검증한다.
- 전송 전 correction은 hold, 전송 후 발견한 correction은 receipt 복구를 우선한다.
- correction으로 자동 새 ID·credit 재정산·ACK를 만들지 않는다.
- 화면 표시 실패에도 성공 marker를 보존한다.

### 17.5 local public success와 privacy

실제 local authenticated public RPC의 `imported` 결과부터 아래 순서를 연결한다.

1. 정확한 native RPC body·authorization·typed decode.
2. worker의 원자 완료와 canonical cache.
3. 명시적 sharing 설정.
4. 허용 viewer의 공개 scene 조회.

mock 응답만으로 DB success를 수용하지 않는다. 초기 current 0과 별도 in-flight delta에 따른 후속 증가를 구분해 확인한다.

recursive privacy field 검사, 반복 공개 조회의 무쓰기, hidden·outsider·다른 world·anon 접근 정책, client의 private table/function 접근 거부를 검증한다. DB 전후 증거는 해당 실행이 소유한 synthetic fixture만 cleanup/rollback하고 기존 데이터 보존을 확인한다.

## 18. 완료의 의미와 다음 gate

완료는 새 provenance로 기록한 **제한된 ordinary 첫 reset 도메인의 local public 성공**이다. item/다기기/다중 reset/full guest import/correction 재정산/과거 복구/hosted 배포/live 전송 완료로 보고하지 않는다.

Task 1–2 gate는 통과했다: v2 DTO/provenance 및 실제 reset baseline·atomic provenance가 구현됐고, 최종 focused 55/55·전체 Rust `--lib` 295/295·관련 reset 회귀 1/1·stable format·diff check가 통과했다. 독립 Reviewer는 Task 2 재검토를 CLEAR로 마쳤다. Clock regression과 `occurred_at == reset_at` 경계는 RED→GREEN으로 확인했다.

다음 구현 gate는 Task 3의 immutable capture·source relation·raw-preserving game gate다. 이어지는 SQL validator/writer/public dispatch, native transport/worker/cache completion, 공개 장면 E2E 및 hosted/live rollout은 각각 후속 Task의 별도 gate를 통과하기 전에는 완료로 주장하지 않는다. 이 상태 기록은 Task 1–2의 검증 근거만 반영한다.

## Planner 및 팀장 자체 검토

- A를 기존 authenticated usage/server credit과 일치시켰고 issuer/attestation을 요구하지 않았다.
- raw 1m 대표 도메인이 normal scan·settlement·reset에서도 도달 가능하며 era/다중 날짜 threshold 경계는 보류한다.
- schema 1 불변과 schema 2 전용 검증, guest baseline/서버 채택 timeline의 의미를 분리했다.
- source 수집은 유지하고 game mutation/reward 확정만 제한했다. prefix/delta/ACK와 correction hold를 명시했다.
- fresh 전 upload·partial bootstrap·held 승격·새 ID retry·ACK 확장·다른 계정의 cache에 적용되는 오류을 금지했다.
- actual nine-field canonical builder와 segment 4 필드, 기존 journal 9 필드 hash를 대조했다. JSONB raw duplicate 검출의 한계를 바로잡았다.
- 최초 draft의 hidden 표현은 실제 `shared_visible` = false로 명시하고, 공개 scene 검증 전 명시적 sharing 단계를 추가했다.
- correction 자동 해소/재정산, unsupported source 복구는 명시적 held/지원 제외로 처분했다. 이 도메인 안에서 추가 제품 질문은 없다.
- sequence/prefix/schema 2 lineage와 실제 reset evidence는 Task 1–2에 구현됐다. immutable capture/prefix lifecycle·ACK/cache completion 및 서버 검증·writer는 아직 구현 완료로 주장하지 않는다.

## 확인한 source 근거

아래 위치는 기준 commit f233b61의 repository source다.

| 파일 | 근거 |
|---|---|
| 기존 명세 §8.2–8.4, 계획 Task8B–D | guest/server 권위, 전체 보류, closed cycle, atomic/replay/public privacy |
| storage/shop_import.rs:34–145,2176–2397 | immutable capture, 전체 fingerprint, reset 증빙 누락, old raw 1m held |
| storage/shop_effects.rs:593–650 | actual canonical builder, segment 4 필드/activity snapshot |
| domain/growth_journal.rs:34–55 | journal 9 필드 hash |
| lib.rs:715–746, storage/cosmetic_shop.rs:650–660 | normal scan/reset journal/reward 정산 |
| domain/shop_effects.rs:81–114, growth.rs:10 | zero-effect growth와 threshold 5 |
| domain/planet.rs:96–120 | 기존공개 `WorldPlanet` DTO |
| migrations/20261001000200_shop_effect_validation_fixes.sql:39–195 | authenticated 9 필드/raw 합계/서버 baseline/closed segments |
| migrations/20261001000201_shop_effect_upload.sql:254–281 | server wallet 권위/canonical 업로드 |
| migrations/20261001000205_shop_reset.sql:314–342,440–506 | server credit/deadline/closed cycle |
| migrations/202610010004_shop_import_and_sharing.sql:3757–4020 | fresh/private bootstrap/public held |


## Task 3 execution evidence (2026-10-03)

Task3 PASS after independent QA and Reviewer targeted re-review CLEAR. Exact QA commands: `cargo test --manifest-path apps/desktop/src-tauri/Cargo.toml --lib guest_import_v2 -- --nocapture` 85/85; full `--lib` 329/329, including account switch 6 and cosmetic command 26. Stable cargo format and git diff checks passed. Logs: `/private/tmp/shop-import-v2-task3-qa-review-{focused,lib,fmt,diff,fixture}.log`.

Reviewer confirmed immutable capture, raw-preserving gates, ownership ordering and compatibility; two P2 fixes retain complete zero-token post-reset occurrences/current0/actual canonical builder parity and preserve AttemptStarted independently of correction_hold. V2 occurrence cycle aggregates are checked against raw day-agent count/sum/coverage before grouping. Positive new usage remains a durable hold. No unresolved P1/P2.

Actual native fixture is generated by `exports_native_v2_first_reset_fixture_from_durable_capture` without serialized repair. JSON/inc bytes match. Retained raw claim and one reset proof, zero effects/bonus/current, deadline 86400s, authoritative=false. V2 JSON SHA256 `c5bbc955848e86d389c4d4071026b940f1f8837617cb9ed11f138a610d4e3a62`; inc `c6280034c20667e5d9d06ca2746ed7c398bb5ba988aa10c4d69bf80fa00bac5f`. V1 JSON SHA256 `8655804f5f7b4bb4761e3616362cbf519816cfcff3875b5bbe87dea60e42bf0c`; inc `9261df576689250909dac48b4095f5cb2032ca1b00cc60110301fffe5c3077cb`, unchanged.

Task3 gate 당시의 기록: local storage/capture/scan/command acceptance만 완료했고 Task4–10 및 public RPC/native completion/worker/public scene은 미검증이었다. 당시 hosted/live·deployment·commit·push는 수행하지 않았다. 이후 실행과 사용자 지시 변경은 아래 Task4 기록에 따른다.


## Task 4 execution evidence (2026-10-03)

Task4 PASS: pure schema2 validator와 pinned runner의 독립 QA 및 Reviewer 재검토 CLEAR. 사용자의 최신 지시에 따라 Task4 완료 후 중단한다. Tasks5–10의 writer/public RPC/native completion/worker/public scene은 수행하지 않았으며 승인된 설계의 후속 미구현 범위로 남는다. 완료된 Task마다 commit·non-force push하는 후속 사용자 지시가 기존 push 금지 실행 제약을 대체한다. Tasks1–3 checkpoint는 `63dff1054874a3c7fb64e44eb4c1ca098f37c8b8`으로 기존 `origin/feat/shop-system-revamp`에 push됐다.

CLI 2.118.0이 생성한 migration은 `supabase/migrations/20261003061830_guest_import_first_reset_validation.sql`이다. 설치·upgrade·reset 없이 지정된 local project `token-planet-shop-revamp-test`, Docker `desktop-linux`의 로컬 Unix endpoint, 고정 container/volume 및 host port 55432, `postgres|postgres|5432`를 확인했다. 해당 파일만 `psql -X -v ON_ERROR_STOP=1 --single-transaction`으로 적용했다. 기존 010004 helper의 본문·권한 동등성을 확인했으며 누락된 history 행을 repair하거나 다른 pending migration을 일괄 적용하지 않았다.

정상 native fixture와 schema1 held control을 유지한다. occurrence로 raw aggregate/canonical segment/activity/journal/reset/wallet을 독립 재구성한다. 과거 cycle의 end는 실제 bonus settlement 시각과 일치하며 activation..reset 안에 있어야 한다. 실제 bounds/baseline/journal reset 경계는 그대로 일치시킨다. 0-token segment를 보존하고 activity의 최초 시각은 양수 occurrence에서만 계산한다. 미래 capture와 inner prefix hash mutation은 선행 형식·outer source hash 오류에 가려지지 않도록 수정했다. Reviewer의 P2 세 건을 해소했다.

독립 QA 명령·결과:

- `bash supabase/tests/run_guest_import_v2.sh shop_guest_import_v2_validation.sql`: exit0, 53/53, 명시적 ROLLBACK 및 probe cleanup.
- `cargo test --offline --manifest-path apps/desktop/src-tauri/Cargo.toml --lib post_reset_zero_occurrence_is_captured_as_valid_v2_with_empty_daily_totals`: exit0, 1/1 actual native builder/capture parity.
- `bash -n supabase/tests/run_guest_import_v2.sh`, `git diff --check`: exit0.

QA 증거: `/private/tmp/shop-import-v2-task4-qa-review-{summary,sql,native,before,after}.log`. 수정 전 RED는 `/private/tmp/shop-import-v2-task4-reviewer-p2-red-confirmed.log`의 53개 중 51 PASS/정확한 두 정상 source 실패다. 변경 후 전체 GREEN과 적용 기록은 `/private/tmp/shop-import-v2-task4-reviewer-p2-{green,apply,poststate}.log`다.

Migration SHA256 `d62e939da7a5898b5b584ad48a27edbdbb084a024f54deb5e10142a86714708a`; suite `bfc77efc75acd42552ebd7e7b2cf7b2c6cf6f253364bb442358664670efb6bc2`; runner `79ede74a3ddb3bc57e9cb7cbf6a7c817f4dc3dd5eea0303463e982473e0addd9`. 이전 Task3의 v1/v2 fixture 네 SHA는 불변이다. 설치된 다섯 private 함수는 postgres owner/SECURITY INVOKER/빈 search_path/postgres-only EXECUTE이고 신규 public 함수는 0개다. Normalizer body MD5 `7ec86be52da19ee502d7ecb936374434`. Migration history 26행/max `20261001000208`/digest `953f6733f0fa190faefec5ff83ff022b`와 public/private/auth 보호 데이터 digest `dce33ee61f853986a32dd5fc84600182`가 QA 전후 동일하다. Hosted/live 작업·배포·merge는 수행하지 않았다.
