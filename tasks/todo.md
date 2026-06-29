# N0-pre.1 — lean-runner `#eval` forbidden token — 2026-06-29

EXECUTION-ORDER [3] N0-pre 잔재 중 **N3 전 binding 선결조건**. master todo spec
(todo-l1-network-master.md:156~) 기준. origin/main=811ebf6 at start.

## Why
`#eval` 은 임의 IO(`IO.Process.run`/`IO.FS.readFile`)를 node 권한으로 실행하고,
Lean이 이를 에러가 아닌 side-effecting 명령으로 컴파일 → 악성 proof가 검사 중
코드를 실행할 수 있는 구멍. peer가 proof를 보내는 N3.2부터 외부 공격면.

## Steps
- [x] RED: `check_file_rejects_eval_before_lake_spawn` (axiom 테스트 패턴 — temp
      package dir + `#eval IO.println` proof + expect_err, lake 불필요).
      토큰 추가 전 실행 → FAILED 확인 (lake 있으면 `#eval`이 유효 Lean이라 accepted).
- [x] GREEN: `FORBIDDEN_TOKENS`(lib.rs)에 `(b"#eval", "#eval")` 추가
      (기존 `blank_non_code` 어휘 처리 그대로 적용 — 주석/문자열 내 #eval 무시).
- [x] focused: forbidden-token 13/13 GREEN (#eval 신규 + sorry/axiom/native_decide
      무회귀).
- [x] 모듈 doc 주석에 `#eval` 반영. fmt clean.
- [ ] full gate `self-test: PASS` (boole-lean-runner = consensus 경로 — runtime-
      smoke-all / proof-to-block-benchmark green 로그 직접 확인).
- [ ] commit (NotoriAndo) + push + remote verify + CI green. EXECUTION-ORDER
      [3] pre.1 done 표기.

## Notes
- 범위: `#eval`만 (사용자 결정: `#check`는 IO 실행 불가라 제외).
- 사전점검: `fixtures/`·`lean/checker/Boole*`에 `#eval` 0건 → false positive 없음.
- 남은 N0-pre: pre.3/4/5/6/7/9 (전부 작은 하드닝, 병렬).
