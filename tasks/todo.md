# N0-pre 잔여 마감 (pre.3/4/6/7/9) + pre.5 보류 — 2026-07-02

EXECUTION-ORDER [3]의 N0-pre 병렬 잔여를 3개씩 2배치로 마감. 각 slice
RED→GREEN→focused, 배치별 공유 full gate 1회, NotoriAndo author 단독 커밋,
push, remote SHA 일치, CI green 확인. **closed-local validation만 — public/API
benchmark claim 아님.**

## Batch 1 — 독립 크레이트 (충돌 0), push `c039820`, CI green
- [x] pre.3 install.sh elan `master` → v4.2.3 tag pin + sha256 checksum
      (portable sha256sum/shasum fallback) · contract test · `2b80317`
- [x] pre.6 boole-mcp `read_mcp_frame` 16 MiB `MCP_FRAME_MAX_BYTES` 가드
      (할당 전) · RED `stdio_frame_over_max_content_length_is_rejected` · `16098ba`
- [x] pre.9 boole-miner `extract_openai_compat_text` reasoning-channel gate
      (기본 off, `allow_reasoning_as_answer` opt-in builder + LLMDriverConfig
      필드 7 literal) · RED `empty_content_with_reasoning_is_not_answered_by_default`,
      기존 fallback 테스트는 opt-in으로 갱신 · `c039820`

## Batch 2 — boole-node (같은 크레이트, 다른 영역), push `bf00e26`, CI green
- [x] pre.4 `--http-rate-limit-per-60s` flag (env, 기본 None) + wiring
      hardcode `None` 제거 · CLI-spawn RED `run_local_rate_limit_flag_returns_429_over_limit`
      (quota 2 → 3번째 429) · `53b2fc8`
- [x] pre.7 `/status`에서 `blockStorePath`·`lean_checker_dir` 절대경로 키 제거
      (`lean_checker_disabled` bool 유지), config doc 주석 정정 · RED
      `status_response_contains_no_filesystem_paths` · `bf00e26`

## pre.5 — 보류 (2026-07-02 사용자 결정)
- [ ] pre.5 leanSource digest화 — **deep_verify 충돌로 보류.** 전수 조사:
      `deep_verify.rs::reverify_lean_event`(accepted 증명 offline 재실행, ADR-0007
      audit 증거)가 원문 leanSource 소비 + `bounty_proof_audit_persists_lean_source_and_verifier_hash`/
      `state_verify_deep_lean_cli` 테스트가 원문 존재 단언. plan non-goal도
      "원문 별도 아카이브=후속 결정"으로 이미 defer. **원문 아카이브 설계 시
      재검토** — 그때까지 deep_verify audit 재실행 온전 유지. EXECUTION-ORDER
      [3] + master todo §pre.5 배너에 결정·근거·후보방향 기록.

## Review
- **결과**: N0-pre는 pre.5(보류) 제외 전부 닫힘. 5개 slice 모두 full gate
  `self-test: PASS`(cargo-fmt/clippy/clippy-dev/lean-checker-build/runtime-smoke-all
  6/6/proof-to-block 7케이스·17블록·replay-fail 0·divergence 0/gitleaks green),
  양 배치 CI green, working tree clean, HEAD=origin/main=`bf00e26`.
- **게이트 이슈**: batch1 gate가 cargo-fmt에서 1회 fail(신규 `candidates` vec
  포맷) → `cargo fmt` 자동수정 후 재실행 PASS. 교훈: 파이프 `... | tail`은
  self-test.sh exit code를 가리므로 exit는 별도 캡처(`; echo EXIT=$?`).
- **방향 검증 성과**: pre.5를 plan 문구대로 바로 구현하지 않고 소비처 전수
  grep → deep_verify 충돌 발견 → 설계 결정으로 사용자에 상신. "audit-before-
  reimplement" 패턴(lessons) 재확인.
- **추천 다음**: N0-pre 닫힘 → mainline은 N3. N3.2(untrusted peer 입력) binding
  선행 = ADR-0008(lean-runner 커널 격리) 결정(seccomp/Landlock 범위·macOS
  자세·기본값) — "논의 후 결정" 성격이라 사용자 합의 후 TDD 착륙.
