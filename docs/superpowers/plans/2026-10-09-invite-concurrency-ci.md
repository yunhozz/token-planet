# 공식 PostgreSQL runner의 초대 동시성 검증 통합 계획

> **For agentic workers:** Yunho Harness에 따라 CEO가 순차 실행을 조정한다. Coder가 승인된 runner와 테스트 파일을 수정하고, QA와 Reviewer가 독립 검증한다. 계획 승인은 구현 시작 승인을 포함하지 않는다.

**목표:** 공식 disposable PostgreSQL runner가 migration replay와 SQL suite를 통과한 뒤 초대 lifecycle의 독립 세션 동시성 테스트를 실행하고, 기존 소유 자원을 정리하게 한다.

**구조:** 기존 QA 전용 동시성 호출을 보존하고 명시적인 CI 모드를 추가한다. runner가 확인한 workdir·프로젝트·container ID·network ID를 전달하고, 동시성 harness가 대상 신원을 다시 검증한 뒤 기존 시나리오를 실행한다. guard 정책은 변경하지 않는다.

**기술:** Bash, Python 표준 라이브러리, `unittest`, Docker, container 내부 `psql`, Supabase CLI `2.119.0`.

**Spec:** `docs/2026-10-08-multiplayer-verification.md`의 I05–I13 동시성 수용 기준 및 `docs/superpowers/plans/2026-10-08-token-planet-invite-lifecycle.md`의 독립 세션 시나리오.

## 공통 제약

- 대상 저장소 `/Users/yunho/Desktop/project/token-planet`, branch `feat/multiplayer-policy-verification`.
- Coder 시작 전 branch, HEAD, staged/unstaged/untracked 상태를 새로 확인한다. 기존 네 개 README·검증 문서 변경은 보존한다.
- 기준 HEAD는 현재 brief상 `2dba01ec7fd8427666897f2d50bc634ad3d6a857`; handoff 직전에 재확인한다.
- `supabase/ci/product_docker_guard.py`, guard 허용 정책, Supabase CLI 및 pinned image 버전은 변경하지 않는다.
- shim, 광범위 Docker `PULL`/`PS`/prune 허용, hosted 프로젝트 접근, 제품 migration·Rust·React 변경을 추가하지 않는다.
- CI 모드는 `token-planet-ci-[0-9a-f]{24}`와 loopback DB port `56432`에만 허용한다. QA 기본 모드의 기존 정확한 project/workdir/container/port `56322` 계약을 유지한다.
- 테스트 fixture는 실행별 UUID와 이름을 사용한다. 이번 실행이 만든 fixture만 제거한다.
- 코드·invite code·token·DB URL·raw subprocess 출력은 artifact에 남기지 않는다.
- 전체 migration replay, 40개 pgTAP suite, 초대 동시성 7개 시나리오 모두 성공한 경우만 CI runner 통과로 기록한다.
- 이 계획은 hosted DB grants, hosted deployment, 실제 앱 A/B 또는 I01–I16 전체 완료를 검증하지 않는다.

## 파일과 책임

| 파일 | 책임 |
| --- | --- |
| `supabase/tests/invite_lifecycle_concurrency.sh` | 기존 QA 모드 유지, 제한된 CI 입력 및 신원 검증, 기존 동시성 시나리오 실행 |
| `supabase/tests/test_invite_lifecycle_path.py` | QA/CI 대상 경로와 project identity의 순수 검사 |
| `supabase/ci/run.sh` | 모든 migration·pgTAP 확인 후 기존 cleanup 전에 동시성 harness 호출 |
| `supabase/ci/test_run.py` | 호출 순서, 정확한 인자, 실패 전파, cleanup, 안전한 summary 검사 |
| `.github/workflows/supabase-migrations.yml` | CI에서 동시성 대상 identity 단위 테스트 실행 |

## 검토 초점

1. 이름이 같더라도 immutable container ID가 달라졌으면 SQL을 시작하지 않는다.
2. workdir·config·container labels·volume mount·network attachment·loopback binding 중 하나라도 예상과 다르면 실행 전에 거부한다.
3. migration reset, migration history, staging 재검증 또는 pgTAP 실패 뒤에는 동시성 테스트를 시작하지 않는다.
4. 동시성 테스트 실패·시간 초과·fixture cleanup 실패 뒤에도 기존 EXIT cleanup을 수행하고 최종 실패 상태를 보존한다.
5. artifact에는 일곱 고정 시나리오 결과만 기록하고 raw 로그·비밀 값을 내보내지 않는다.

