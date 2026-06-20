# RM2.3 — session/submit gate 추출 (R3) — 2026-06-20

Goal: EXECUTION-ORDER.md [7] RM2.3 — architecture-remediation-tdd-plan Phase 2.
감사 결과 `submit_session_gate`는 이미 axum-free 타입 함수. 잔여 R3 갭(직접
단위테스트 불가)만 해소: state-free parse prefix 추출 + 직접 단위테스트.

origin/main=a619f1c (N1 wave 완료·push됨).

## RM2.3 slice

- [x] RED: in-crate `parse_submit_session_envelope` 직접 단위테스트 3개 작성
      → compile FAIL(`cannot find function`) = RED 확인.
- [x] GREEN: state-free envelope-parse prefix를 `parse_submit_session_envelope(
      body) -> Result<Option<ParsedSubmitSession>, HttpError>`로 추출, gate 위임.
      `ParsedSubmitSession`이 envelope Value 소유(suffix가 body/session 재독해).
- [x] focused: in-crate tests 35/35(신규 3 포함), submit_session_policy route
      16/16(무변경 안전망 그린). clippy boole-node exit 0.
- [ ] full gate (production code, consensus-adjacent) → `self-test: PASS`
- [ ] NotoriAndo commit + push + remote 검증 + EXECUTION-ORDER [7] ✅ 확정

## RM2.3 결정 (EXECUTION-ORDER 결정 로그에 미러)

- gate 전체 재작성 안 함 — state-free prefix만 분리(동작 보존 최우선).
- 상태 의존 suffix(session_store/nonce_ledger/서명검증) 무변경.
- HttpError.reason(public)으로 단언(field는 private) — missing_field/malformed_pk.
- fixture 무영향: gate는 내부 CheckedSubmitSession 반환, wire-shape 불변 →
  summary/JSON 빌더 미변경 → fixture-mirror 리스크 없음(게이트 전 확인).

## Notes / hazards

- boole-node엔 dev-tools feature 없음(그건 miner) — 단일 clippy run.
- zsh는 `${pipestatus[1]}`(bash `$PIPESTATUS[0]` 아님)로 파이프 첫 exit 확인.