## 작업 1: 동시성 harness의 CI 입력 계약

**파일:** `supabase/tests/invite_lifecycle_concurrency.sh`, `supabase/tests/test_invite_lifecycle_path.py`

**계약:** 기존 QA 호출은 그대로 유지한다. CI 호출은 아래 값을 모두 요구하며 부분·중복·알 수 없는 인자는 exit `2`로 거부한다.

```sh
bash "$REPO_ROOT/supabase/tests/invite_lifecycle_concurrency.sh" \
  --ci \
  --workdir "$WORKDIR" \
  --container "$CONTAINER_NAME" \
  --container-id "$CONTAINER_ID" \
  --network-id "$NETWORK_ID"
```

CI 모드는 canonical workdir, config의 `project_id`, container 이름·immutable ID·running state·labels·loopback `56432` publish·정확한 volume mount·network name/labels·loopback bridge 옵션·container attachment를 확인한다. 검증을 모두 마치기 전 SQL을 시작하지 않고, 이후 Docker `exec`는 전달된 immutable container ID로만 수행한다. QA 모드에서는 기존 고정 대상을 그대로 검증한다.

- [ ] **경계 helper 계약을 먼저 고정한다.** heredoc의 순수 helper 서명은 다음과 같다. helper는 Docker·SQL·subprocess를 실행하지 않는다.

```python
def parse_arguments(args: list[str]) -> tuple[str, str, str, str | None, str | None]: ...
def resolve_target(
    mode: str, workdir: str, container: str,
    container_id: str | None = None, network_id: str | None = None,
) -> tuple[pathlib.Path, str, int]: ...
def validate_target_identity(
    target: tuple[pathlib.Path, str, int], container: str,
    container_id: str | None, network_id: str | None, config: str,
    context: dict, container_info: dict, volume_info: dict,
    network_info: dict | None,
) -> str: ...
```

- [ ] **RED: 대상 identity 단위 테스트를 먼저 작성한다.** 기존 세 경로 테스트를 유지하고 다음 assertion을 고정한다.
  - `test_ci_arguments_require_exact_complete_contract`: 유효한 CI 인자는 `("ci", CI_ROOT, CI_CONTAINER, CI_ID, CI_NETWORK_ID)`를 반환한다. 누락·중복·추가·`--ci` 누락·빈 입력은 `ValueError`다.
  - `test_qa_arguments_preserve_existing_contract`: 기존 QA 인자는 `("qa", APPROVED, "supabase_db_" + PROJECT, None, None)`를 반환한다.
  - `test_ci_target_accepts_generated_identity_on_56432`: CI 대상 helper는 `(CI_ROOT.resolve(), CI_PROJECT, 56432)`를 반환한다.
  - `test_ci_target_rejects_partial_or_malformed_identity`: ID 누락, 잘못된 CI project 형식, 상대 경로, 다른 basename, CI identity의 QA 모드는 `RuntimeError`다.
  - `test_ci_identity_accepts_exact_owned_resources`: 정확한 fixture에서 `validate_target_identity(**fixture) == CI_ID`다.
  - `test_ci_identity_rejects_each_ownership_or_connection_mismatch`: config·container ID/name/state/labels·volume/mount·network ID/name/labels/attachment·Unix socket 불일치 각각이 `RuntimeError`다.
  - `test_ci_identity_rejects_wrong_missing_or_extra_wildcard_binding`: port 누락, `56322`, `0.0.0.0`, `::`, 추가 wildcard publish 각각이 `RuntimeError`다.
  - `test_ci_target_accepts_canonical_tmp_alias`: alias와 canonical path가 같은 target tuple을 반환한다.
- [ ] **RED 확인:** `python3 -m unittest discover -s supabase/tests -p 'test_invite_lifecycle_path.py' -v`를 실행한다. 새 CI 계약의 허용·거부 assertion이 실패해야 한다.
- [ ] **GREEN:** 실제 shell heredoc에서 안전하게 분리된 순수 `resolve_target(...)`와 `validate_target_identity(...)`를 테스트한다. Docker/SQL/subprocess 본문은 unit test로 실행하지 않는다. 기존 일곱 시나리오와 lock barrier는 유지한다.
- [ ] **검증:** 위 unit test와 `bash -n supabase/tests/invite_lifecycle_concurrency.sh`가 통과한다.

## 작업 2: 공식 runner의 순서·실패 전파

**선행:** 작업 1의 승인된 입력 계약.

- [ ] **RED: runner 호출을 검사하는 fake concurrency harness와 테스트를 추가한다.** 기존 `RunnerTests.setUp`, `FAKE_EVENTS`, `run_runner`, `event_rows`를 사용한다. Fake script는 호출 argv를 `{"tool":"concurrency","args":[...]}` event로 기록하고 기본 stdout으로 일곱 고정 `PASS <scenario>` 줄을 낸다. fixture env로 exit/stdout/stderr를 지정한다.
  - `test_concurrency_runs_after_all_tap_suites_before_owned_cleanup`: runner exit `0`; event index가 마지막 TAP < final migration verify < concurrency(1회) < container rm < volume rm < network rm이다.
  - `test_concurrency_receives_exact_owned_target`: argv가 `--ci`, canonical workdir, `supabase_db_<project_id>`, reset 후 확인한 immutable container ID, owned network ID와 정확히 일치한다.
  - `test_concurrency_failure_preserves_exit_and_cleans_owned_resources`: fake exit `47`이면 runner exit `47`, summary는 `concurrency_status=FAIL exit=47 diagnostics=redacted`, cleanup 순서는 container→volume→network다.
  - `test_reset_or_tap_failure_skips_concurrency`: reset·psql/TAP·final migration verify 실패 각각에서 concurrency event가 없고 기존 owned cleanup 순서가 유지된다.
  - `test_concurrency_summary_requires_all_scenarios_and_redacts_output`: 일곱 PASS만 성공 summary에 기록한다. exit `0`이어도 하나라도 누락되면 runner exit `1`; stdout/stderr의 secret sentinel은 runner 출력 및 artifact에 없어야 한다.
- [ ] **RED 확인:** 작업 디렉터리 `supabase/ci`에서 위 runner 테스트를 실행해 기존 runner에 동시성 호출·summary가 없어 실패하는지 확인한다.
- [ ] **GREEN:** `supabase/ci/run.sh`에서 migration staging 재검증이 끝난 직후, 성공 로그를 쓰기 전에 CI 모드 harness를 호출한다. 출력은 private workdir에만 저장한다. exit가 0이고 일곱 고정 PASS 결과가 모두 확인된 경우에만 `invite-lifecycle-concurrency-summary.log`에 PASS 행을 남긴다. 실패는 안전한 요약과 원래 종료 상태로 전파하고 기존 EXIT cleanup을 보장한다.
- [ ] Workflow의 기존 runner unit test 단계에 `supabase/tests/test_*.py` 검사를 추가한다. pinned image pull, artifact allowlist, 권한, `product_docker_guard.py`는 변경하지 않는다.
- [ ] **검증:** `python3 -m unittest discover -s supabase/ci -p 'test_*.py' -v`, `python3 -m unittest discover -s supabase/tests -p 'test_invite_lifecycle_path.py' -v`, `bash -n supabase/ci/run.sh`, `bash -n supabase/tests/invite_lifecycle_concurrency.sh` 및 기존 guard 회귀 검사가 통과한다.

## 작업 3: 독립 disposable DB 검증 및 수용

**선행:** 승인된 코드, 이미지가 이미 준비된 로컬 Docker, 빈 artifact 경로. 이미지가 없으면 추가 pull 없이 중단한다.

- [ ] QA가 branch·HEAD·변경 상태, 실행 순서와 Docker target 소유권 경계를 독립 확인한다.
- [ ] 저장소 밖의 새 빈 artifact 경로로 공식 `supabase/ci/run.sh`를 실행한다.
- [ ] 32개 migration 적용, 40개 pgTAP suite, 아래 일곱 concurrency 시나리오의 실제 PASS를 확인한다.
  1. `same-code competition and retry`
  2. `last-slot competition`
  3. `accept versus revoke`
  4. `accept versus transfer`
  5. `expiry during world-lock wait`
  6. `same-user different-world competition`
  7. `independently committed rate-limit counter`
- [ ] QA가 cleanup log, sanitized artifact, 종료 코드와 evidence path를 보고한다. Reviewer는 QA 뒤 전체 diff에서 target identity, 실행 순서, cleanup, 실패 전파, secret redaction, guard 불변을 독립 검토한다.
- [ ] CEO가 findings와 각 검증 기준을 수용한다. hosted grants와 native A/B는 별도 미검증으로 남긴다.

권장 commit 메시지:

```text
test(supabase): 공식 실행기에 초대 동시성 검증 통합
```
