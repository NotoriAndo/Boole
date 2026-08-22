# 문서 정직성 정정 — L1 적합성 리뷰 실행권고 1 (2026-07-03)

`local-docs/l1-fitness-review-2026-07-03.md` 실행권고 1 이행: 마스터플랜
(`local-docs/todo/todo-l1-network-master.md`, operator-internal)의 "완성"
라벨 2건 — evidence-backed replay / N0.4 `deep_verify_block` — 이 실제
배선 상태보다 앞서 있어 정정. 기술 실사 전 자체 발견·자체 정정을 신뢰
자산으로 기록. **closed-local 문서 작업 — public/API benchmark claim 아님.**

## 정정 전 코드 재확인 (직접 grep, 2026-07-03)
- [x] `deep_verify_block`(boole-node `deep_verify.rs`) 호출자 전수 grep →
      `tests/deep_verify_block_roundtrip.rs`뿐. 노드 런타임/CLI 진입점 0.
      `boole state verify --deep`(boole-cli `main.rs`)은
      `deep_verify_bounty_events`(bounty 원장 전용)에만 연결.
- [x] `replay_evidence.rs::verify_selected_share_evidence` 첫 가드:
      `selected_share_evidence.is_empty() → Ok(())` — 빈 evidence면
      PoW/점수/커널 재검증 전체 스킵. 빈 evidence 금지 불변식 부재.

## 정정 내용 (gitignored 마스터플랜 — 파일 자체는 커밋 대상 아님)
- [x] baseline 표 "evidence-backed replay: 완성" → **evidence-optional**로
      철회 + 표 아래 정정 배너(근거 함수/경로 인용)
- [x] §N0 canon path summary에 2026-07-03 갱신 주석 — `deep_verify_block`
      신설됐으나 CLI/노드 런타임 미배선
- [x] §N0 closure에 정정 배너 — "persisted block이 real Lean으로
      deep-verify"는 테스트 하네스 한정, 오퍼레이터 실행 경로 없음.
      "§2 invariant 2 라이브 실존"은 재검증 가능성(persisted 필드 충분)로
      한정해 읽음. 배선 주장은 CLI/노드 배선 착륙 전까지 금지.
- [x] 미변경 확정: ADR-0007(설계 기록 — 배선 완료 주장 없음), tracked
      `docs/replay-consensus.md`(legacy/no-evidence 경로 이미 명시 — 정직).

## Review
- **결과**: 마스터플랜 정정 4곳(표 라벨 1 + 배너 3). 정정문은 전부 코드
  직접 재확인(위 grep) 근거로 작성 — 리뷰 문서 인용만으로 쓰지 않음.
- **게이트**: docs-only tier — `scripts/docs-smoke.sh` + `git diff --check`.
  (마스터플랜은 gitignored라 이 기록 파일만 커밋.)
- **추천 다음**: 리뷰 실행권고 2 — N3 slice 스펙에 4건(라이브
  `deep_verify_block` 배선 / evidence-필수 replay / 블록 선택순서 재유도 /
  ts 앵커) 명시 편입. N3 스펙 변경은 "논의 후 결정" 성격 — 사용자 합의 후
  착륙.

---

# 2026-07-04 — 배치 A+B 일괄 실행 (외부 감사 후속, 텔레그램 승인 "추천안으로 바로 실행")

## 승인/결정 (완료)
- [x] pre.6·TB.2·TB.3·TB.4(b) binding 승인 기록 — L1 master + EXECUTION-ORDER 양쪽 갱신
- [x] pre.1 legacy 정책 = 권장안 / pre.3 ts 규칙 = 강화안(median-time-past) — 스펙에 결정 확정 표기
- [x] ADR-0009 amendment (pre.1/pre.3 결정 기록)
- [x] ADR-0013 초안 (checker soundness boundary, Proposed — grill 리뷰 대기)
- [x] TB.4(b) relabel — external-review-brief §1.2/§6 (v1-lenbound = seed-derived template 명시)
- [x] TB.3↔N4-pre.1 교차참조 + 3-노드 데모 트리거 보완 (사전 지시분)

## slice 구현 (worktree 멀티에이전트, wf_3b67ec5d-04f)
- [x] N3-pre.1 evidence-필수 replay (consensus) — **머지 b64eb4a (PR #11 rebase)**
- [x] N3-pre.2 canonical 선택 재유도 (consensus, pre.1 직후) — **머지 d436566 (#12→#11 스택)**
- [x] N3-pre.6 AmbiguousProposer tie-break (consensus, pre.2 직후) — **머지 ccf7bfc (#16→#11 스택)**
- [x] N3-pre.3 block ts 규칙 median-time-past (consensus) — **머지 8e8e5a1 (PR #14)**
- [x] N3-pre.4 deep_verify CLI 배선 (production) — **머지 86d223c (PR #8)**
- [x] N3-pre.5 proof-dedup /ready 필수화 (production) — **머지 94f74b9 (PR #15/#13)**; 선결로 faucet smoke 401 기존 결함 수리 b4ef112
- [x] TB.2 bounty problem_hash 바인딩 (production) — **머지 91e0ae7 (PR #10)**
- [x] TB.3 proof_bridge canon 정규화 (consensus-adjacent) — **머지 6222c8d (PR #9)**

## 착륙 후 (메인 세션)
- [x] 전 PR merge 확인 + origin/main 검증 — 8 slice + faucet 수리 전부 main, 최종 조합 push CI green (ccf7bfc success)
- [x] L1 master closure 박스 체크 + 착륙 SHA 기록 (§N0 closure "배선 주장 금지" 해제, baseline 표 evidence-optional 정정 해소 포함)
- [x] tasks/todo.md Review 섹션 + 최종 텔레그램 보고 (SHA/CI 링크/claim boundary)

## Review

- **결과**: 배치 A+B 9건 전부 main 착륙 — slice 8건(N3-pre.1~6 + TB.2 + TB.3) + 선결 수리 1건(faucet smoke 401, `b4ef112`). N3-pre wave 닫힘(N3.3 선결 충족), TB는 TB.1(ADR-0013 대기)만 잔여. 최종 SHA: pre.1 `b64eb4a` / pre.2 `d436566` / pre.3 `8e8e5a1` / pre.4 `86d223c` / pre.5 `94f74b9` / pre.6 `ccf7bfc` / TB.2 `91e0ae7` / TB.3 `6222c8d`.
- **게이트**: 전 slice TDD RED 실증 → focused green → 티어별 게이트(consensus는 runtime-smoke-all + proof-to-block-benchmark 로컬 직접 확인) → PR별 CI green → **main push 최종 조합 CI green** (run: ccf7bfc success). 스택 체인(pre.1→2→6)은 PR #11 rebase 머지로 커밋별 메시지 보존.
- **부수 수확**: main 기존 결함 1건 발굴·수리(faucet smoke 401, ecaa7c0부터 잠복 — CI 밖 게이트 스크립트 부패). GitHub CI 트리거 드랍 1회(수동 dispatch로 우회), 공유 캐시 오염으로 인한 가짜 컴파일 에러 확인.
- **lessons 기록 3건**: ① 에이전트 커밋 게이트에 CI 선두 게이트(fmt+clippy) 원문 포함 + worktree별 개별 CARGO_TARGET_DIR ② 비보호 base 스택 PR의 auto-merge 즉시발동 특성과 landing PR rebase 머지 규칙 ③ CI 밖 게이트 스크립트는 부패 의심 + baseline 재실행으로 원인 귀속 후 수리-선행 slice화.
- **관찰 항목(비차단)**: pre.1 에이전트가 로컬에서 `state_verify_deep_reverifies_persisted_blocks_with_real_lean` 실패를 main 기준으로 관찰 보고 — CI 클린 러너에선 86d223c 이후 전 run green이라 로컬 부하/캐시 요인 추정. 재발 시 조사.
- **claim boundary**: 전부 closed-local 검증 + CI. public mining/유료 API/leaderboard claim 아님.

---

# 2026-07-05 — TB.1 checker soundness boundary (ADR-0013 grill → 착륙)

- [x] ADR-0013 grill 리뷰 (텔레그램) — L1 적합성 도전 + 3공리 범위 도전 문답 후 전부 승인.
      확정: 3공리 allowlist(propext/Classical.choice/Quot.sound) / 감사는 제출 소스가
      영향 못 주는 분리 프로세스 / blacklist 확장은 보조 방어 / 격리 enforce는 결정 4
      개정으로 ADR-0008 자체 slice로 분리(N3.2 전 binding 유지)
- [x] TB.1 구현 착륙 — **7c4c743** (PR #18, CI green + main push CI green).
      RED 4종(addDecl 공리 주입 / custom elab IO / debug.skipKernelTC / 허용 밖 공리
      의존) 사전 실패 확인 → GREEN, v1-lenbound 정상 경로 수락 positive 테스트 동봉.
      audit = `BooleCheck/Audit.lean` 2차 `lake env lean --run` 프로세스.
      checker_artifact_hash 재고정 + 의존 fixture/README pin 전체 재생성.
      smoke: runtime-smoke-all + proof-to-block-benchmark PASS.

## Review
- 외부 감사(2026-07-04) critical/high 편입분 전부 착륙 완료: N3-pre.1~6 + TB.1~TB.3
  + TB.4(경로 b). §TB wave 닫힘. claim boundary 갱신: bounty 레인은
  "commissioned-statement-bound, axiom-bounded machine check" 표현 가능,
  verify-answer 레인은 D3 전까지 "문자열 검사" 표기 유지.
- 명시적 이연 잔여: ADR-0008 격리 enforce slice(N3.2 전 binding) / replay 진실 갭
  (N3.3 ingress 재검증) / TB.4 경로 a(D2 결합).
- closed-local 검증 + CI only. public mining/유료 API claim 아님.

---

# 2026-07-05 — ADR-0008 kernel isolation slice (EXECUTION-ORDER [9]) 착륙

- [x] 격리 코드 착륙 — **b405a49** (PR #20, log 모드 기본). cfg-gated:
      Linux = seccomp(egress deny denylist 11종) + Landlock(FS 격리),
      macOS = Seatbelt 프로필. IsolationMode::Log 기본(enforce는 N3.2 전환),
      enforce-capable + 가드 3종(egress / write-outside-scratch /
      non-toolchain-exec)으로 실제 차단 증명.
- [x] 신규 deps `landlock` 0.4.5 + `seccompiler` 0.5.0 (cfg-linux) —
      cargo-deny/audit 공급망 게이트 통과, 버전 핀.
- [x] Linux 전용 회귀 CI 발굴·수리 — **d20bb72**. Landlock의 Execute 권한이
      ELF 인터프리터(동적 로더)에도 적용돼 execve 실패(EACCES). 로더 +
      표준 공유 라이브러리 디렉토리(/lib·/lib64·/usr/lib·multiarch)를
      Execute 허용목록에 추가. 프로덕션 관련 수정(lake/lean도 동적 링크).
      landlock 크레이트 자체 예제가 동일 요구 확인. CI 1회 왕복으로 수렴.

## Review
- ADR-0008 격리막이 log 모드로 착륙. main 안전(기본 log라 실제 검사 경로
  smoke green), enforce 전환은 N3.2에서 신뢰 경계 개방과 동시(ADR 결정 4).
- **N3.2 전 잔여 (binding)**: ① macOS Seatbelt 가드 CI 미검증 — ci.yml에
  ubuntu-latest만, macOS 러너 없음(ADR 결정 3 미충족). macOS 러너 잡 신설 vs
  ADR 개정(로컬-검증-한정 인정)은 사용자 결정 대기. ② N3.2 커밋에서 enforce
  기본 전환 + opt-out 플래그.
- 개발 머신이 macOS라 Linux 경로는 CI 검증 의존 — 착수 때 명시한 리스크가
  실제 CI 실패로 나타났고 1회 왕복으로 수리(lessons: 로컬 미검증 플랫폼
  코드는 CI 왕복 전제, log 모드 착륙이 그 리스크를 흡수).
- closed-local 검증 + CI only. public mining/유료 API claim 아님.

---

# 2026-07-06 — N3.2 share gossip (egress + ingress re-admit) + ADR-0008 enforce 전환

텔레그램 지시 "N3.2 시작해" (chat 1311067056). spec: L1 master §N3.2 +
EXECUTION-ORDER [9] 잔여 ②(enforce 기본 전환 + opt-out — ADR-0008 결정 4,
네트워크 ingress 개방 커밋과 결합). closed-local 검증 + CI only —
public mining/유료 API claim 아님.

## 선결 확인
- [x] N3.1 transport 착륙 (a7aae0c, PR #26) — boole-p2p crate 존재
- [x] ADR-0008 격리 log 모드 착륙 (b405a49) + macOS canary (dd764be) —
      N3.2 앞 binding 잔여는 enforce 전환뿐(이번 slice 범위)
- [x] N3-pre wave 닫힘 — N3.2와 병렬 안전 항목 전부 착륙

## slice 계획
- [x] 코드 탐색 (Explore 3: boole-p2p surface / node admission·submit 경로 /
      isolation enforce surface)
- [x] RED: `crates/boole-node/tests/p2p_share_propagation.rs` —
      컴파일 에러 확인(serve_local_node_with_p2p/P2pConfig 부재, N3.1 RED 관행)
      + reject-path 2종(비allowlist drop / Hello network_id mismatch) 동봉.
      enforce RED: `config_records_verifier_hash` Enforce 기대로 수정 →
      Log!=Enforce 실패 확인
- [x] GREEN: `p2p_egress.rs`(admit+dedup 통과 share announce, Hello 상호검증,
      실패는 카운터로) + `p2p_ingress.rs`(allowlist→Hello 검증→동일
      `admit_parsed_submission_typed` 재admit — 두 번째 검증 정책 금지,
      HTTP 경로와 같은 단일 write guard 안에서 admit+dedup peek) +
      `--p2p-listen`/`--peer` CLI + typed drop 카운터 /metrics 노출.
      비목표 준수: ingress는 블록 생성/전파 안 함(N3.3), relay 없음
- [x] enforce 전환: IsolationMode 기본 Log→Enforce + opt-out 플래그
      `--allow-isolation-log-mode`(run-local/submit-lean), 기본값 테스트 갱신
      + LeanBountyVerifier 배선 테스트 신설
- [x] focused gate: gossip 3/3 + lean-runner --lib 26/26(RUST_TEST_THREADS=1,
      Seatbelt enforce 가드 포함) + node --lib 40/40 + real_checker 4/4
      (실제 lake가 Enforce 아래 첫 검증 — green)
- [x] 커밋 게이트 (consensus 티어): cargo fmt --all --check PASS +
      clippy 2종(-D warnings, dev-features 포함) PASS +
      runtime-smoke-all ok:true 6/6 + proof-to-block-benchmark ok:true 7/7
      (replayFailures 0, invalidAccepted 0) — 전부 Enforce 기본값 아래 실행
- [x] NotoriAndo author 커밋 → feature branch push → PR #27 → CI green
      (self-test + supply-chain + macOS isolation canary) → rebase 머지
      → remote 검증 (main `a78482e`, 코드 커밋 `152ab5b`, local==origin)
- [x] L1 master §N3.2 착륙 기록 + EXECUTION-ORDER [9] 완전 종결/[10] 갱신
      (local-docs, gitignored) + 텔레그램 최종 보고

## Review
- **결과**: N3.2 착륙 — 두 노드가 share를 gossip으로 주고받고, 받은 노드는
  로컬 HTTP 제출과 완전히 같은 admission 경로(`admit_parsed_submission_typed`
  + N2.3 dedup peek, 같은 단일 write guard)로 재승인. 두 번째 검증 정책
  없음(ADR-0009 (e)). ingress는 블록 생성/relay 안 함(N3.3 비목표 준수 —
  테스트가 B height==0을 고정). `--p2p-listen`/`--peer`(inbound IP allowlist
  겸용), Hello(protocol_version/network_id/genesis_hash) 상호검증, typed
  drop/outcome 카운터 8종 /metrics 노출.
- **ADR-0008 결정 4 이행**: IsolationMode 기본 Log→**Enforce**를 네트워크
  ingress 개방과 같은 커밋에 동승 + `--allow-isolation-log-mode`
  (run-local/submit-lean) opt-out. 실제 lake/lean이 Enforce(Seatbelt) 아래
  첫 실행 green — real_checker 4/4, 클린 macOS 러너 canary도 green.
- **게이트**: RED 2건 실증(컴파일 에러 + Log!=Enforce assert) → GREEN.
  focused: gossip 3/3 + lean-runner 26/26 + node lib 40/40 + real_checker
  4/4. consensus 티어: fmt/clippy 2종 로컬 재현 PASS + runtime-smoke-all
  ok:true 6/6 + proof-to-block-benchmark ok:true 7/7(replayFailures 0,
  invalidAccepted 0) — 전부 Enforce 기본값 아래 실행 로그 직접 확인.
  PR #27 CI 3 job green 후 rebase 자동 머지, 커밋별 메시지 보존.
- **설계 노트**: LocalNodeConfig 무변경(신규 P2pConfig 파라미터 +
  `serve_local_node_with_p2p` 진입점 — 기존 테스트 ~58개 literal 무churn,
  2026-06-04 lesson 적용). egress는 admit+dedup 통과 후에만 announce,
  ingress는 재announce 안 함(2~3 peer full mesh라 relay 불필요 — loop
  구조적 불가). per-peer ingress rate limit은 admission rate limiter를
  peer IP로 재사용(ADR-0009 (c) presence 충족, 별도 한도 튜닝은 N3.3+).
- **claim boundary**: closed local 검증 + CI only. public mining/유료
  API/leaderboard claim 아님.

---

# 2026-07-05 — ADR-0008 [9] macOS-CI 갭 종결 (제3안: 좁은 canary)

- [x] 사용자 결정 (텔레그램) — 3안 중 제3안 채택: 전체 macOS 러너(비용 10배) 도, ADR 개정(canary 상실) 도 아닌 **격리 가드 전용 좁은 macOS CI 잡**.
- [x] `.github/workflows/macos-isolation.yml` 신설 — `cargo test -p boole-lean-runner --lib`를 macos-latest에서, path-filter(boole-lean-runner + 이 워크플로 변경 시에만). 필수 체크 아님(path-filter라 required로 걸면 무관 PR이 hang). 착륙 **dd764be (PR #22)**.
- [x] ADR-0008 개정 — macOS-CI 갭을 이 canary로 종결 기록(헌법 §13: 불변량 유지, 실행만 최적화).
- [x] canary 첫 실행이 실제 취약점 즉시 발굴 — `cargo test --lib`가 sibling `sandbox_probe` bin을 안 빌드해 클린 러너에서 셋업 assert 실패(4/4). 워크플로에서 probe 선빌드로 수정(3b75447). 이후 macOS 가드 4종 GitHub 러너 실제 통과(26 passed) 확인 후 머지.

## Review
- ADR-0008 [9] macOS-CI 잔여 종결. **[9] 남은 것은 N3.2 enforce 기본 전환(결정 4) 하나뿐** — 이는 네트워크 개방 커밋과 묶는 명시적 이연분(설계상 지금 하면 안 됨).
- 외부 감사(2026-07-04) 후속 트랙 전체 정리: N3-pre.1~6 + TB.1~4(b) + ADR-0008 격리 slice(log 모드) + macOS canary. 잔여는 전부 명시적 이연(N3.2 enforce, N3.3 replay 진실, TB.4-a D2).
- closed-local 검증 + CI only. public mining/유료 API claim 아님.

---

# 2026-07-06 — N3.3 block announce + linkage-checked ingest (+ per-peer rate limit 튜닝)

텔레그램 지시 "N3.3 시작해, per-peer rate limit 수치 튜닝도 묶어서"
(chat 1311067056). spec: L1 master §N3.3 + ADR-0009 (c) per-peer ingress
rate limit 기본값 튜닝(N3.2에서 명시 이연분). closed-local 검증 + CI only —
public mining/유료 API claim 아님.

## 선결 확인
- [x] N3-pre wave 6건 전체 닫힘 (2026-07-05) — N3.3 착수 binding 충족
- [x] N3.2 착륙 (152ab5b, PR #27) — p2p ingress/egress 뼈대 + enforce 전환 완료

## slice 계획
- [x] 코드 탐색 (Explore: PersistedBlock/FileBlockStore/replay 검증 집합/
      runtime 적용 경로/reward ledger 정합/HttpRateLimiter API)
- [x] RED: `crates/boole-node/tests/p2p_block_propagation.rs` — 컴파일 에러
      (P2pConfig.rate_limit_per_60s/ingest API 부재) 확인
- [x] GREEN: egress BlockAnnounce(commit 시, announce/pull — 본문은 Blocks
      프레임으로만) + ingress: head+1 확장 확인 → GetBlocks pull → 검증은
      strict replay 경로 재사용(evidence-필수·canonical 재유도·median-time-past
      + future-drift 경계 가드; LegacyEvidenceOptIn 구조적 접근 불가) →
      commit과 동일 쓰기 순서로 append(블록→reward ledger→적용→bounty rows→
      dedup 미러). head 수렴 + 위조(evidence-less) 블록 거절 테스트 3/3 green.
      reorg/fork-choice 없음(N4 비목표) — head+1 아닌 announce는 ignored 카운트
- [x] per-peer rate limit: ingress에 IP별 60초 창 프레임 한도 기본 600
      (HttpRateLimiter 재사용, 연결 넘나들며 지속 — 재접속으로 리셋 불가),
      `--p2p-rate-limit-per-60s` 튜닝 플래그(0=해제), 초과 시 typed drop
      카운터 + 연결 종료. flood 테스트 green
- [x] consensus 티어 게이트: fmt --check PASS + clippy 2종(-D warnings,
      dev-features 포함) PASS + runtime-smoke-all ok 6/6 +
      proof-to-block-benchmark ok 7/7(replayFailures 0) 로컬 직접 확인
- [x] 커밋 → PR #29 → CI 1라운드 실패(python 계약 테스트 — submit_json
      bounty append 헬퍼 추출로 정적 미러 어긋남) → 미러 갱신 + 로컬 전체
      python-script-tests 186 OK 재현 → 2라운드 CI green(self-test +
      supply-chain) → rebase 자동 머지 → remote 검증
      (main `fffe165`, 코드 `c7e66c4`, local==origin)

## Review
- **결과**: N3.3 착륙 — A가 만든 블록이 peer B에 announce/pull로 전달되고,
  B는 strict replay 경로(evidence-필수·canonical 재유도·median-time-past·
  hash 재유도 + future-drift 경계 가드)를 그대로 재사용해 검증한 뒤에만
  저장. byte-identical head 수렴을 테스트로 고정. head+1 확장만 수용
  (reorg/fork-choice = N4 비목표). 위조(evidence-less) 블록 거절 테스트로
  N3-pre.1 truth boundary가 gossip ingest에 실제 작동함을 입증.
- **rate limit 동봉(사용자 지시)**: ADR-0009 (c) 잔여 — peer IP별 60초 창
  600프레임 기본(HttpRateLimiter 재사용, 연결 재접속으로 리셋 불가),
  `--p2p-rate-limit-per-60s` 튜닝(0=해제), 초과 시 연결 종료 + typed 카운터.
  flood 테스트 green.
- **정합성 설계**: ingest 쓰기 순서 = 자체 커밋과 동일({check, append,
  reward-append, apply, cache}) + bounty rows + N2.3 proof-dedup 미러 —
  재부팅 시 원장-replay 대조 검증이 그대로 통과. 합의-레벨 dedup(N4-pre.1,
  ADR-0012)은 건드리지 않음(노드-로컬 운영 원장 미러만).
- **게이트**: RED(컴파일 에러) 실증 → GREEN 3/3 + N3.2 gossip 3/3 +
  node lib 40/40. consensus 티어: fmt/clippy 2종 + smoke 2종(6/6, 7/7,
  replay 실패 0) 로컬 green. CI: 1라운드 python 계약 테스트 실패 →
  원인은 헬퍼 추출에 따른 정적 소스-구조 미러 어긋남(의미상 순서 동일),
  미러를 헬퍼 추출을 따라가게 갱신(+헬퍼 본문 내 credit→share_promoted
  순서 pin 신설) 후 2라운드 green. lessons에 재발 방지 규칙 기록
  (consensus-adjacent 함수 리팩토링 전 scripts/*.py grep + 로컬 python
  스테이지 실행).
- **claim boundary**: closed local 검증 + CI only. public mining/유료
  API/leaderboard claim 아님.

---

# 2026-07-06 — N3.4 initial sync (GetBlocks/Blocks)

텔레그램 지시 "N3.4 진행해" (chat 1311067056). spec: L1 master §N3.4.
closed-local 검증 + CI only — public mining/유료 API claim 아님.

## slice 계획
- [x] RED: `crates/boole-node/tests/p2p_initial_sync.rs` — src stash로 기능
      부재 상태 재현, 2테스트 모두 행동 실패(타임아웃) 확인 후 복원
- [x] GREEN: ① ingress가 GetBlocks를 블록 캐시에서 서빙(Blocks 응답, 범위
      상한은 코덱 검증 재사용) ② sync 스레드 — Hello 교환으로 peer head 파악
      → 뒤처진 범위를 256블록 페이지로 pull → 블록마다 N3.3
      `ingest_announced_block` 재사용(검증 정책 추가 없음) → 동일 head 수렴.
      부팅 직후 1회 + 5초 주기 재확인(announce 누락 gap 보정). 위조 체인은
      블록 단위 거절 + sync 중단(테스트 고정). 테스트 2/2 green
- [x] 테스트 하네스 교훈 2건: multiminer fixture는 dedup-공격용(같은 proof
      bytes)이라 dedup 원장 켠 채 2블록 체인 구축 불가 → 원장 없이 부팅 /
      미리 바인딩한 리스너 백로그로 announce가 "부팅 전" 전제를 무효화 →
      A egress를 dead peer로 차단해 sync 경로만 남김
- [x] 회귀: N3.2 3/3 + N3.3 3/3 + lib 40/40. consensus 티어: fmt --check
      PASS + clippy 2종 PASS + runtime-smoke-all 6/6 +
      proof-to-block-benchmark 7/7(replayFailures 0) 로컬 직접 확인.
      scripts/*.py 미러 grep 사전 확인(해당 없음 — N3.3 lesson 적용)
- [x] 커밋(`3048bdf` 코드 + `79185a8` 기록) → PR #31 → CI 1회 green
      (self-test + supply-chain) → rebase 자동 머지 → remote 검증
      (main `79185a8`, local==origin, tree clean)

## Review
- **결과**: N3.4 착륙 — 빈 노드가 peer의 head를 Hello 교환으로 파악하고
  뒤처진 범위를 GetBlocks(256블록 페이지, wire 상한)로 내려받아 블록마다
  N3.3 검증-후-수용 루프를 그대로 통과시켜 동일 head까지 복원. 서빙 쪽
  (GetBlocks → 블록 캐시 응답)도 함께 착륙. 부팅 즉시 1회(N5.3 node-join의
  기반 경로) + 5초 주기 재확인으로 announce 누락 gap도 자가 보정.
- **신뢰 경계**: 위조(evidence-less) 체인을 서빙하는 peer는 블록 단위로
  거절되고 그 sync 라운드가 중단됨 — fresh 노드가 위조 체인을 채택하지
  않음을 테스트로 고정. 검증 정책 추가 없음(strict replay 재사용).
- **TDD 정직성**: 최초 RED 실행이 병행 편집의 컴파일 에러와 섞여서, src만
  stash해 기능 부재 상태를 재현한 행동 RED(2테스트 타임아웃)를 별도 증명.
- **하네스 교훈 2건**: ① multiminer fixture는 N2.3 dedup-공격용(같은 proof
  bytes 반복)이라 dedup 원장을 켠 채 다블록 체인을 만들 수 없음 ② 테스트가
  미리 바인딩한 p2p 리스너는 노드 부팅 전에도 OS 백로그로 연결을 받아
  "부팅 전 announce 불가" 전제를 무효화 — dead-peer allowlist 구성으로
  sync 경로만 분리 검증.
- **claim boundary**: closed local 검증 + CI only. public mining/유료
  API/leaderboard claim 아님.

---

# 2026-07-06 — N3.5 3-peer convergence smoke (gate 배선, N3 wave 마지막)

텔레그램 지시 "N3.5 진행해" (chat 1311067056). spec: L1 master §N3.5.
closed-local 검증 + CI only — public mining/유료 API claim 아님.

## slice 계획
- [x] RED: `test_self_test_contract.py`에 p2p-convergence 스테이지 + smoke
      스크립트 계약 2테스트 선추가 → 스크립트/배선 부재로 2건 실패 확인
- [x] GREEN: `scripts/p2p-local-convergence-smoke.sh` 신규 — 노드 3개
      (ephemeral 포트, full-mesh --peer), share를 노드1·노드2 두 곳에 주입,
      셋 다 동일 head(높이 2) + replayMatchesRuntime 전원 true(발산 0) 폴링
      검증, JSON 요약 출력(claim boundary 명시). self-test.sh에
      run_capture_json p2p-convergence 스테이지 + 요약 JSON check 추가
- [x] 게이트: smoke 단독 2회 green(--locked 반영 후 재확인) + python
      스테이지 전체 OK + self-test 요약 파이썬 모의 실행 OK + bash -n +
      docs-smoke + git diff --check. Rust 무변경(scripts-only)
- [x] PR #33 → CI green — 신규 p2p-convergence 스테이지가 클린 ubuntu
      러너에서 첫 실행 통과(self-test + supply-chain) → rebase 자동 머지 →
      remote 검증(main `d43ad9e`, 코드 `a382c70`, local==origin, tree clean)
- [x] N3 closure 기록 — L1 master §N3 closure 박스 7항목 전부 체크(N3 wave
      완료), EXECUTION-ORDER [10] 갱신(다음 = N4-pre.1)

## Review
- **결과**: N3.5 착륙으로 **N3 wave(minimal P2P) 전체 마감** — 독립 실행
  노드 3개가 static peer 구성으로 share/블록을 주고받아 같은 replayable
  체인으로 수렴(S7 목표). 수렴 여부는 이제 사람 판단이 아니라 self-test/CI가
  매 커밋 기계적으로 지키는 게이트(p2p-convergence 스테이지)가 됨.
- **게이트**: 계약 테스트 선추가 RED(2건 실패) → GREEN. smoke 로컬 2회
  green(동일 head 높이 2, replay 발산 0) + python 스테이지 전체 OK + 요약
  파이썬 모의 실행 사전 검증 + bash -n. scripts-only 변경이라 Rust 게이트
  불필요. CI 1회 green — 신규 스테이지의 실제 첫 클린 러너 실행 포함.
- **N3 wave 결산**: N3.0(ADR-0009) → N3-pre 6건 → N3.1 transport →
  N3.2 share gossip(+ADR-0008 enforce 전환) → N3.3 block ingest(+rate
  limit) → N3.4 initial sync → N3.5 수렴 게이트. 전부 TDD RED 실증,
  전 slice CI green, 커밋별 rebase 머지로 이력 보존.
- **claim boundary**: closed local 검증 + CI only. public mining/유료
  API/leaderboard claim 아님.
- **wave 완료 지표(비게이트, pre-mortem U00/PM.2)**: 유료 검증 구매자/LOI
  수: 0.

---

# 2026-07-06 — N4-pre.1 합의-레벨 proof dedup (ADR-0012 구현)

텔레그램 지시 "N4-pre.1 진행해" (chat 1311067056). spec: L1 master
§N4-pre.1 + ADR-0012(Accepted 2026-07-03). N4.1 착수 전 binding 게이트.
closed-local 검증 + CI only — public mining/유료 API claim 아님.

## 선결 확인
- [x] N3-pre.1 evidence-필수 replay 착륙 (b64eb4a) — canon_hash 재유도 입력
- [x] TB.3 canon 정규화 착륙 (6222c8d) — dedup 키 안정성 선결
- [x] ADR-0012 전 항목 Accepted (2026-07-03 grill)

## slice 계획
- [x] 탐색 — 핵심 발견: runtime-smoke 계열 fixture 5개 전부(v1/restart/
      three-block/retarget/multiminer)가 한 증명 bytes를 전 step 재사용 →
      새 규칙 아래 다블록 체인 전부 위법. 단, 하드코딩 head 없음(step 1+는
      전부 cFromRuntimeHead) → bytes만 교체하면 됨
- [x] RED: replay 2종 행동 RED(중복 체인이 현재 replay 통과 확인) +
      builder 1종(신규 파라미터) — 양성 대조(distinct 수락) 동반
- [x] GREEN: replay 체인 순서 BTreeSet + typed 거절(재유도 canon_hash 키,
      verify_selected_share_evidence 이후 실행으로 (c) 결정 충족; legacy
      evidence-less 예외) + build_block_selection credited 셋 파라미터
      (이미 보상된 share 선택 전 제외 + 블록 내 중복은 preselection 순서
      첫 것만 유지) + runtime이 block_cache에서 셋 재유도. 전용 4/4 green
- [x] N2.3 원장 doc 강등 (proof_dedup_ledger.rs — "admission early-reject
      cache, not the source of truth")
- [x] fixture 정합: 5개 fixture step별 distinct bytes(v1 package의 expr
      payload u32만 수술) + N2.3 테스트는 중복을 테스트 안에서 위조 +
      p2p/smoke 낡은 주석 갱신 + 기존 co-qualifying 테스트의 부수적 중복
      package 수리(회귀 1건)
- [x] consensus 티어 게이트: boole-core 전체 green + node lib 40/40 +
      N2.3 2/2 + p2p 8/8 + fmt/clippy 2종 + runtime-smoke-all 6/6 +
      proof-to-block-benchmark 7/7(blocksProduced 17 보존, replayFailures 0)
      + 3-peer convergence smoke green + python 계약 테스트 OK
- [x] 커밋 → PR #35 → CI green → rebase-merge → remote 검증 → L1 master
      착륙 기록 → 보고

## Review
착륙 완료 (2026-07-07). PR #35 rebase-merge, main = `67d0c25`. 코어 규칙
커밋 `f43256d` (`core: enforce chain-wide proof dedup as a consensus rule`).
동봉 3커밋: `df8431d`(진행 기록) · `f1eb3b6`(reward/bounty heal 블록 distinct
proof) · `67d0c25`(runtime 다중-커밋 테스트 distinct proof). 전부 NotoriAndo
author.

무엇을 했나 (쉬운 말): "한 번 보상받은 증명은 체인 어디서도 다시 보상받지
못한다"를 replay가 블록 데이터만으로 재유도·강제하는 합의 규칙으로 만들었다.
이전엔 각 노드의 로컬 장부 파일이 중복을 막았고 파일을 지우면 우회됐는데,
이제는 중복 보상이 든 블록 자체가 모든 노드에서 가짜 판정된다. N4.1
fork-choice 착수 전 binding 게이트가 이걸로 풀렸다.

검증:
- focused: consensus_proof_dedup 4/4 · boole-core 전체 green · node lib 40/40
  · N2.3 2/2 · p2p 8/8 · runtime-smoke-all 6/6 · proof-to-block-benchmark
  7/7(blocksProduced 17 보존, replayFailures 0) · 3-peer convergence green
- CI: self-test pass 8m0s + supply-chain pass 3m15s (PR #35)
- working tree clean, origin/main == local HEAD == `67d0c25`

CI 반송 2라운드 (교훈 적재): (1) 테스트 body-reuse 4건 — 새 규칙이 한 template
body 복제 다블록 커밋을 무효화 → 각 후속 블록에 distinct POFP payload 부여.
(2) round-2 cargo-fmt(14s) — 단일-파일 amend를 fmt 게이트 없이 force-push.
lessons.md 2026-07-07 항목에 재발 노트로 강화.

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

# 2026-07-07 — N4.1 체인 누적 작업량 (fork-choice weight primitive)

텔레그램 지시 "추천작업진행해" (chat 1311067056). spec: L1 master §N4.1.
N4-pre.1 게이트 해소 후 N4 wave 첫 슬라이스. closed-local + CI only.

## slice 계획
- [x] 탐색 — PersistedBlock.difficulty_weight 필드 형식 확인: 핵심 발견은
      이 값이 `difficulty_weight(t_block).to_string()` = BigUint Display =
      **10진수** 문자열이라는 것(hex 아님). spec 초안의 parse_biguint_hex
      제안은 오독 → min_share_score 파싱 관용구(parse::<BigUint>())로 결정
- [x] RED: cumulative_work 2종(heavier chain / equal-length ordering) +
      base case(empty=0, single=weight). 함수 부재 → unresolved import 실패
- [x] GREEN: 신규 fork_choice.rs — cumulative_difficulty_weight, BTree 아님
      순수 폴드(anyhow::Result, 파싱 실패 시 height 문맥 담아 전파). lib.rs
      pub mod + pub use 재수출. 전용 2/2 green
- [x] 로컬 게이트: cargo fmt --all --check clean + clippy 2종(-D warnings)
      clean + boole-core 전체 테스트 무회귀 (fork_choice는 admission/replay/
      hash/block_builder 밖 순수 추가 함수 = production 티어, full은 CI)
- [x] 커밋(`02eab79`) → PR #37 → CI green → rebase-merge(`d58e502`) →
      remote 검증 → 착륙 기록 → 보고

## Review
착륙 완료 (2026-07-07). PR #37 rebase-merge, main = `d58e502`. 코어 커밋
`02eab79`(rebase 후 `d58e502`), NotoriAndo author.

무엇을 했나 (쉬운 말): 포크(체인이 두 갈래로 갈림)가 생겼을 때 "어느 쪽이
진짜 체인이냐"를 길이가 아니라 실제로 쌓인 작업량으로 판정하기 위한 토대
함수를 만들었다. 각 블록에는 그 블록을 캐낸 난이도에 비례하는 가중치가
붙어 있는데, 체인 전체의 가중치를 더해 총 작업량을 계산한다. 아직 "선택"
규칙은 아니고(그건 N4.2), 그 선택이 딛고 설 합산 함수까지가 이번 몫.

정정 1건: 블록에 저장된 가중치가 16진수인 줄 알기 쉬운데 실제로는 10진수
문자열이었다. spec 초안대로 16진수로 읽었으면 값이 틀어졌을 것 — 코드베이스
기존 관용구(min_share_score 10진수 파싱)와 똑같이 맞췄다.

검증:
- focused: cumulative_work 2/2 (heavier / equal-length / empty=0 / single)
- 로컬 게이트: fmt clean + clippy 2종 clean + boole-core 전체 무회귀
- CI: self-test pass 8m15s + supply-chain pass 3m13s (PR #37)
- working tree clean, origin/main == local HEAD == `d58e502`

이번엔 push 전에 fmt+clippy 로컬 게이트를 먼저 돌려 CI 반송 0 (2026-07-07
재발 노트 규칙 적용 성공).

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

# 2026-07-07 — N4.2 canonical-head 선택 + 결정적 tie-break (fork-choice)

텔레그램 지시 "추천진행해" (chat 1311067056). spec: L1 master §N4.2.
N4.1(누적 작업량 합산) 위에 얹는 N4 wave 둘째 슬라이스. closed-local + CI only.

## slice 계획
- [x] 방향 검증 — N4.2는 N4.1의 `cumulative_difficulty_weight`를 소비해
      경쟁 체인 중 총 작업량 최대 head를 고르고, 정확 동률은 최저 block hash로
      결정적 tie-break. 노드 적용/reorg는 N4.3(비목표)
- [x] RED: fork_choice 2종(`selects_heaviest_chain`,
      `breaks_exact_tie_by_lowest_block_hash`). 함수 부재 → unresolved import 실패
- [x] GREEN: fork_choice.rs 확장 — `choose_canonical_head(&[Vec<PersistedBlock>])`
      단일-패스 폴드(weight 내림차순, 동률 시 hash 오름차순). head hash는 저장된
      `c`를 믿지 않고 canonical 입력(prev_c + selected_share_hashes)에서
      `block_hash`로 재유도(replay가 검증하는 그 유도). lib.rs pub use 추가.
      전용 2/2 green
- [x] 로컬 게이트: cargo fmt --all --check clean + clippy 2종(-D warnings)
      clean + boole-core 전체 테스트 무회귀 (fork_choice는 admission/replay/
      hash/block_builder 밖 순수 추가 함수 = production 티어, full은 CI)
- [x] 커밋(`5f69fcc`) → PR #39 → CI green → rebase-merge(`ba8f302`) →
      remote 검증 → 착륙 기록 → 보고

## Review
착륙 완료 (2026-07-07). PR #39 rebase-merge, main = `ba8f302`. 코어 커밋
`5f69fcc`(rebase 후 `ba8f302`), NotoriAndo author.

무엇을 했나 (쉬운 말): 체인이 두 갈래로 갈렸을 때 "어느 쪽이 진짜냐"를
실제로 고르는 규칙을 만들었다. N4.1이 만든 "체인 총 작업량 더하기"를 써서
후보 체인들 중 작업량이 가장 큰 쪽의 끝 블록을 canonical head로 고른다.
작업량이 정확히 똑같으면(아주 드문 경우) 끝 블록 해시가 더 작은 쪽을 택해
모든 정직한 노드가 같은 끝점으로 수렴하게 한다. 아직 노드에 붙여
reorg(체인 갈아끼우기)를 하는 건 아니고(그건 N4.3), 그 "선택" 규칙까지가
이번 몫.

설계 포인트: head 해시를 블록에 저장된 `c` 필드를 그대로 믿지 않고 canonical
입력(prev_c + 선택된 share 해시)에서 재유도한다 — replay가 각 블록을 검증할
때 쓰는 바로 그 유도라, "믿지 말고 다시 계산" 원칙과 tie-break가 저장값
위조에 흔들리지 않게 한다.

검증:
- focused: fork_choice 2/2 (selects_heaviest_chain / breaks_exact_tie)
- 로컬 게이트: fmt clean + clippy 2종 clean + boole-core 전체 무회귀
- CI: self-test pass 8m8s + supply-chain pass 3m12s (PR #39)
- working tree clean, origin/main == local HEAD == `ba8f302`

이번에도 push 전 fmt+clippy 로컬 게이트 선행 → CI 반송 0.

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

# 2026-07-07 — N4.3 reorg가 state를 결정적으로 재유도 (노드 적용)

텔레그램 지시 "N4.3 진행해" (chat 1311067056). spec: L1 master §N4.3.
N4.1(누적 작업량)·N4.2(canonical-head 선택) 위에 얹는 N4 wave 셋째 슬라이스 —
선택 규칙을 노드에 실제로 적용하는 첫 런타임 primitive. closed-local + CI only.

## slice 계획
- [x] 방향 검증 — 노드가 앉아 있는 체인 A에 공통 창세 prefix를 공유하는
      무거운 경쟁 체인 B(fork-choice 승리)가 들어오면, 창세부터 재유도해 잔액을
      B의 fresh replay와 byte-identical로 맞추고 재기동 후에도 동일 상태. 채택
      판단은 N4.2 `choose_canonical_head` 재사용(규칙 이중화 금지). p2p 배선은
      후속(비목표)
- [x] RED: `reorg_state_convergence` 2종
      (`reorg_to_heavier_chain_rederives_balances_byte_identical`,
      `lighter_chain_is_not_adopted`). `ReorgOutcome`/`reorg_to_heavier_chain`
      미구현 → unresolved import 실패(깔끔한 RED)
- [x] GREEN: `RuntimeAdmissionState::reorg_to_heavier_chain(block_path, candidate)`
      — ① 경쟁 체인 strict replay(legacy evidence-less 부팅 경로 미사용) ②
      채택 판단 = N4.2 `choose_canonical_head` + `head_block_hash`(pub 승격)
      재사용, 동일 tip=no-op, 더 무거운 쪽만 채택 ③ 블록 저장소+보상 장부
      원자적 스왑(신규 `durability::write_ndjson_lines_atomic`: temp→fsync→
      rename→dir fsync) ④ in-memory 캐시/head/장부/pool 후보로 재구성. 전용
      2/2 green
- [x] 로컬 게이트: cargo fmt --all --check clean + clippy 2종(-D warnings)
      clean + fork_choice 2/2·durability 8/8 무회귀 (reorg는 admission/replay/
      hash/block_builder 코어 밖 = production 티어, full은 CI)
- [x] 커밋(`d0bbfe1`) → PR #41 → CI green → rebase-merge(`885df14`) →
      remote 검증 → 착륙 기록 → 보고

## Review
착륙 완료 (2026-07-07). PR #41 rebase-merge, main = `885df14`. 코어 커밋
`d0bbfe1`(rebase 후 `885df14`), NotoriAndo author.

무엇을 했나 (쉬운 말): 지금까지는 "어느 체인이 진짜냐"를 고르는 규칙만
있었는데(N4.2), 이번엔 노드가 그 규칙에 따라 실제로 체인을 갈아끼우게 했다.
내 노드가 체인 A 위에 있는데, 같은 창세 블록에서 갈라져 나온 더 무거운 체인
B가 들어오면, 창세부터 B를 다시 재생해서 계좌 잔액을 "B를 처음부터 새로
재생한 결과"와 한 바이트도 다르지 않게 맞춘다. 그리고 이 교체가 재기동 후에도
살아남도록, 블록 저장 파일과 보상 장부 파일을 통째로 원자적으로 갈아끼운다 —
교체 도중 컴퓨터가 꺼져도 "옛 파일 전체" 아니면 "새 파일 전체"만 남고 반쪽짜리
파일은 절대 안 생긴다.

설계 포인트:
- 채택 여부 판단은 N4.2의 `choose_canonical_head`를 그대로 재사용 — reorg
  트리거와 선택 규칙이 두 벌로 갈라져 어긋나는 일을 원천 차단
- 경쟁 체인은 strict replay 진입점만 사용(부팅용 legacy evidence-less 경로
  절대 미사용) — 위조/evidence-less 후보는 거절되고 현재 체인 무변경
- 원자적 파일 교체 헬퍼(`write_ndjson_lines_atomic`)를 durability에 신설,
  reorg 중 크래시에도 반쪽 파일 없음
- 보상 장부는 블록당 1이벤트로 재유도 — 부팅 재유도 경로와 동일해 다음 부팅의
  `verify_ledger_matches_replay`가 green 유지

범위: 런타임-레벨 primitive만. 트리거를 p2p ingress/sync 경로에 배선하는 것은
후속 slice(N4.4 인근). 증분 rollback 없음(공통 조상까지 diff가 아니라 전 체인
재유도 — testnet 규모 허용).

검증:
- focused: reorg_state_convergence 2/2 (byte-identical 재유도 + 가벼운 체인
  미채택)
- 회귀: fork_choice 2/2(N4.1/N4.2) + durability 8/8
- 로컬 게이트: fmt clean + clippy 2종 clean
- CI: self-test pass 8m12s + supply-chain pass 3m9s (PR #41)
- working tree clean, origin/main == local HEAD == `885df14`

이번에도 push 전 fmt+clippy 로컬 게이트 선행 → CI 반송 0.

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

# 2026-07-07 — N4.4 invalid/equivocating peer block reject (test-only 회귀 방어)

N4 wave 마지막 slice. 텔레그램 "진행해" → 방향 검증 → "1번 진행해"(옵션 1:
test-only 회귀 방어) 승인. 스펙(N3.3 착륙 前 작성)은 `p2p_ingress.rs`
production 강화를 예상했으나, 방향 검증 결과 **N3.3이 이미 그 동작을 구현
완료** — peer 블록을 로컬과 full parity로 strict replay(PoW·linkage·hash
재유도·evidence·canonical·MTP·forged t_block) 후 실패 시 `Rejected` +
`boole_p2p_ingress_blocks_rejected_total` 증가. 따라서 잔여 실체 = 그 보장을
못 박는 회귀 테스트 1건.

## 방향 검증 (완료)
- [x] `ingest_announced_block`(local_node.rs:4675) 전수 확인 — 이미 strict
      replay full-parity 거절 + reject metric 배선, evidence-less reject
      테스트 존재. 스펙의 production 강화는 중복이므로 test-only로 축소.
- [x] 사용자에 정직 보고(취약점 아님, 이미 구현됨) → 옵션 1 선택 수신.

## slice 구현
- [x] RED→진단: 첫 위조 시도(`difficultyWeight`를 "1"로) → 거절 안 되고
      ingest됨. 진단 결과 이 시나리오는 near-max tBlock(`0xfff…ffe`)이라
      정상 가중치가 원래 "1" → 위조가 no-op였음(취약점 아님, 오진 규명).
- [x] 교정: 위조 방향을 "부풀리기"(`"1000000000000"`)로 — 실제 최저
      작업량인데 과장해 fork-choice에서 이기려는 시나리오. B가 replay에서
      재유도로 적발·거절. `assert_eq!(real difficultyWeight, "1")` 가드로
      전제 못 박음.
- [x] wire 소스 교정: `/block/latest` HTTP DTO는 wire-identical 아님(정상
      블록도 거절됨) → A의 `blocks.ndjson` 저장소 raw 라인에서 직접 읽음.
- [x] 대조군: 위조 안 한 쌍둥이를 별개 신선 노드에 같은 경로로 전송 →
      정상 ingest(height 1). "거절이 검증 때문이지 전송 오류 아님" 증명(별개
      노드인 이유: 쌍둥이가 같은 블록 `c` 공유 → 첫 노드는 이미-본으로 취급).
- [x] 공용 헬퍼 `announce_block_to`(Hello→BlockAnnounce→GetBlocks→Blocks)
      추가. 스펙의 신규 파일 대신 reject 헬퍼가 이미 사는
      `p2p_block_propagation.rs`에 형제 테스트 추가(DRY).
- [x] 로컬 게이트(test-only 티어): p2p 4/4 green + 새 테스트 3회 반복 안정 +
      fmt clean + clippy(`-p boole-node --tests -D warnings`) clean +
      `git diff --check` clean
- [x] 커밋(`767b3d8`) → PR #43 → CI green → rebase-merge(`5f45d73`) →
      remote 검증 → 착륙 기록 → 보고

## Review
착륙 완료 (2026-07-07). PR #43 rebase-merge, main = `5f45d73`. 커밋
`767b3d8`(rebase 후 `5f45d73`), NotoriAndo author. **N4 wave 종결.**

무엇을 했나 (쉬운 말): "이웃 노드가 보낸 위조/무효 블록은 거부된다"를 못 박는
회귀 테스트를 추가했다. 이 거부 동작 자체는 이미 지난 N3.3에서 만들어졌기에,
이번 일은 "나중에 실수로 위조 블록을 믿기 시작하지 못하게" 자물쇠를 거는
테스트다. 위조 블록은 자기가 실제보다 훨씬 많은 작업을 했다고 거짓말해서(작업량
가중치 부풀리기) 체인 경쟁에서 이기려는 시나리오인데, 받는 노드가 블록을 처음부터
다시 계산해 검증하면서 거짓을 잡아내 버린다. 위조 안 한 정상 블록은 같은 길로
보내면 잘 받아들여지는 것도 나란히 확인(대조군)해서, 거부가 "검증 때문"이지
"전송이 깨져서"가 아님을 증명했다.

개발 중 배운 것: 처음엔 가중치를 낮춰(1로) 위조하려 했는데, 이 테스트 시나리오는
채굴 난이도가 거의 최저라 정상 블록의 가중치가 원래부터 1이었다. 그래서 "1을
1로" 바꾼 셈이 되어 아무 변화가 없었고, 정상 블록이라 통과했다. 순간 취약점으로
오인했지만 파고들어 원인을 규명하고, 위조 방향을 "부풀리기"로 바로잡아 진짜
거부 경로를 검증했다. 또 하나: 블록을 HTTP `/block/latest`로 가져오면 실제
네트워크 전송 형식과 미묘하게 달라 정상 블록도 거부됐는데, A의 실제 저장 파일
(`blocks.ndjson`)에서 원본 바이트를 읽어 해결했다.

범위: 테스트 전용, production 코드 무변경. slashing/peer-ban은 비목표(E2).

검증:
- focused: `ingress_rejects_tampered_peer_block` — 위조 거절(head 무변경 +
  reject metric↑) + 정상 쌍둥이 ingest(대조군)
- 회귀: p2p_block_propagation 4/4 green, 새 테스트 3회 반복 안정
- 로컬 게이트: fmt clean + clippy clean + git diff --check clean
- CI: self-test pass 8m23s + supply-chain pass 3m5s (PR #43)
- working tree clean, origin/main == local HEAD == `5f45d73`

이번에도 push 전 fmt+clippy 로컬 게이트 선행 → CI 반송 0.

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

# 2026-07-08 — N4 reorg 트리거를 p2p 동기화 경로에 배선 (fork-choice end-to-end)

N4 wave 후속 slice. 텔레그램 "추천작업진행해" → 방향 검증 → "1번으로
진행해"(옵션 1: reorg 배선 + consensus 상태만 지금 정합, side-ledger 재빌드는
후속 slice로 이월) 승인. N4.2 fork-choice와 N4.3 reorg 원시연산
(`reorg_to_heavier_chain`)은 착륙했으나 라이브 경로에 한 번도 불려가지
않았다 — 더 무거운 **경쟁 체인**이 오면 조용히 버려졌다.

## 방향 검증 (완료)
- [x] `sync_with_peer`(p2p_ingress.rs)가 `ingest_announced_block`에만 의존 —
      이건 로컬 head를 딱 1블록 연장만 가능. head 아래에서 갈라지는 peer
      체인은 첫 블록 `prev_c`가 로컬 head와 달라 `Ignored`로 버려짐. fork-choice가
      라이브 경로에서 실행될 기회 자체가 없음을 확인.
- [x] `reorg_to_heavier_chain`은 착륙·테스트 완료(reorg_state_convergence)이나
      호출자 grep 결과 라이브 경로 0 — 미배선 확정.
- [x] 사용자에 옵션 제시 → 옵션 1(배선+consensus 정합 지금, side-ledger 이월) 수신.

## slice 구현
- [x] RED: `sync_reorgs_to_heavier_competing_chain` — B를 가벼운 1블록 fork
      `[X0]`로 pre-seed, peer A는 무거운 2블록 fork `[Y0,Y1]`(`Y0 != X0`) 보유.
      현재 코드에선 B가 reorg 못 해 20s 타임아웃(RED 확인).
- [x] GREEN(production 4곳):
      1) `local_node::ingest_candidate_chain` + `CandidateChainOutcome` 신설 —
         후보 체인을 `reorg_to_heavier_chain` 안에서 strict replay, fork-choice가
         엄격히 더 무거우면 채택(block store + reward ledger + in-memory
         chain/head/pool 창세부터 재유도), 위조·evidence-less는 `Rejected`.
      2) `sync_with_peer`의 `Ignored` arm → `reorg_from_peer`: peer 체인을
         창세부터 페이지네이션 GetBlocks로 전량 fetch 후 `ingest_candidate_chain`.
      3) 신규 metric `boole_p2p_sync_reorgs_applied_total` — fork-choice reorg
         (`sync_blocks_applied`는 0 유지)를 선형 fast-forward와 구분.
      4) RwLock 동일 스레드 write-write 교착 회피 — ingest 가드를 tight scope로
         drop 후 reorg 경로가 새 가드 재획득.
- [x] 테스트 race 교정: reorg가 B의 첫 sync pass에서 near-instant 발화 →
      transient height-1 단언이 sync loop와 경합. 해당 단언 제거,
      `sync_reorgs_applied==1` + `sync_blocks_applied==0` metric으로 "B가 [X0]에서
      출발해 reorg했음"을 엄밀 증명(empty-boot fast-forward면 reorgs=0/applied=2).
- [x] 로컬 게이트(node production 티어): p2p_initial_sync 3 + p2p_block_propagation
      4 + reorg_state_convergence 2 + boole-node lib 40 green
      (`--include-ignored --test-threads=1`) + fmt clean +
      clippy(`-p boole-node --all-targets -D warnings`) clean + `git diff --check` clean
- [x] 커밋(`c79e5bc`) → PR #45 → CI green → rebase-merge(`7bd27cc`) →
      remote 검증 → 착륙 기록 → 보고

## Review
착륙 완료 (2026-07-08). PR #45 rebase-merge, main = `7bd27cc`. 커밋
`c79e5bc`(rebase 후 `7bd27cc`), NotoriAndo author.

무엇을 했나 (쉬운 말): 이웃 노드가 "우리 것보다 더 무거운(=더 많은 일이 담긴)
경쟁 체인"을 들고 오면, 예전엔 그 체인의 첫 블록이 우리 머리에 안 이어진다는
이유로 그냥 무시하고 버렸다. 이제는 그런 경우 이웃의 체인을 창세(제일 처음
블록)부터 통째로 받아와, 처음부터 다시 계산·검증해서 정말로 더 무거우면 우리
노드가 그쪽으로 갈아탄다(reorg — 우리가 쥐고 있던 체인을 버리고 더 무거운
체인으로 재구성). 이걸로 fork-choice(어느 체인을 정답으로 삼을지 고르는 규칙)가
처음부터 끝까지 실제로 작동한다. 위조하거나 근거(evidence)가 빠진 경쟁 체인은
재검증에서 걸려 거부되고, 우리 체인은 그대로 유지된다.

새 계기판 눈금: `boole_p2p_sync_reorgs_applied_total`. 이걸로 "체인을 갈아탄
reorg"와 "그냥 뒤에 이어 붙인 fast-forward"를 구분한다(reorg면 이어붙이기
카운터는 0으로 남는다).

이월(옵션 1 결정 + 사후 정정 2026-07-08): reorg 원시연산이 소유한 consensus
상태(블록 저장소·보상 원장·메모리 체인/머리/풀)만 이 slice에서 재유도한다.
노드-로컬 bounty-event 원장과 N2.3 proof-dedup 미러는 이번엔 되감지 않는다.
**정정**: 여기서 이 둘이 "다음 부팅 때 블록 저장소로부터 다시 유도돼 self-heal
된다"고 적었으나 이는 부정확했다 — 둘 다 부팅 때 블록 저장소로부터 깨끗이
재유도되지 않는다. proof-dedup 미러의 `recover`는 제 파일(NDJSON)만 replay할 뿐
블록 저장소 재유도가 없어, 버려진 fork에서 크레딧된 proof가 미러에 남아 새 체인에서
다시 크레딧 가능한 재제출을 잘못 조기거절한다. bounty-event 부팅 heal은 on-disk
행을 기대 시퀀스의 PREFIX로 가정하는 suffix-append라, head 아래로 갈라지는 reorg가
그 가정을 깨뜨려 `--bounty-events` 노드는 `verify_ledger_matches_replay`에서 부팅
실패까지 날 수 있다. → proof-dedup 미러는 바로 아래 후속 slice에서 reorg 시점
곧바로 재빌드로 수리했고, bounty-event 원장·registry·side_pool은 여전히 별도 후속
slice로 이월(더 어려운 케이스: 라우트-구동 행은 블록에 없음).

개발 중 배운 것: reorg가 B의 첫 동기화 시도에서 거의 즉시(~0.3s) 발화해서,
"reorg 직전의 잠깐 상태(높이 1)"를 단언하려던 테스트가 동기화 루프와 경합해
깨졌다. 그 순간 단언을 지우고, 대신 "reorg 1회 + 이어붙이기 0회" 계기판 값으로
B가 가벼운 fork에서 출발해 갈아탔음을 흔들림 없이 증명하도록 바꿨다(빈 상태에서
출발했다면 이어붙이기 2회로 나올 것이라 구분됨).

범위: boole-node production. side-ledger 재빌드는 후속 slice로 이월. slashing/
peer-ban은 비목표(E2).

검증:
- focused: `sync_reorgs_to_heavier_competing_chain` — B가 A로 블록 단위 수렴,
  `sync_reorgs_applied==1` + `sync_blocks_applied==0`(reorg 증명)
- 회귀: p2p_initial_sync 3 + p2p_block_propagation 4 + reorg_state_convergence 2 +
  boole-node lib 40 green
- 로컬 게이트: fmt clean + clippy clean + git diff --check clean
- CI: self-test pass 8m11s + supply-chain pass 3m24s (PR #45)
- working tree clean, origin/main == local HEAD == `7bd27cc`

이번에도 push 전 fmt+clippy 로컬 게이트 선행 → CI 반송 0.

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

---

# 2026-07-08 — N4 후속: reorg 시 proof-dedup 미러 곧바로 재빌드 (노드, 옵션 1)

위 N4.3 reorg-sync 착륙에서 이월했던 "옵션 1"의 앞부분을 처리한다. 두 가지를
했다: (1) reorg가 새 체인을 채택할 때 N2.3 proof-dedup 미러를 그 자리에서 곧바로
새 체인 기준으로 재빌드, (2) 위 이월 노트의 부정확한 "self-heal on boot" 주장을
정정(위 문단 **정정** 참조). bounty-event 원장·registry·side_pool 재빌드는 더
어려운 별도 후속 slice로 이월(라우트-구동 행은 블록에 없음, suffix-heal PREFIX
가정이 reorg에서 깨짐).

## 방향 검증 (구현 전)
- ADR-0012 확인: proof-dedup 미러는 비권위(non-authoritative) admission 조기거절
  캐시일 뿐, "canon_hash당 크레딧 1회" 합의 규칙은 블록 replay가 독립적으로 강제.
  → 미러를 새 체인 기준으로 통째로 재작성하는 것은 합의 안전성에 무해(파일을
  지워도 조기거절 지연만 손해). 되감기 규모가 과하지 않음(작은 캐시 재작성).
- 정정 발견: `FileProofDedupLedger::recover`는 제 NDJSON 파일만 replay하고 블록
  저장소 재유도가 없어 reorg 후 self-heal 안 됨 → 이월 노트가 부정확했음을 확인,
  구현 전 사용자에게 정직 보고 후 옵션 1 축소 승인받음.

## slice 구현
- [x] RED: `rebuild_from_credits_replaces_stale_entries_atomically`(stale 시드 후
      새 체인 크레딧으로 재빌드 → stale 사라지고 새 것만, 파일도 원자적 교체),
      `rebuild_from_credits_with_no_credits_clears_the_mirror`(빈 입력→미러 비움),
      `reorg_rebuilds_proof_dedup_mirror_from_adopted_chain`(배선 free fn이 채택
      체인 evidence의 canon_hash를 모아 재빌드), `reorg_proof_dedup_rebuild_is_
      noop_without_configured_ledger`(원장 미설정→None 유지). 함수 부재로 컴파일
      실패(RED 확인).
- [x] GREEN(production 2곳):
      1) `FileProofDedupLedger::rebuild_from_credits(path, canon_hashes)` —
         canon_hash들을 첫-등장순 dedup해 NDJSON 라인으로 만들고
         `write_ndjson_lines_atomic`(temp+rename)로 파일을 원자적 교체, 새 in-메모리
         set 반환. append와 달리 truncate(중간 크래시 시 옛 파일/새 파일 중 하나,
         찢긴 splice 없음).
      2) `local_node::rebuild_proof_dedup_mirror_after_reorg(ledger_path, ledger,
         adopted)` 배선 free fn — 채택 체인의 `selected_share_evidence[].canon_hash`
         전량을 모아 (1)로 재빌드. `ingest_candidate_chain`의 `Reorged` arm에서 호출,
         실패 시 로그-후-계속(reorg는 이미 커밋됨, 미러는 지연 캐시).
- [x] doc 정정: `ingest_candidate_chain` doc-comment의 "both re-derived on boot
      (self-heal)" 문구를 정확히 교체(미러는 여기서 in-line 재빌드/부팅 self-heal
      아님; bounty-event는 이월이며 부팅 heal도 깨끗하지 않음).
- [x] 로컬 게이트(node production 티어): p2p_initial_sync 3 + p2p_block_propagation
      4 + reorg_state_convergence 2 + boole-node lib(신규 4 포함) green
      (`--include-ignored --test-threads=1`) + fmt clean +
      clippy(`-p boole-node --all-targets -D warnings`) clean + `git diff --check` clean

## Review
착륙 완료 (2026-07-08). PR #47 rebase-merge, main = `e74bc20`. 코어 커밋
`a0e1378`(rebase 후 `e74bc20`), NotoriAndo author.

무엇을 했나 (쉬운 말): 우리 노드가 더 무거운 경쟁 체인으로 갈아탈 때(reorg),
"이 증명은 이미 상 받았으니 또 안 줌"이라고 빠르게 걸러내는 작은 메모장(미러)이
있다. 예전엔 이 메모장을 갈아타기 후에도 그대로 뒀는데, 그러면 버려진 옛 체인에서
상 받았던 증명이 메모장에 남아, 새 체인에선 다시 상 받을 수 있는 재제출을 잘못
막아버린다. 이제는 갈아타는 그 순간 메모장을 새 체인 기준으로 통째로 새로 쓴다.
이 메모장은 "정답 장부"가 아니라 속도용 캐시라(진짜 규칙은 블록 재검증이 지킴),
통째로 새로 써도 안전하다. 그리고 예전 착륙 기록에 "이건 다음 부팅 때 저절로
고쳐진다"고 적었던 게 사실이 아니어서(메모장 복구는 제 파일만 다시 읽을 뿐 블록에서
새로 만들지 않음) 그 설명도 바로잡았다.

범위: boole-node production(비합의, 노드-로컬). bounty-event 원장·registry·
side_pool 재빌드는 후속 slice로 이월(더 어려운 케이스).

검증:
- focused: 신규 4 (rebuild_from_credits 2 + reorg 배선 2) green
- 회귀: p2p_initial_sync 3 + p2p_block_propagation 4 + reorg_state_convergence 2 +
  boole-node lib 44(신규 4 포함) green
- 로컬 게이트: fmt clean + clippy clean + git diff --check clean
- CI: self-test pass 8m04s + supply-chain pass 3m11s (PR #47)
- working tree clean, origin/main == local HEAD == `e74bc20`

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

---

# 2026-07-08 — N4 후속: reorg 시 bounty-event 원장·side_pool 곧바로 재빌드 (노드, 옵션 1 뒷부분)

위 proof-dedup 착륙에서 이월했던 "더 어려운 후속 slice"를 처리한다. reorg가 더
무거운 경쟁 체인을 채택할 때, 노드-로컬 bounty 상태 중 **블록-투영(block
projection)** 부분만 새 체인 기준으로 재유도한다.

## 방향 검증 (구현 전)
- 상태를 라우트-구동 vs 블록-투영으로 분류:
  - 원장의 `create`/`status_change`/`proof` 행 = 라우트-구동(블록에 없음, off-chain
    announce/status/proof 핸들러가 기록) → reorg 무관, 보존.
  - 원장의 `credit`/`share_promoted` 행 = 블록-구동 → `derive_bounty_events`로 채택
    체인에서 재유도.
  - `bounty_registry` = (정적 catalog + 라우트 행)의 순수 함수, 블록에서 파생 불가 →
    reorg-불변(재빌드 불필요).
  - `bounty_side_pool` = {수락 proof} − {블록에서 promote됨}; 차감집합만 블록-구동 →
    재유도 필요.
- 결론: "세 상태 전부 블록에서 재빌드"는 불가능(라우트 상태가 블록에 없음). 올바른
  설계는 "라우트 행 보존 + 블록 투영 재유도 + registry 그대로". 구현 전 이 통찰을
  사용자에게 보고 후 진행.

## slice 구현
- [x] RED: `rebuild_bounty_ledger_rows_keeps_route_rows_and_reprojects_block_rows`,
      `reorg_rebuilds_bounty_state_and_reopens_unpromoted_share`(옛 fork에서 promote
      됐던 proof가 새 체인에서 미promote면 side_pool에 pending으로 재등장),
      `reorg_bounty_rebuild_is_noop_without_configured_ledger`,
      `rewrite_atomic_replaces_file_and_round_trips`,
      `rewrite_atomic_rejects_invalid_event_and_writes_nothing`. 함수 부재로 컴파일
      실패(RED 확인).
- [x] GREEN(production 3곳):
      1) `runtime::derive_bounty_events` → `pub(crate)`로 승격(재빌드에서 재사용).
      2) `FileBountyEventLedger::rewrite_atomic(path, events)` — 각 이벤트 검증 후
         `write_ndjson_lines_atomic`(temp+rename)로 원장 전체 원자적 교체(append로는
         재작성 불가; 중간 크래시 시 옛 파일/새 파일 중 하나, 찢긴 splice 없음).
      3) `local_node::rebuild_bounty_ledger_rows`(순수: 라우트 행 원순서 보존 + 블록
         행 재유도) + `rebuild_bounty_state_after_reorg`(recover→재유도→rewrite→
         side_pool 초기화 후 `rebuild_bounty_side_pool`로 재빌드; registry 미변경).
         `ingest_candidate_chain`의 `Reorged` arm에서 proof-dedup 재빌드 뒤 호출,
         disjoint 필드 borrow, 실패 시 로그-후-계속(reorg는 이미 커밋됨).
- [x] doc 정정: `ingest_candidate_chain` doc-comment의 "bounty state NOT rewound —
      deferred" 문구를 "원장·side_pool은 여기서 in-line 재빌드, registry는 reorg-불변,
      원장 재작성이 부팅 heal의 PREFIX 가정도 유지"로 교체.
- [x] 로컬 게이트(node production 티어, 비합의): boole-node lib 신규 5 + rewrite 2 +
      reorg_state_convergence 2 + bounty_event_crash_heal 8 + bounty_event_ledger_
      recovery 2 + p2p_initial_sync 3 + p2p_block_propagation 4 green
      (`--include-ignored --test-threads=1`) + fmt clean +
      clippy(`-p boole-node --all-targets -D warnings`) clean + `git diff --check` clean

## Review
착륙 완료 (2026-07-08). PR #49 rebase-merge, main = `9c7d41d`, NotoriAndo author.

무엇을 했나 (쉬운 말): 우리 노드가 더 무거운 경쟁 체인으로 갈아탈 때(reorg), 현상금
(bounty) 관련 노드 기록 중 "블록에서 만들어진 부분"만 새 체인 기준으로 다시 만든다.
현상금 기록에는 두 종류가 섞여 있다. (1) 사람이 체인 밖에서 올린 것(현상금 공고,
상태 변경, 증명 제출) — 이건 블록과 무관하니 그대로 둔다. (2) 블록이 만들어질 때
찍힌 것(지급 크레딧, 이미 상 준 증명 표시) — 이건 갈아탄 새 체인 기준으로 새로 찍는다.
현상금 목록(registry)은 (1)만으로 정해지므로 갈아타도 안 바뀌어 손대지 않는다.
현상금 대기줄(side_pool)은 "수락된 증명 − 이미 상 준 증명"이라, 뺄셈 대상이 (2)라서
다시 계산한다. 결과적으로, 버려진 옛 체인에서 상 줬던 증명이 새 체인에선 상을 못
받게 됐다면 그 증명이 대기줄에 다시 나타난다. 원장을 새 체인 기준으로 통째로 다시
쓰기 때문에, 나중에 재부팅할 때 하던 "빠진 뒷부분만 채우는" 복구도 어긋나지 않는다.

범위: boole-node production(비합의, 노드-로컬). 현상금 투영 필드는 `block_hash`에
들어가지 않음.

검증:
- focused: 신규 5 (원장 재유도/재배선 3 + rewrite_atomic 2) green
- 회귀: bounty_event_crash_heal 8 + bounty_event_ledger_recovery 2 +
  p2p_initial_sync 3 + p2p_block_propagation 4 + reorg_state_convergence 2 green
- 로컬 게이트: fmt clean + clippy clean + git diff --check clean
- CI: self-test pass + supply-chain pass (PR #49)
- working tree clean, origin/main == local HEAD == `9c7d41d`

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

---

# SC.6 — family manifest registry determinism (2026-07-11 착수)

§SC(consensus safety closure) 첫 slice. GAP-03 결정성 절반: registry 순회가
HashMap 순서, store 로드가 read_dir 파일시스템 순서 + 중복 family_id
last-write-wins — 블록 생산(승격 walk)이 노드/재실행마다 달라질 수 있음.
ADR-0015 (c) family root 계산의 선결. **closed-local — public claim 아님.**

## Plan
- [x] RED(core): `crates/boole-core/tests/family_manifest_registry.rs` 신설 —
      `family_registry_iteration_is_deterministic_across_load_orders`
      (로드 순서 무관 + family_id 정렬 순회)
- [x] RED(node): `family_manifest_store.rs`의 last-write-wins 테스트를
      `manifest_store_rejects_duplicate_family_id`로 반전(typed error 단언)
- [x] RED 실패 확인
- [x] GREEN(core): `FamilyManifestRegistry` HashMap→BTreeMap
- [x] GREEN(node): 정렬 로드 + 중복 family_id typed hard error
      (`FamilyManifestStoreError`), skip-and-warn 정책은 유지
- [x] focused gate: `--test family_manifest_registry`(core) +
      `--test family_manifest_store`(node) + bounty_promotion 회귀
- [x] fmt + clippy + `git diff --check`
- [x] NotoriAndo author 커밋 → branch push → PR → CI green → merge → remote 검증
- [x] 텔레그램 최종 보고

## Review
착륙 완료 (2026-07-11). PR #56 rebase-merge, main = `30633b0`, NotoriAndo author.

무엇을 했나 (쉬운 말): family manifest(채굴 문제 유형 명세) 목록을 노드가 읽고
도는 순서를 어느 노드/어느 재시작에서든 똑같게 만들었다. 지금까지는 목록이
HashMap(순서 무작위 자료구조)과 파일시스템이 주는 순서에 의존해, 현상금 승격
walk(블록 생산 입력)가 노드마다 다를 수 있었다. 이제 (1) registry 순회는
family_id 알파벳 순으로 고정(BTreeMap), (2) 디렉토리 로드는 파일명 정렬 순서,
(3) 같은 family_id가 두 파일에 있으면 조용히 덮어쓰지 않고 typed error로 부팅
거부(ADR-0015 (c) family root 계산의 중복 정책과 동일). ADR-0015 (c) root 계산
(SC.2)의 선결이 닫힘.

검증:
- RED 직접 확인 2건: core는 로드 순서에 따라 순회가 실제로 달라짐(단언 실패),
  node는 typed error 부재로 컴파일 실패
- focused GREEN: family_manifest_registry 1/1 (core) +
  family_manifest_store 4/4 (node, 중복 거절 반전 포함)
- 회귀: bounty_promotion 15/15 + family_manifest_signature 15/15 +
  manifest_fixtures 1/1 (core), work_manifest_store 4/4 + bounty_route 4/4 (node)
- fmt clean + clippy(-D warnings) core/node clean + git diff --check clean
- CI: self-test pass + supply-chain pass (PR #56,
  run 29150092471) → auto-merge(rebase)
- working tree clean, origin/main == local HEAD == `30633b0`

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

추천 다음 작업: §SC 순서대로 리셋 창(SC.2+SC.3+SC.9) 착수 — SC.6이 선결이었고
이제 닫힘. SC.4/SC.5/SC.7/SC.8은 병렬 후보.

---

# §SC 리셋 창 W1 — 스키마 브레이크 1회 (2026-07-11 착수)

ADR-0015 (d)/(d-1) + ADR-0016 (e): 체인 데이터 형식을 깨는 변경 전부를 한 PR에.
이후 SC.2 잔여(root 강제·golden vector)/SC.3/SC.9/SC.1은 enforcement-only.
**closed-local — public claim 아님.**

## W1 구성 (전부 한 리셋)
- [x] preimage v3 (`b"block.v3"`): `promotedBountyShares`(+reward) 커밋, `promotedBountyCredits` 제거
- [x] `PromotedBountyShare.reward` (decimal string) 신설
- [x] `PersistedBlock.promoted_bounty_credits` 필드 제거 + validate_shape 이동
- [ ] `derive_bounty_settlement(committed_rows, registry, height)` 합의 공유 함수 —
      생산자/replay 동일 정책 (clamp = min(reward, budget_left), 구조 위반 typed reject,
      no_protocol_reward는 credit 행 없음)
- [x] replay가 선언 credit 가산 대신 위 함수로 재유도 (registry 파라미터 플럼빙)
- [x] evidence v2: `SelectedShareEvidence.signed_work` 슬롯 (권한 증거 자리 — 강제는 SC.1)
- [x] work.v2: `boole.signer.work.v2` — rewardRecipient가 서명 payload 안으로
      (CLI 생산 + node gate + audit lineage)
- [x] `FamilyManifest.resourceLimits.maxHeartbeats`/`maxRecDepth` 필수 양수 필드
- [x] `GenesisSpec.params.family_manifest_root: Option<String>` (dev/testnet 초기 None)
- [x] `CONSENSUS_RULE_VERSION` 2→3, preset `boole-testnet-1`→`boole-testnet-2`
- [x] (2026-07-11 감사 5 편입) multiplier 합의 홈 = rule v3 Tier-2 상수 — 아래 W1.a
- [x] (2026-07-11 감사 6 편입) proofHash 서버 유도 결박 — 아래 W1.b
- [x] fixture 재생성: block-hash v3 / replay v1·v2 / runtime-smoke 6종 / manifests v1
- [x] node runtime: `derive_bounty_events`·`derive_reward_event`가 유도 credit 사용
- [x] focused + consensus 게이트(runtime-smoke-all, proof-to-block-benchmark 직접 확인)
- [ ] NotoriAndo 커밋 → PR → CI green → merge → 보고

## W1.a — MinShareScoreMultiplier 합의 홈 (2026-07-11 마스터플랜 감사 이슈 5, 사용자 승인: Tier-2 상수)

ADR-0014는 Tier-3 node-local로 분류했으나 replay가 자기-선언 값의 산술 일관성만
검사 → 소스 미결박. rule v3가 W1에서 이미 브레이크 중이므로 지금 상수로 고정
(2차 브레이크 회피). 네트워크별 차등 근거 없음 → genesis param 대신 Tier-2.

- [x] RED `replay_rejects_block_authored_score_multiplier`
      (replay_fixtures.rs — 일관된 산술 + 비합의 multiplier → 거절, RED 실패 직접 확인)
- [x] GREEN: `rules::MIN_SHARE_SCORE_MULTIPLIER_NANOS = 1_000_000_000` 신설,
      `replay_evidence.rs`가 상수 일치 강제 (0-검사 대체) — replay_fixtures 15/15
- [x] ADR-0014 amendment (c-1): Tier-3 → Tier-2 이동 기록
- [x] focused: `cargo test -p boole-core --test replay_fixtures` — 15/15 GREEN

## W1.b — proofHash 서버 유도 결박 (2026-07-11 마스터플랜 감사 이슈 6 = SC.2 18번 흡수)

현재 node는 클라이언트 proofHash를 hex 형식만 검사 후 block.v3 preimage까지 전파.
정의 확정: `proof_hash := hex(SHA-256(canonicalize(envelope)))` — 서명 경로와 동일한
Boole canonical JSON (`canonical_payload_hash_hex`). miner의 원본 파일 바이트 해시는
JSON 재직렬화로 서버와 어긋날 수 있어 canonical JSON으로 통일. node/miner/CLI 동일 계산.
결박 지점은 intake(수령 시점) — replay 수준 재결박은 envelope가 블록에 없어 불가,
offline 재검증은 audit ledger + deep verify 표면(SC.10)이 담당함을 문서에 명시.

- [x] RED `bounty_rejects_claimed_proof_hash_not_matching_verified_bytes`
      (bounty_proof_route.rs — 형식 유효·내용 불일치 proofHash가 200 통과함을 RED로 직접 확인)
- [x] GREEN: node가 `canonical_payload_hash_hex(&envelope)` 재계산, 불일치 시
      `proof_hash_mismatch` 거절 (dedup peek 이전) — bounty_proof_route 19/19
- [x] miner `bounty_client`: envelope_bytes 해시 → canonical JSON 해시로 교체
      — bounty_client 7/7
- [x] CLI `bounty submit`: `--proof-hash` 생략 시 동일 계산으로 자동 산출,
      제공 시 로컬 검증(`proof_hash_mismatch` typed, wire 도달 전) — bounty_submit_cli 7/7
- [x] 기존 dummy proofHash 테스트 정리 (node 6파일/miner/cli)
- [x] focused: bounty_proof_route 19/19 + audit persists 1/1×2 + ledger recovery 2/2
      + verify-not-block-ready 1/1 + cross-network 5/5 + hard_guard 5/5
      + miner 7/7 + cli 7/7
- [x] SC.2/SC.7 문서에 W1 흡수 기록 + EXECUTION-ORDER 결정 로그
- [x] (부수) hard_guard S23 테스트를 W1 정산 규칙에 정렬 — 기존 W1 잔여 fallout:
      `no_protocol_reward` 가족은 credit 0이 새 규칙인데 테스트가 옛 기대(balance 100)
      → manifest를 `capped_bonus`로 파라미터화 (제 slice와 무관, 원인 코드 확인 완료)

비차단 후속 메모 (W1 범위 아님): `scripts/boole-model-benchmark.py`의 bounty
mode는 서명 없는 body를 직접 POST(현행 signed.v1 route와 이미 비호환)하고
proofHash를 attempt salt로 인위 유도(`derive_bounty_proof_hash`) — W1.b 결박
정의(canonical JSON 해시)와도 어긋남. 스텁 테스트만 있어 self-test에는 영향
없음. bounty mode를 다음에 실사용할 때 signed envelope + canonical 해시(고유성
필요 시 envelope 안에 attempt salt 필드)로 정렬 필요.

## Review (리셋 창 W1)
착륙 완료 (2026-07-12). PR #58 rebase-merge, main = `2f397a6`
(코드 `13103b8` + python 계약 동기화 `74a3569`), NotoriAndo author.

무엇을 했나 (쉬운 말): 테스트넷 전 마지막 "체인 데이터 형식 깨는 변경"을 전부 한
번에 실었다. 핵심은 현상금 정산의 진실 소스 교체 — 지금까지는 블록이 "나 현상금
얼마 받음"이라고 스스로 적으면 모든 노드가 그 금액을 그대로 믿었는데(감사가
확인한 치명 구멍 GAP-02), 이제 블록에는 정산의 입력(승격된 증명 + 공고된
현상금액)만 해시에 봉인해 싣고, 금액은 모든 노드가 같은 규칙(캡 한도로 자르기,
자격 없는 family 거절, 무보상 family는 0)으로 각자 계산한다. 만드는 쪽과 검증하는
쪽이 같은 함수 하나를 쓰므로 갈라질 수 없다. 함께 실은 것: 보상 주소가 서명
범위에 들어간 새 서명 형식(work.v2), 서명 증거를 블록에 실을 자리(evidence v2),
검증 예산 필드(manifest maxHeartbeats/maxRecDepth), genesis의 family root 자리,
규칙 버전 3, testnet-2 리네임, 점수 하한 배율의 합의 상수화(W1.a), 현상금
proofHash 서버 재유도(W1.b). 이후 §SC 잔여 slice는 형식을 다시 깨지 않고
enforcement만 얹는다.

검증:
- 전 워크스페이스 컴파일 + fmt + clippy(-D warnings) clean
- core 17 스위트 / node lib 49 + 테스트 바이너리 23종(p2p --include-ignored 포함)
  / cli 6종 / miner / p2p 전부 green — 테스트 파일 ~40개를 새 스키마에 동기화
- 골든 fixture 재생성: block-hash v3 + replay v1/v2 (regen 헬퍼를 --ignored
  테스트로 상설화 — 다음 리셋 때 명령 1번)
- consensus 티어 게이트 직접 확인: runtime-smoke-all 6/6 PASS +
  proof-to-block-benchmark 7/7·17블록·replay 실패 0·divergence 0
- CI: self-test pass(8m29s) + supply-chain pass(3m9s), 반송 1회(python 계약
  테스트 2건 — 로컬 게이트에서 생략했던 티어; lessons에 적재)
- working tree clean, origin/main == local HEAD == `2f397a6`

claim 경계: closed-local 검증 + CI only. public mining/유료 API/leaderboard
claim 아님.

추천 다음 작업: SC.3(복구가 커밋 근거에서 재유도) 또는 병렬 SC.4/SC.5 —
전부 enforcement-only라 리셋 없음.

---

---

# SC.2-f1 — proofHash를 verifier-effective artifact에 결박 (2026-07-12 착수, 운영자 채택 승인)

3차 검토 1 반영: W1.b의 envelopeHash 결속은 유지하되, dedup·registry·side pool·
블록 행·audit의 proof identity는 "verifier가 실제로 판정한 바이트"의 domain-separated
해시로 교체. 무시 필드(salt)·`:=` prefix 변경으로 같은 증명이 다수 지문을 갖는 구멍 마감.
스키마 무변경(값 유도 규칙만 변경), rule 범프 불요. spec = L1 master SC.2 착륙 노트 ②.

- [x] RED `bounty_dedups_on_verifier_effective_artifact` (route — salt만 다른 재제출 → duplicate;
      RED = trait 부재 컴파일 실패로 직접 확인)
- [x] RED `proof_hash_commits_verifier_effective_artifact` (lean unit — salt/prefix 불변 artifact)
- [x] GREEN: `BountyProofVerifier::effective_artifact` (기본 = canonical envelope,
      lean = 합성 모듈 바이트 — verify가 동일 메서드로 유도해 판정 바이트=지문 구조 일치)
      + `bounty_proof_hash_hex` (domain `boole.bounty.proof.v1\0`)
- [x] node: verifier lookup을 dedup 앞으로, artifact proofHash로 dedup/registry/side pool/
      audit 교체, audit 이벤트에 `envelopeHash` 동반 (envelopeHash wire 게이트는 불변)
- [x] 기존 proofHash 값 단언 테스트 갱신 (ledger recovery)
- [x] focused: bounty_proof_route 20/20 + lean unit 2/2 + ledger recovery 2/2
      + audit persists 1/1×2 + verify-not-block-ready 1/1 + hard_guard 5/5
- [ ] 게이트(production 티어): focused + runtime-smoke 확인 → NotoriAndo 커밋 → PR → CI → 머지

## SC.2-f1 확장 (2026-07-12 4차 검토, 커밋 전 반영)
- [x] HIGH: audit 이벤트에 `effectiveArtifact` 영속 + deep-verify가 그 바이트를 실행
      + `bounty_proof_hash_hex` 재계산 대조(runner 실행 전) — RED 3종
      (executes_same_artifact / rejects_tampered_proof_hash / legacy fallback)
- [x] MEDIUM: 응답에 `{proofHash, envelopeHash}` (정상+duplicate), miner Ok 확장,
      CLI/miner 문서에 "v1 wire proofHash = legacy envelope hash" 명문화
- [x] MEDIUM: trait `verify_artifact_with_evidence` — route가 해시한 artifact를
      verifier가 verbatim 실행 (기본 구현 위임, lean 오버라이드)
- [x] 문서 잔여 5건: N5.3 본문 선결 H.1~H.4 / H.5 대안 삭제 / H.11 boole-mcp /
      SC.10 gate 실체 파일 / SC.7 RED 2종(boot fail-fast·self-produce parity)
- [ ] focused GREEN 확인 → NotoriAndo 커밋 → PR → CI → 머지

## SC.2-f1 확장 2차 (2026-07-12 5차 검토, 커밋 전 반영)
- [x] HIGH downgrade 우회 마감: deep-verify legacy fallback 제거 — accepted lean 행에
      `effectiveArtifact` 부재 = divergence (RED
      `deep_verify_rejects_event_with_stripped_effective_artifact`; 리셋 창 직후라
      보존할 legacy 원장 없음 — 스키마 v2 대안 기각 기록)
- [x] 합성 원장 CLI 테스트 2건(state_verify_deep_lean_cli) artifact 계약으로 갱신
      (probe_effective_artifact 헬퍼 — live 경로와 동일 유도)
- [x] 5차 2·3번(trait 필수화, wire v2 개명 + miner 응답 필수 검증) = SC.2-f2 이월 등록
- [x] 5차 4번(SC.7 잔여·peer replay 이연) 기존 등록 확인 + claim 경계 재확인
- [ ] 최종 focused GREEN → NotoriAndo 커밋 → PR → CI → 머지

---

# SC.7 — share 문턱의 합의 결박 (2026-07-12 착수, 운영자 승인 "1번 진행해")

Critical 감사 1번(per-share 점수 검증 부재) + t_share 자기 선언 + 생산자/검증자
multiplier 단일 소스. 전부 enforcement-only(스키마 무변경).

- [x] RED `replay_rejects_selected_share_below_committed_min_score` — 실패 직접 확인
      (기준 미달 share 블록이 replay 통과함을 실증)
- [x] RED `replay_rejects_block_whose_t_share_diverges_from_genesis` — 실패 직접 확인
- [x] RED `same_block_hash_implies_same_share_threshold_verdict` — 실패 직접 확인
      (t_share가 preimage 밖 → 같은 해시·다른 문턱 변종 실존을 테스트가 고정)
- [x] GREEN A: replay가 모든 선택 share의 점수를 재계산해 committed floor 미달 시 거절
      (`share_score(재유도 hash) >= min`) + genesis `t_share` 값 동등 결박
      (retarget은 t_block만 조정 — 문서화) — replay 인접 12개 스위트 전부 GREEN
- [x] RED `producer_never_emits_non_consensus_multiplier` (config_fixtures) — 실패 확인
- [x] RED `named_network_boot_fails_fast_on_non_consensus_multiplier` (runtime_policy_boot)
      — 실패 확인
- [x] GREEN B: builder가 rule 상수 직접 커밋(`from_policy_with_t_block`) + **named
      network** boot가 비합의 calibration 거부 — config_fixtures 6/6,
      genesis_network_binding 4/4, runtime_policy_boot 9/9, builder 회귀 전부 GREEN
      · 결정 기록: 거부 지점을 `from_calibration_report`(1차 시도)에서 named-network
      preset 결박 분기(local_node.rs)로 이전 — admission 골든 fixture(multiplier 2,
      합법적 node-local 설정·ADR-0014 Tier-3)와의 충돌 발견이 계기. 이름 붙은 망만
      상수 강제, unnamed/fixture 런은 종전대로
- [ ] `self_produced_block_survives_strict_replay` — SC.5(boot/live parity)로 위임
      (genesis를 runtime에 배선하는 작업이 SC.5 본체와 중복; B1+B2가 구체 벡터 차단)
- [ ] 합의 티어 게이트: runtime-smoke-all + proof-to-block-benchmark 직접 확인
- [ ] NotoriAndo 커밋 → PR → CI → 머지 → 보고

---

# SC.5 — boot/live replay parity (2026-07-12 착수, 운영자 승인 "추천작업진행해")

GAP-08 Critical: 재부팅 시 자기 디스크 체인엔 관용 검사(legacy opt-in, zero 앵커,
k_max/seed 미강제), 네트워크 유입 체인엔 strict genesis 검사 — 같은 체인이 경로별로
다르게 판정. + 2차 검토 9(reorg 후보가 future-drift 가드 우회) + SC.7 위임분
(self-produce strict replay). spec = L1 master §SC.5.

## 작업 항목 (TDD 순서)
- [x] RED `boot_rejects_chain_rejected_by_live_ingest` — 4종 corpus(evidence-less /
      k_max 초과 / 빈 seedHex(seed 필수 시) / 이질 앵커)를 boot·ingest 양쪽에 급식,
      판정 동일 단언 (runtime_policy_boot 또는 신설 binary)
- [x] GREEN: `boot_from_store*`가 `RuntimeConfig::genesis_spec()` 기반 strict replay로
      전환, "no p2p ingest path yet" stale 주석 정정
- [x] legacy 진입점(`LegacyEvidenceOptIn`)을 named network에서 구조적 접근 불가로
- [x] RED `reorg_rejects_candidate_suffix_beyond_future_drift` — ingest는 거부하는
      미래 ts suffix를 reorg가 채택하면 Fail (`check_block_ts_future_drift` 호출처가
      local_node.rs:4426/:4722 2곳뿐 — sync_with_peer→reorg 경로 우회 확인됨)
- [x] GREEN: reorg 후보 suffix의 near-tip 높이에 future-drift 가드 적용
- [x] RED `self_produced_block_survives_strict_replay` (SC.7 위임) — commit 전
      cache+block strict replay; genesis를 runtime에 배선(boot 전환과 같은 표면)
- [x] CLI `state verify` genesis-aware 전환 — `--network` opt-in strict replay (preset→genesis 명시 매핑, 조용한 폴백 금지; mainnet은 typed 오류)
- [x] store fixture 중 legacy 관용 의존분 전수 확인 — fallout 2건 수리: account_balance_route(legacy v1 시딩→라이브 커밋 전환; v2 golden은 t_block==t_share라 유효 genesis 불가 확인), reorg_state_convergence(미래 고정 ts→현재 기준 재정렬) (W1 리셋으로 대부분 재생성 —
      잔존분만)
- [ ] focused: runtime_policy_boot + replay_fixtures + reorg_state_convergence +
      p2p_block_propagation(--include-ignored)
- [ ] 합의 티어 게이트: runtime-smoke-all + proof-to-block-benchmark 직접 확인
- [ ] NotoriAndo 커밋 → 브랜치 push → PR → CI green → auto-merge → remote 검증 → 보고

주의: closed-local + CI only, public claim 아님. 진행 보고는 텔레그램(chat_id
1311067056)으로.

---

# SC.9 — 결정적 verifier budget + checker pin 반전 (2026-07-13 착수, 운영자 승인 "다음 작업 진행해")

판정 = (증명 바이트, pinned checker, 커밋 budget)의 순수 함수. 벽시계/rlimit은
containment 전용으로 강등. spec = L1 master §SC.9. manifest 스키마분
(maxHeartbeats/maxRecDepth)은 W1에서 선착륙 — 잔여는 enforcement-only.

## 하위 단계
- [x] SC.9a — runner budget 배선 + 3상태 verdict + 소스 재정의 방어 2선
  - [x] RED `verdict_is_budget_exceeded_not_timeout_when_steps_run_out`
        (`budget_verdict.rs` — 컴파일 RED: LeanVerdict/budget 필드 부재 실증)
  - [x] RED `containment_kill_is_retryable_unavailable_and_does_not_advance_head_or_checkpoint`
        — 3분할: 러너 분류(`wall_clock_containment_kill_...`, budget_verdict.rs) +
        매핑 단위(`containment_kill_maps_to_retryable_error_...`,
        lean_bounty_verifier.rs) + 라우트/원장/head 불변
        (`bounty_containment_availability.rs`, 스펙 원명 유지)
  - [x] RED `proof_cannot_override_committed_max_heartbeats` /
        `..._max_rec_depth` / `unlock_limits_is_forbidden` — 행동 RED 실증
        (`budget_override_boundary.rs`): 현재 코드가 override 소스를
        **accepted=true로 통과**시키는 것 + audit pass 무방비 확인 후 GREEN.
        layer 2 독립 테스트 `audit_pass_rejects_budget_override_independently_of_intake` 동봉
  - [x] GREEN: `LeanRunnerConfig.max_heartbeats/max_rec_depth` →
        `boole_check <proof> <hb> <rd>` → 내부 `lean -DmaxHeartbeats/-DmaxRecDepth`
        + Audit.lean 동일 budget 옵션 elaboration; `LeanVerdict` 3상태
        (timeout/신호사 = retryable_unavailable, heartbeats/recDepth 소진 =
        budget_exceeded 결정적 거절); FORBIDDEN_TOKENS에 maxHeartbeats/maxRecDepth
        (1선) + Audit.lean 원문 스캔 `BOOLE_BUDGET_OVERRIDE`(2선);
        rules.rs `BASE_LANE_MAX_HEARTBEATS=400_000`/`BASE_LANE_MAX_REC_DEPTH=512`
        + miner 배선 + 기본값 동기화 테스트; bounty verifier는
        retryable을 Err로 매핑(502, 원장 무기록); `maxSteps` 메타데이터 퇴역
        (bounty registry allowed-keys 제거 + fixture 재생성);
        checker_artifact_hash 재고정 `1dd3055a…42be1` (README + verify 스크립트 green)
- [x] SC.9b — checker pin 반전 + 부팅 toolchain identity 강제
  - [x] RED `named_network_boot_refuses_on_checker_artifact_hash_mismatch`
        (+ 대조군: 무변조 사본은 checker 게이트 통과 후 genesis 게이트에서 거절 —
        거부 원인의 특정성 증명)
  - [x] RED `named_network_boot_rejects_wrong_lean_version_or_githash` / `..._lake_version`
  - [x] RED `preset_pin_matches_released_checker_toolchain_manifest`
        (pin None + manifest 부재로 RED 실증)
  - [x] RED `effective_toolchain_evidence_matches_checker_process` — **실기계 실증**:
        개발 머신 ambient lean 4.32.0 vs checker 유효 lean 4.29.1 불일치로
        evidence가 잘못된 toolchain을 기록하던 TOCTOU 갭 재현 후 수리
  - [x] GREEN: testnet-2 preset `checker_artifact_hash = Some(1dd3055a…42be1)` +
        `lean/checker/RELEASE-MANIFEST.json`(boole.checker.release.v1) +
        `SHA256SUMS`(scripts/make-checker-release-sums.sh) + named boot 3중 대조
        (소스 해시 / manifest↔pin / 실행 lean version·githash·lake version) typed 거부
        (`checker_pin.rs`, genesis 게이트보다 먼저) + evidence를
        `lake env lean` 유효 identity로 전환(프로세스 캐시). 검증: 신규 4/4 +
        기존 genesis_network_binding/genesis_commitment 무회귀
- [x] SC.9c — cross-platform verdict corpus CI (branch protection 반영만 잔여)
  - [x] RED `verdict_corpus_is_identical_across_platforms_and_profiles`
        (golden 부재 RED → BOOLE_REGEN_VERDICT_CORPUS=1 재생성 → verify green;
        corpus 7케이스: accept/false/heartbeats 소진/recDepth 소진/override 2종/sorry —
        wall-clock containment는 기계 의존이라 corpus에서 의도적 제외)
  - [x] GREEN: `fixtures/verdict-corpus/golden.json` + self-test.sh
        `verdict-corpus` 스테이지 + `.github/workflows/verdict-corpus.yml`
        4 job(Linux/macOS × debug/release, fail-fast 없음, path-filter 없음) +
        `if: always()` aggregate `verdict-corpus` + python 계약 6종(18/18 green)
  - [ ] branch protection에 `verdict-corpus` required 추가 — PR에서 status 확인 후
        gh api 적용(머지 전), EXECUTION-ORDER 기록
- [ ] 게이트(consensus 티어): lean-runner focused + manifest/binding focused +
      runtime-smoke-all·proof-to-block 직접 확인 + **L8 규칙: CI 동일 clippy
      (-D warnings, CI feature) + boole-node/cli 전체 --no-fail-fast**
- [ ] NotoriAndo 커밋 → PR → CI green → 머지 → 착륙 기록 → 텔레그램 보고

주의: TB.1/N0-pre.1 blacklist/allowlist 로직 무접촉 확인. closed-local + CI only.

---

# 게이트 단축 로드맵 (2026-07-14 운영자 승인 "추천조합 진행해")

- [x] ① L8 규칙 2 개정 — 인접 crate 전체 테스트를 CI로 이관, 로컬은
      focused+fmt+clippy 2종+smoke (lessons.md L8/L9 기록)
- [ ] ② cargo-nextest 도입 슬라이스 (SC.9 착륙 후) — 테스트 프로세스 격리로
      병렬화; P0.3 결정성 계약(test_self_test_contract.py) 개정 동반 필요
- [ ] ③ SC.4/SC.8 worktree 멀티에이전트 병렬 착수 (SC.9 착륙 후)

주의: "full green 없이 main 금지" 불변량 무변경 — CI required checks가 강제.

---

# SC.10-ii-b 착륙 기록 (peer-block ingest Lean 재검증)

의도: 구조 replay는 블록의 모양·선택·seed↔chain 결속만 증명하고, 각 share의
`proofPackage`가 Lean-유효 증명의 canon인지는 증명하지 않는다. checker-pinned
named network에서 피어 블록을 채택하기 전에, base-lane 증거에 대해 committed
budget으로 pinned checker를 재실행한다(ADR-0016 (c)). closed-local/무-checker
노드는 스킵(helper `None`)해 pre-SC.10 동작 유지. bounty lane은 재검증 안 함
(ADR-0016 (d)).

- [x] RED/GREEN `reverify_block_selected_shares` 블록 단위 fold — 3-state:
      전부 accept/not-lean-bound/skip ⇒ Verified; deterministic 실패(source
      re-derive / canon mismatch / Lean DeterministicReject) ⇒ 합의 거부;
      availability 실패(Lean RetryableUnavailable) ⇒ Deferred(거부도 fail-open
      accept도 아님, ADR-0016 (a-3)). deterministic이 retryable을 이긴다.
- [x] ingest 배선(`ingest_announced_block`): replay 통과 후 채택 전 재검증 게이트
      → Rejected / Deferred(신규 IngressBlockOutcome variant) / continue.
      announce·sync 두 consumer가 Deferred 처리(메트릭 bump, sync는 peer-fail
      없이 hold). `boole_p2p_ingress_blocks_deferred_total` 메트릭 추가.
- [x] 단일 verifier 신원: ingest는 CLI audit과 동일 profile `v1-lenbound`,
      동일 committed budget(BASE_LANE_MAX_HEARTBEATS/REC_DEPTH), 동일 pin
      (`network_genesis_preset(...).checker_artifact_hash`) — ADR-0016 (c-2).
- 검증: focused block_evidence_verifier 4/4 + ingest_block_reverify 3/3,
      fmt/clippy 2종 0경고, runtime-smoke-all·proof-to-block-benchmark green
      (invalidAccepted 0 / chainDivergence 0). closed-local 검증만, public 아님.
- 후속(ii-c): 동일 verifier 신원을 reorg 경로에 배선. (ii-d): admission 수렴 +
      self-produce parity + resource-limit 공유.

---

# SC.10-ii-c 계획 (peer-competing-chain reorg Lean 재검증)

의도: ii-b가 head를 1칸 늘리는 단일 블록 ingest에 Lean 재검증 게이트를 배선했다.
reorg 경로(피어의 FULL 경쟁 체인을 fork-choice로 채택)는 아직 구조 replay만 하고
Lean 재검증을 안 한다. ADR-0016 (c)는 admission/ingest/reorg 3곳이 같은 verifier
entry로 수렴할 것을 요구한다 — ii-c는 reorg를 배선한다. candidate 체인의 각 블록
base-lane 증거를 채택 전에 committed budget으로 pinned checker에 재실행. closed-local
/무-checker 노드는 스킵(pre-SC.10 동작 유지). bounty lane 재검증 안 함(ADR-0016 (d)).

- [ ] RED/GREEN `reverify_candidate_chain_selected_shares` 체인 단위 fold —
      ii-b 블록 fold를 candidate 각 블록에 적용, 같은 precedence:
      deterministic reject 어디서든 즉시 승리(체인이 절대 유효할 수 없음) ⇒
      합의 거부; 아니면 첫 availability 실패가 체인 전체를 defer; 아니면 Verified.
      detail에 `block[idx]` prefix. BlockReverifyOutcome 재사용(신규 타입 없음).
- [ ] reorg 배선(`ingest_candidate_chain`): candidate 파싱 후 `reorg_to_heavier_chain`
      호출 전 재검증 게이트 → DeterministicReject⇒Rejected / RetryableUnavailable⇒
      Deferred(신규 CandidateChainOutcome variant) / Verified·None⇒proceed.
- [ ] `reorg_from_peer` consumer가 Deferred 처리: `sync_reorgs_deferred` 메트릭
      bump, peer-fail 없이 hold(다음 poll 재시도). `boole_p2p_sync_reorgs_deferred_total`
      메트릭 추가.
- [ ] 단일 verifier 신원: ii-b와 동일 profile `v1-lenbound`/budget/pin —
      ADR-0016 (c-2). full-candidate 재검증(genesis부터)은 최소 슬라이스; 이미
      검증된 prefix 스킵 최적화는 SC.10-iii(verified-prefix checkpoint)로 이관.
- 게이트: focused reorg_chain_reverify + ingest_block_reverify + block_evidence_verifier,
      fmt/clippy 2종 0경고, runtime-smoke-all·proof-to-block-benchmark green
      (consensus tier). closed-local 검증만, public claim 아님.

---

# SC.10-iv-c 계획 (3-노드 Lean-invalid 주입 스모크 — SC.10 필수 gate)

텔레그램 승인: msg 2435 "1번 진행해" (2026-07-16), 재개 지시 msg 2437. spec =
L1 master §SC.10 커밋/Full gate — "3노드 수렴 smoke에 Lean-invalid 주입 케이스
추가 — 필수 gate: 주입 블록이 어느 노드에도 채택되지 않음을 smoke가 직접
단언해야 SC.10 완료" (2026-07-12 3차 검토 6으로 권장→필수 승격).
closed-local 검증 + CI only — public mining/유료 API claim 아님.

의도: iv-b가 "정직한 증명이 실 Lean으로 검증돼 통과한다"(양성)를 필수 레인에
넣었다. iv-c는 음성 대조 — 구조적으로는 유효하지만(canon hash·seed 결박·score
전부 통과) Lean-증명이 아닌 share/블록을 checker-pinned 3-노드 망에 주입해
admission(HTTP)·gossip ingest(p2p) 관문이 전부 거부하고 어느 노드도 채택하지
않음을 self-test 필수 스테이지로 못박는다. reorg 관문은 Rust focused 테스트
(ii-c)가 이미 고정 — 스모크는 admission+ingest 라이브 경로 담당.

- [x] 탐색: p2p wire 코덱 / ii-b Lean-invalid 블록 구성법 / 거절 카운터
      메트릭 이름 / 주입 도구 유무 (Explore 에이전트). 핵심 발견: HTTP `/submit`은
      admission Lean 게이트를 우회하므로 주입은 반드시 **peer 노드**(checker-off
      생산자)가 만들어 gossip으로 넣어야 하고, bash용 p2p 클라이언트는 없다.
- [x] RED 1 (계약): test_self_test_contract.py에 신규 스테이지
      `testnet2-lean-invalid-injection` + 스크립트 존재/마커 + 집계 gate 필드
      계약 테스트 2건 추가 → 스크립트/배선 부재로 실패 확인(FF)
- [x] RED 2 (fixture 계약): 신규 생성기 테스트 `testnet2_lean_invalid_fixture.rs`
      — Lean-invalid fixture가 합의 소스에서 재유도 일치(honest canon byte-50
      flip) + "admissible yet canon-mismatched" 속성 단언 → fixture 부재로 실패,
      재생성 본문 출력 (iv-b golden 관행)
- [x] GREEN: fixture 2개(`testnet2-lean-invalid.v1.json` +
      `testnet2-pinned-highrate.v1.json`) 커밋 +
      `scripts/testnet2-lean-invalid-injection-smoke.sh` 신규(F=checker-off
      생산자 + H1·H2=checker-pinned, full mesh → ① 주입: F가 위조 블록
      self-produce·gossip → H1·H2 ingest 재검증 거부(채택 0 + 거부 카운터 관측)
      ② 대조군: 정직 share→H1 self-produce → H1·H2 수렴 height 1) + self-test
      스테이지 + 집계 gate 조건(invalidBlockAdoptedBy==0 ∧
      invalidBlockRejectedByIngest ∧ honestConvergedHeight==1 강제)
- [x] 스모크 로컬 green: {invalidBlockAdoptedBy:0, invalidBlockRejectedByIngest:
      true, honestConvergedHeight:1, convergedHead != invalidHead}. 생성기 2/2 +
      계약 16/16. 개발 중 rate-limit 2건(IpQuota loopback 공유 → highrate
      시나리오, PkQuota 같은 nonce 티켓 충돌 → 주입 fixture 별도 nonce) 규명·수리
      (lessons 2026-07-16 기록). 집계 python은 mock으로 pass/fail 양방향 확인.
- [ ] 게이트: fmt --check PASS + clippy 2종 (진행 중) + git diff --check PASS +
      pycache 없음 (production src 무변경 = test/scripts/fixtures 티어;
      runtime-smoke/proof-to-block는 CI self-test가 강제)
- [ ] NotoriAndo 커밋 → branch push → PR → CI green → rebase 머지 → remote
      검증 → L1 master §SC.10 기록 갱신 → 텔레그램 보고

설계 노트: 주입 위치가 spec 문면은 p2p-local-convergence-smoke.sh(N3.5,
checker-off·비명명 망)이나, Lean 재검증은 checker-pinned 명명 망에서만
활성이라 N3.5 스모크에 얹으면 그 게이트가 Lean 툴체인에 결합되고 비용이
3배가 된다 — iv-b가 만든 pinned 부팅 기반 위의 독립 3-노드 스모크로 착륙
(spec 의도 "3노드 수렴 smoke + 주입 단언"은 신규 스모크가 그대로 충족,
편차는 착륙 기록에 명시).

---

# SC.10-iii 계획 (verified-prefix checkpoint — SC.10 마지막 조각)

텔레그램 승인: msg 2442 "추천 작업 시작해" (2026-07-16). spec = L1 master
§SC.10 + ADR-0016 (c)/(c-1). Bitcoin assumevalid 모양의 node-local 비합의
성능 상태 — "내가 Lean으로 검증 끝낸 높이"를 기록해 재-부트스트랩 시 그 prefix의
Lean 재검증을 스킵. closed-local + CI only.

방향 검증 (Explore 2회 + ADR 정독):
- 부팅 replay는 구조 검증만(canon 재유도), Lean 미실행. Lean 재실행은
  ingest/sync/reorg/gossip-admission 경로에만(ii-b/c/d). 따라서 checkpoint의
  실효 이득 = 초기 sync 재-pull 시 이미 검증한 prefix의 Lean 스킵(assumevalid).
- 결박 identity(ADR c-1): genesis_spec_hash(체인·체커핀·family root·rule ver
  전이 결박) + checkpoint 높이의 block hash(prefix) + checker_artifact_hash
  (명시 방어) + base-lane budget(max_heartbeats/max_rec_depth).
- 원자 파일: durability::write_ndjson_lines_atomic(temp→fsync→rename→dir-fsync)
  재사용. 손상/torn 파일 = 부재로 읽음(안전, genesis 재검증).

## 서브슬라이스
- [x] iii-a store primitive (**PR #75, CI 대기** — 커밋 `1fdd942`):
      VerifiedPrefixCheckpoint 레코드 + read/write(원자) + identity_matches.
      런타임 미배선(소비자 iii-b/c/d) — 기존 경로 무변경, 회귀면 0.
      RED(unimplemented) → GREEN 6/6(round-trip / missing·corrupt→absent /
      atomic replace / identity accept·reject-any-field). pub API 노출로
      dead_code 회피. clippy는 로컬 syspolicyd 스톨로 CI 강제.
- [ ] iii-b advance: ingest_announced_block에서 Lean 재검증 Some(Verified) +
      durable write 성공 후 checkpoint 전진(ADR c-1 순서). Deferred/Rejected는
      조기반환이라 head·checkpoint 불변(retryable_unavailable_does_not_move_
      head_or_checkpoint). reorg 경로도. checkpoint 경로 = block_path 형제.
      /status에 verified 높이 노출.
- [ ] iii-c boot: checkpoint 로드 + identity·on-disk block hash 검증, mismatch
      → discard(typed log) → genesis 재검증. sync 재-pull에서 checkpoint 높이
      이하 Lean 스킵. restart_skips_reverification_below_checkpoint /
      checkpoint_discarded_on_genesis_or_toolchain_or_budget_mismatch /
      named_network_boot_refuses_without_checker_toolchain.
- [ ] iii-d reorg/rollback 안전: reorg_below_checkpoint_invalidates_checkpoint
      (검증된 common ancestor까지 rewind or 폐기) / block_store_rollback_cannot_
      reuse_future_checkpoint. + 재-sync-skip smoke(선택). SC.10 wave 종결.

주의: iii-b/c/d는 consensus-adjacent(ingest/reorg/boot) — checker-pinned 테스트
하네스 필요. 슬라이스별 TDD → focused → CI. public claim 아님.

---

# SC.10-iii-c-2 착륙 기록 (assumevalid 재검증 스킵)

재부팅으로 로컬 exec 회복 후 로컬 스모크 검증하며 진행. `checkpoint_skip_decision`
순수 함수(스킵/정상/발산) + ingest 배선(boot-load된 checkpoint 소비, 스킵 시
메트릭 bump·전진 안 함, 발산 시 폐기). 재-부트스트랩 스모크(저장소 삭제·checkpoint
유지→재sync에서 Lean 스킵 0→1, 동일 head 재수렴) 로컬 PASS. 순수 함수 단위
테스트 6종은 CI cargo-test(로컬 test-binary exec는 여전히 syspolicyd 스톨,
스모크=prod 바이너리는 정상). self-test 집계가 skip0→재sync스킵→동일head 강제.
잔여: iii-d(reorg/rollback checkpoint 안전) → SC.10 wave 종결.

---

# SC.10-iii-d 착륙 기록 (reorg/rollback checkpoint 안전) — SC.10 wave 종결

`checkpoint_survives_reorg` 순수 함수(정확 일치만 생존, 짧은 chain/불일치=무효) +
reorg 경로(ingest_candidate_chain) 배선: reorg가 checkpoint 아래에서 갈라지면
(채택된 chain의 그 높이 블록 해시 불일치 or chain이 더 짧음) checkpoint 무효화
(in-memory None + 파일 삭제). 롤백 안전 스모크: 저장소 비우고 checkpoint의
block_hash를 틀린 값으로 변조→재sync에서 그 checkpoint는 재사용되지 않고(스킵
카운터 0 유지) 재검증 후 실제 head로 수렴. 로컬 PASS. self-test 집계가
divergentCheckpointNotReused∧skip0∧convergedToRealHead 강제.

**SC.10 wave 종결**: ii(a~d) + iii(a/b/c-1/c-2/d) + iv(0/a/b/c) 전부 착륙.
verified-prefix checkpoint 기록·전진·부팅검증·assumevalid 스킵·reorg/rollback
안전까지 완결. closed-local + CI only.

---

# SC.1 — proposer/share reward ownership binding (GAP-05, ADR-0015 (b)/(b-1))

plan: /Users/seoyong/.claude/plans/cozy-wiggling-lobster.md (2026-07-18 승인).
스키마 분은 리셋 창(PR #58) 착륙 완료 — 잔여는 전부 enforcement-only(rule v3 유지).
방향 검증: 결정로그 4개 주장 전부 main 990a1fe 코드와 일치 확인.

- [x] SC.1-a replay 검증기 (verify-when-present + proposer==winner) —
      ✅ 착륙 2026-07-18, PR #82 cb6e92c, CI green(self-test·supply-chain·verdict-corpus)
      - [x] RED 4종 신설 (reward_authorization_replay.rs): identity chain /
            share reward 미승인 / proposer reward 미승인 / proposer!=winner —
            전부 기대대로 실패 확인 (replay가 무권한 라우팅 수용)
      - [x] share_authorization.rs 신설 (봉투-내재 공유 검증기 +
            SIGNER_WORK_V2_SCHEMA 상수) — audit와 공유, node/ingress가 SC.1-b에서 재사용
      - [x] replay_evidence.rs 배선 (evidence 존재 시 invariant 강제 +
            verify_canonical_selection에 proposer==winner + proposer reward 승인)
      - [x] evidence_backed_block proposer=winner 교정 + v2.json 재생성
            (proposerPk→1111, credits 병합 {1111:2}, regen 헬퍼가 credits/balances 재유도)
      - [x] focused 게이트 GREEN (영향권 테스트 바이너리 18종 — 전체 crate 실행은
            syspolicyd 스톨로 중단, lessons.md 기록. full은 CI가 강제)
      - [x] fmt + clippy 2종 (CI-동일, -D warnings) 통과
      - [x] 커밋 d526573 → PR #82 → CI green → squash merge cb6e92c → remote 검증
- [x] SC.1-b 봉투 보존 배선 — ✅ 착륙 2026-07-19, PR #83 squash ecf4263, CI green
      - [x] RED 4종: 세션 2종(work_pk_mismatch 403 / evidence signedWork 운반)은 로컬 확인,
            p2p 2종(roundtrip 보존 / 위조 봉투 ingress 거절)은 **CI 스크래치 브랜치로 RED 실증**
            (sc1b-red-check run 29646264167 — admitted=1/rejected=0, evidence signedWork Null.
            로컬 test-binary exec가 syspolicyd 스톨로 불능이라 SC.10-iii-c-2 선례대로 CI 이관.
            ※ 로컬 exec는 재부팅으로 회복된 전례 있음)
      - [x] 구현: 게이트 봉투 보존(CheckedSubmitSession.signed_work) + body.pk==submittedBy
            typed 거절(work_pk_mismatch) + 서명 body 정규형 강제 + CandidateShare 슬롯 +
            runtime evidence 채움 + egress signedWork 동봉 + ingress 봉투-내재 검증(공유 헬퍼)
      - [x] typecheck + fmt + clippy 2종 clean (테스트 GREEN은 CI가 실증)
      - [x] 커밋 cea7011 → PR #83 → CI green → squash merge ecf4263 → 브랜치/스크래치 정리
      - [x] CI 반송 2건 처리: ① 신규 테스트 max_requests 과다(3>2연결)로 self-test hang
            (lessons.md 재발 기록) ② p2p roundtrip을 wire-중계(MITM) 구조로 재설계 —
            즉시-블록 시나리오에서 pool 스냅샷이 블록 채택과 경합(구조적 플레이크).
            같은 경합이 잠재해 있던 기존 base 테스트도 단조 admitted 카운터로 견고화
            (pre-N3.3의 height==0 scope pin 제거 — PR 본문 명시)
- [ ] SC.1-c testnet2 fixture/스모크 세션 이행 (강제 없음, 전면 재생성)
- [ ] SC.1-d named 강제 반전 (testnet-2 전용 술어, replay에 network_id 스레딩)

---

# §ZK — base family 교체 플랜 채택 기록 (2026-07-19, 운영자 지시 "1번으로 진행")

docs-only 기록 — 플랜 본문은 local-docs (gitignored), 이 엔트리는 결정의 repo 흔적.

- [x] 운영자 지시 (2026-07-18 텔레그램): 공식 base family를 v1-lenbound에서
      hash-generated ZK circuit verification family로 완전 교체 (재포장 금지,
      정상 작동 후 운영 경로 삭제). 채굴자 실제 제출 답의 블록 결박 필수
      (정답 템플릿 재유도 금지), 전 노드 독립 재검증, Base/Bounty lane 분리,
      외부 저장소 불가져옴, D2 재정의 + P0 승격.
- [x] 방향 평가 3건 승인·플랜 편입: R1 지름길 함정(생성기 전지식 공격자의
      솔버 붕괴 위험) → 오프체인 스파이크(ZK.0)를 ADR 선결 go/no-go로 신설 /
      R2 정직 라벨(base lane 가치 = 캘리브레이션+corpus+liveness, E층 탈출
      클레임 금지) / R3 답 결박 = canon f(seed)→f(seed,witness) 합의 계약
      변경으로 4경로(admission/ingest/reorg/replay) 전부 영향권.
- [x] 플랜 작성 (기존 master plan과 동일 9-필드 해상도): L1 master §ZK 신설
      (ZK.0 스파이크 → ADR-0017 → ZK.1~ZK.2 순수 추가 → ZK.3 리셋 창
      (helper 재핀+witnessHex+rule v4+boole-testnet-3) → ZK.4~ZK.7 →
      ZK.8 기본 전환 → ZK.9 lenbound 삭제) + §2 invariant 2 개정(답 결박)
      + base family 북극성 배너 + 추천 실행 순서/한 줄 요약 갱신 +
      EXECUTION-ORDER 본선 [12]/[D2] 재정의 + 결정 로그 + thesis §12 개정.
- [x] 순서 binding: §SC 잔여(SC.1-c/d·SC.2 잔여·SC.3)는 그대로 진행,
      ZK.0 스파이크는 즉시 병렬 가능, ZK.3 창은 §SC 잔여 착륙 후.
      지시문 편차 1건 명시: SC.2 root 강제 메커니즘은 testnet-2 세트로
      선착륙, launch set 확정만 ZK.3 창 이관.
- [x] 운영자 플랜 리뷰 정정 5건 반영 (2026-07-19 동일자): ① ZK.0에 S5
      골라잡기(best-of-N seed 선택) 실험 + ADR 8항 완화 확정 신설 — 티켓
      1:1은 재답만 막고 문제 선택은 못 막음 ② 구 ZK.4(4경로 강제)·구
      ZK.5(난이도 밴드)를 ZK.3 창에 흡수, "활성화=최후 커밋" 원칙 명문화
      (활성-미강제 틈 제거), 후속 재번호(ZK.4 miner→ZK.7 삭제) ③ checker
      배포물 전수 갱신 목록(sums 스크립트 FILES 하드코딩·SHA256SUMS·README
      지문·release 계약 테스트) + helper 경로 Boole/Family/로 정정 ④ "위조
      불가" 표현 폐기 → "잘못된 witness를 싸고 결정적으로 거절" +
      underconstraint 정의(공개 입출력·유일성 범위)를 ADR 9항으로 ⑤ 후반
      slice 9-필드 전수 보충. lessons.md에 재발 방지 규칙 5건 기록.
- [x] 운영자 2차 재검수 반영 (2026-07-19 동일자): ① (핵심) 신규 helper를
      CHECKER_PINNED_FILES에 명시 추가 — 지문은 pinned 목록+BooleCheck/**만
      해시(코드 확인, 초판 "전체 해시" 서술 정정), 신규 RED
      checker_artifact_hash_covers_zk_helper 추가 ② 재번호 잔재 4건 정리
      (ZK.2 배선 참조 2건·PM.5 흡수 표기·ZK.0 게이트 수 3→4) ③ ZK.3~ZK.4
      운영 공백에 "운영 경계" 명시(testnet-3 스모크 전용·외부 운영 금지,
      ZK.4 최우선) ④ lessons 3번 규칙 강화(계산 코드 확인 전 커버리지
      서술 금지).
- [x] ZK.0 오프체인 스파이크 실행·**NO-GO 판정 (2026-07-19)**: 하네스
      `scripts/bench/zk_phase0/`(z3-solver 5.0 로컬, 오프체인·paid 없음) 구축·
      실행. `zk-r1cs-underconstraint.v1` 성립 안 함 — S1 FAIL(솔버-불요 O(n)
      propagation attack이 전 밴드 <1ms, 난이도 무영향; Z3 교차검증으로 완전성
      확인), S2 FAIL(비대칭 ~1.7×), S3 monotone-but-trivial, S5 moot. 재설계
      재시도(checkpoint-inversion)도 구조-인지 공격자에 O(1) 붕괴. 리포트
      `local-docs/zk-family-phase0-report.md`. lessons에 "SMT-timeout≠hardness"
      + 트릴레마 규칙 기록. 문서 반영(L1 master §ZK·EXECUTION-ORDER [12]·결정
      로그).
- [x] NO-GO **판정 범위 한정 (2026-07-19 운영자 정정)**: NO-GO는 실측한 두
      설계 — ① 제약 삭제형 feed-forward 회로 ② checkpoint-squaring 재설계 —
      에만 성립한다. "모든 공개·결정적 underconstraint family가 원리적으로
      불가능하다"는 일반화는 실측으로 증명된 바 없으므로 **미검증 가설**로
      강등 (report·README·salvage_probe 주석·lessons 규칙3·EXECUTION-ORDER·
      L1 master 문구 정정).
- [x] 운영자 결정 (2026-07-19): 새 Base 후보 `zk-circuit-uniqueness-dual-cert.v0`
      를 production 구현 전에 Phase 0 오프체인 실험 (§ZK-DC 참조). ADR-0017
      확정·ZK.1 이후 작성은 실험 결과 전 금지 유지.

---

# §ZK-DC — zk-circuit-uniqueness-dual-cert.v0 Phase 0 오프체인 실험 (2026-07-19, 운영자 지시)

핵심 질문: "Hash가 생성한 회로의 출력이 주어진 입력에서 유일한가?" 채굴자는
BUG(대체 witness 반례) 또는 SAFE(D(seed) UNSAT의 LRAT certificate) 중 하나를
제출한다. D(seed) = Circuit(seed, public_input, w) AND output(w) != reference_output.
생성기는 기준 witness 만족만 보장하고 두 번째 witness 존재 여부를 결정·노출하지
않는다 (정답 라벨·mutation trace·alternate witness 심기 금지).

절대 착수 금지 (실험 GO 확정 전): production 코드 / 합의 스키마 / witnessHex·
evidence 변경 / checker pin·SHA256SUMS 변경 / rule version·testnet 범프 / miner
배선 / ADR-0017 Accepted / 기존 ZK.1~ZK.7 실행 / v1-lenbound 삭제.
실행 규칙: 로컬 도구만(cadical·kissat·z3-python·pinned Lean v4.29.1), 유료 API
금지, 기본 run은 임시 결과 파일에 기록(tracked sample 갱신은 명시적 별도 명령),
timeout≠hardness, 실패 결과 그대로 보존.

- [x] DC.0 문서 정정 (docs-only, `316ad7c`): 기존 ZK.0 NO-GO를 candidate-specific
      으로 한정, 보편 명제("결정적 오픈소스 생성기는 …할 수 없다")를 삭제 또는
      미검증 가설로 강등 — tracked: `scripts/bench/zk_phase0/README.md` 헤드라인·
      `salvage_probe.py` 주석·`tasks/lessons.md` 규칙3·본 todo 엔트리 / untracked:
      `local-docs/zk-family-phase0-report.md`·EXECUTION-ORDER·L1 master.
- [x] DC.1 하네스 골격 (`713c1ef`): `scripts/bench/zk_dualcert_phase0/` — XOF 결정적 생성기
      (P0-A Boolean 축소 모델: planted 기준 witness, 관계형 제약, 출력 변수),
      canonical CNF encoder(byte-identical DIMACS), BUG verifier, native LRAT
      checker(파이썬 독립 구현), pinned Lean v4.29.1
      `Std.Tactic.BVDecide.LRAT.Checker` 배선(별도 실험용 Lean 프로젝트, 기존
      `lean/checker` pinned 파일 무변경), 구조 공격자(propagation·의존 그래프·
      자유 변수·GF(2) Gaussian·기준 witness 국소 교란·seed 분기 예측·certificate
      재사용), solver portfolio(cadical --lrat·kissat·z3), benchmark runner,
      self-check 테스트, 재현 README.
- [x] DC.2 S0 게이트 — **PASS** (2026-07-19 full run): byte-identical 회로·CNF
      (독립 프로세스 재실행 포함) + tiny 밴드 80 seeds 전수검사 BUG 58/SAFE 22,
      매 seed 정확히 한 경로 성립, brute-force·솔버·증서 경로 판정 100% 일치.
- [x] DC.3 S1~S7 실측 (P0-A, full: 384 seeds/32 밴드/78분, UNDECIDED 5 별도
      보존) — **S2·S3·S5·S6 FAIL / S0·S7 PASS / S1·S4 부분**: 범위 한정 긍정 =
      구조 지름길 미재현(경계 밴드 구조공격 판정률 42~75%, planted-freedom 누출은
      창발 설계로 고쳐짐) + 밀도 축의 BUG:SAFE 단조 제어(12:0→1:11). 실패 = BUG
      비대칭 0.88×/SAFE 3~10×(목표 100×, CDCL 풀이시간≈LRAT 크기≈검증시간 결합),
      난이도는 랜덤 3-SAT 경계에서만 발생하며 SAFE 증서 44~72MB·Lean 1.1~1.8s·
      RSS ~330MB로 폭발(S6), min-of-1000 골라잡기 이득 최대 270×·BUG 100% 수렴·
      통제책 없음(S5).
- [x] DC.4 P0-B — **미착수 확정**: 스펙 규칙 "P0-A 전 게이트 통과 시에만 진행"
      에 따라 착수하지 않음 (P0-A 게이트 실패).
- [x] DC.5 산출물: raw JSON `local-docs/zk-dualcert-phase0-raw-2026-07-19.json`
      (384 seeds 전량·환경·버전·timeout 별도 보존) + 커밋 샘플
      `result.sample.json`(명시적 별도 명령으로 갱신) + 리포트
      `local-docs/zk-dualcert-phase0-report.md` — 첫 줄
      **`NO-GO — ZK Base 후보 폐기`** (candidate-specific, 재설계 비추천 근거 포함:
      S2/S6은 resolution 증명크기 하한과 LRAT 선형 검증의 구조적 결합, S5는
      창발 답+공개 seed 선택에 내재. "ZK 전체 실패" 아님 — 처분 = Rust/Aeneas
      별도 Phase 0(하네스 재사용) + ZK는 Bounty lane·장기 recursive-chain-proof
      이동, v1-lenbound 임시 안전망 유지, ADR-0017 미확정 유지).
- [x] DC.6 커밋 게이트 완료: PR #87 squash 머지 `2a9c912` (CI self-test·
      supply-chain·corpus 4-env·verdict-corpus 전부 green), remote 검증
      (local==origin/main), 한국어 최종 보고 (public/API benchmark claim 아님
      명시).
- [x] base-lane 방향 논의 종결·배치 합의 (2026-07-19, 텔레그램·터미널 3라운드):
      transform.v0은 Base 부적격 확정(리뷰 정정 2건 수용: succinct 랩핑
      가능성·개정 R4 반영, 결론 불변) → Bounty 상품으로 재배치. **운영자
      명확화 3건 합의·정의 고정**: ① "외부 가치" = Base 결과를 사용하는 독립
      소비자 존재 + 소비자의 계산/신뢰 비용 실감소 (Boole 이용 지갑·브리지
      포함; 판정은 산출물로 — boole-light/브리지 verifier/안전한 pruning 중
      1개가 실제 proof 소비) ② 오프체인 대체 테스트 → 보증 프리미엄 테스트로
      정정(중앙/서명만/L1 보증 3층 비교, corpus 제품 KPI — 합의 규칙 아님)
      ③ PoVFN = 목표 아키텍처 **후보**·채택 미결정, 게이트 = PoVFN Phase 0,
      v1-lenbound 제거는 GO 후에만. **최종 배치**: Base 후보=PoVFN / Bounty=
      실물 회로·Rust·EVM 최적화 / Corpus 제품=산출물 정제(오프체인) / 연구
      후보=초최적화 family / Hash=초기 sybil·박자·재편성 방어. AI 성능 판정은
      온체인 금지, 오프체인 승격 게이트 전용. 문서 = local-docs 탐색·비교·평가
      3건 + EXECUTION-ORDER 결정 로그 (2026-07-19).
- [x] PoVFN Phase 0-A 착수 (2026-07-19 운영자 지시문 + "즉시 착수해", 지시문
      검증 회신의 해석 4건 포함: P단계 상한=export 결박+커널 검사(elaborator
      비증명)·환경 커밋 분리 측정·재귀 미실측 시 배치 실측만으로 GO 판단·
      reward pk 변조는 ZK 공개입력 층).

---

# §PoVFN-A — Proof-of-Verifiable-Full-Node Phase 0-A (2026-07-19 운영자 지시문)

핵심 질문: "Boole의 실제 Lean 검증을 블록 생성 속도에 맞춰 작은 ZK proof로
압축할 수 있는가?" 하네스 `scripts/bench/povfn_phase0/`, 보고서
`local-docs/povfn-phase0-a-kernel-zkvm-report.md` (첫 줄 GO/REDESIGN/NO-GO).
금지: 합의 코드/checker pin/스키마/testnet 변경, PoVFN 채굴권·보관 구현,
기존 Base 삭제, 공개 성능 주장, Lean 커널 신규 구현. K단계(커널 항 검증)와
P단계(package 전체 검증)를 절대 혼동 금지.

- [x] A1 §1 검증 경로 조사 완료 — proof package=86바이트 POFP-v2(소스는 seed
      재유도), pinned 경로=`lake exec boole_check`+Audit, guard=토큰 11종·
      import 화이트리스트·16KB/1024decl 한도, 블록 박자=60s·k_max 4 (보고서
      §1에 파일 경로와 함께 기록).
- [x] A1 §3 호환성 — lean4export(v4.29.1 오버라이드, format 3.1.0) +
      nanoda_lib(f58f2f6, 패치 2건 기록) + leanchecker 3자 차등: 실 fixture·
      합성 seed 전 케이스 판정 일치, false accept 0, 변조 매트릭스(명제 치환·
      리터럴·손상·절단·axiom 주입·거짓 명제·예산 초과) 전부 정상 거절. 핵심
      실측: exit code만으론 P단계 결박 불가 → 기대-명제 구조해시 검사 구현.
- [x] A2 §4 zkVM 게스트 — RISC Zero 3.0.6 고정, nanoda 게스트 이식(직렬 경로,
      reader 패치), journal 결박 9필드+명제 구조해시, 변조 flip·fail-closed
      (게스트 panic=proof 불가) 확인.
- [x] A2 §5 밴드 — Real 364KB/206decl: native 8.7ms → zkVM 244,211,417
      cycles/3.09s(실행) / Syn1 4.36MB: 3.45B cycles. **composite proving은
      CPU에서 ≥3,690s 하한에서 운영자 지시로 중단**
      (`operator_cancelled_cpu_budget` — proof 실패·암호학적 NO-GO 아님).
      verify 시간·proof 크기·succinct·GPU = 미측정으로 정직 기록.
- [x] A3 §6 배치·재귀 — 미실측 (중단 지시로 미착수, 산술 유도만 분리 기록).
- [x] §7~8 판정·보고서 — **`REDESIGN — PoVFN Phase 0-A` (예비)**: 실행·호환성·
      결박 성공 / CPU proving 블록 예산 246×+ 초과. 리포트
      `local-docs/povfn-phase0-a-kernel-zkvm-report.md`, raw
      `local-docs/povfn-phase0-a-raw-2026-07-19.json`, 커밋 샘플
      `scripts/bench/povfn_phase0/result.sample.json`. §9 준수: 후속(게스트
      최적화 재실측/GPU 실측/폴백 범위 축소)은 전부 **운영자 결정 대기** —
      자동 진행 없음, Base family·합의 코드 무변경.

---

# §LI-P0 — lean-library-improvement.v0 결정 페이퍼 (2026-07-20, 운영자 지시)

- [x] LI-P0-paper 작성 완료 — **첫 줄 판정: `REDESIGN-PAPER`**. 9개 결정사항
      중 8개 확정(역할=보상 가중 레인·liveness는 Hash / 강제 배정=seed 전
      등록+H(seed‖pk) / 사전식 점수(axiom 무·재검증 통과 입장권 → export
      bytes ↓ → deps ↓ → heartbeats ↓, novelty·실행시간 금지) / epoch 원자적
      baseline+commit-reveal / 조작 방지 7종 / native Lean 판정+예산 캡 /
      PoVFN=CPU 인프라 분리·GPU 전면 불사용 / corpus=오프체인 KPI / 하네스
      kill gate 6종). **승패 게이트인 공급 지속성은 현 자산 기준 정량 부정**:
      lenbound corpus는 일반 합성 lemma 1~2개로 전 family 증명이 붕괴해 첫
      몇 epoch 내 포화, Bounty 유입 0 — snapshot 없이는 1년 불성립. 필요
      결정: 재설계 R("무한 자율 공급"→"고갈 시 우아한 침묵+수요-결합") +
      결정 A(역할 재정의 수용) + 결정 B(genesis snapshot: protocol-owned vs
      hash-고정 스냅샷). R 거부 시 REJECT-AS-BASE 전환. 페이퍼
      `local-docs/lean-library-improvement-p0-paper-2026-07-20.md`.
- [x] 운영자 결정 (2026-07-20): **재설계 R 거부 → 전환 조항에 따라
      `REJECT-AS-BASE` (후보 한정) 확정.** 근거: R 수용 = "외부 수요 없이
      지속 작업을 만드는 Base" 요구사항의 포기이며, snapshot은 무한 공급의
      해결이 아니라 포화 지연(유한·조회 재제출·검색 우위·외부 저장소 불사용
      의도 약화). 처분: 객관 점수·baseline·조작 방지 설계는 **Bounty·corpus
      제품의 선택적 LI 레인**으로 보존 / snapshot 수입·LI 하네스는 Base 목적
      미진행 / Bounty corpus 축적 후 비합의 보상 레인으로 재검토. 문서 정정
      4건 반영(고정 비율 예산 제거·p95/최악 기준·의존성 폐포 전체 측정·전
      비용 항목 비증가 조건). **현재 상태 (운영자 확정)**: PoVFN=검증·정산·
      보관 인프라 후보(CPU 전용) / LI=Bounty·corpus 개선 레인 / Hash=임시
      liveness·보안 바닥 / **최종 Base Family=미결정** — Hash+인프라를 최종
      Base로 확정하지 않음. 다음 트랙(신규 Base 후보 탐색 / Bounty 수요 /
      PoVFN CPU 인프라 연구 / LI-Bounty 레인 설계)은 운영자 지시 대기.

---

# §SP-P0 — zk-formal-selfplay.v0 결정 페이퍼 (2026-07-20, 운영자 "진행해")

- [x] 후보 탐색·정제: selfplay-conjecture(양방향 증명 시장 — 문제 출처를
      공개 생성기/고정 corpus/체인 자신이 아닌 **채굴자 간 커밋된 비밀**로)
      → 운영자 ZK 도메인 특정(`zk-formal-selfplay.v0`: 체인이 유한체·다항식·
      R1CS·range check·commitment 재료 배정, LLM 출제자가 정리+증명 커밋,
      해결자 무작위 배정, pinned Lean 판정). 선행 실증: STP(barely-provable
      커리큘럼 무한 생성, LeanWorkbook 2배)·MINIMO(공리만으로 자가 생성)·
      DeepSeek-Prover(합성 corpus 성능 향상)·PSV(Rust/Verus 확장 경로).
- [x] SP-P0-paper 작성 — **첫 줄 판정: `PROCEED-TO-HARNESS`** (채택 아님,
      선결 2건 + kill gate 9종 전제): 영합 코어 게임 분석(담합·보류·티켓
      소각·자기해결 전부 대수적 적자 — 해결 보상 원천=출제자 본드 한정 +
      생존 보조금 α<티켓비용 캡), 공급 분석(LI형 페이퍼-반증 불성립 — 명제
      공간은 조합적 고갈 불가, 최전선은 해결자와 동행), ZK 기초 라이브러리
      v0 스펙(protocol-owned 300~800줄·신규 axiom 0·암호 경도 axiom 불가라
      도메인="ZK 구성물의 대수 수학" 정직 라벨), 명제 정규화(stmt_hash 재사용
      — de Bruijn 알파-동치 공짜)·캡, LI 기계 전부 상속(보상 가중 레인·epoch
      원자 정산·commit-reveal·p95 예산). 페이퍼
      `local-docs/zk-formal-selfplay-p0-paper-2026-07-20.md`.
- [x] **운영자 반증·판정 전환 (2026-07-20)**: 초판 §3-7이 p_surv·p_solve를
      독립 취급한 수식 오류 — 결합식 p_surv=(1−p_solve)^N + 해결 참여 조건
      + 소각 방지 α<c_t를 넣으면 (1+λ)e^(−λ)≤1에 의해 **영합 코어(본드-원천
      보상)에서 정직 출제 EV가 전 파라미터 음수** (수치 격자 전수 재검증으로
      일반화 확인). 판정 **`REDESIGN-PAPER`로 정정** (§13 부록). 추가 결함
      4건 수용: 못-푸는-문제 편향 보상·티켓≠노력·reveal 규칙 불명(→항상
      reveal)·명제 공간 크기는 품질 근거 아님. family는 유효 — 경제 1회
      재설계.
- [x] 재설계 ① 고정 epoch 발행 경제 페이퍼 완료 (2026-07-20 운영자 "진행해",
      페이퍼 §14): 본드=처벌 전용·발행 고정·밴드 피크 보상·항상 reveal·분모
      고정·이연 정산. **초판 불가능성의 해소 = 결합 절단**(해결이 출제자
      비용이 아니라 밴드 명중 시 보상 근거). 닫힌 생존 조건식 W_P>K(f_p+c_gen)
      ∧ W_S>K·N·c_t + 예시점 3개 수치 확인. 담합 재봉쇄: C1 밴드 제조·C4 분모
      희석은 무작위 배정+분모 고정으로 이항 꼬리 바닥(N=24·φ=0.3에서 3.1%,
      N=48에서 0.29%; **ρ*=0.5가 양방향 조작 바닥 동시 최대화**), C2
      사보타주는 닫힌 형 없음 → 시뮬레이션 1차 대상. 적대적 자문 수행(판정을
      죽일 대수 불가능성 없음 확인). 재설계판 판정 = **PROCEED-TO-HARNESS
      조건부** (1단계 시뮬레이션 게이트 통과 전 2단계 금지).
- [ ] 재설계 ② 하네스 1단계: 에이전트 시뮬레이션 (로컬 파이썬, LLM 불요) —
      파라미터 스윕으로 "정직 출제·해결 EV>0 ∧ C1 운-대기 선택·C2 사보타주·
      C4 희석·혼합 전략 EV<정직" **동시 만족 영역** 확인. 영역 없으면
      REDESIGN 회귀. 착수는 운영자 승인 대기.
- [ ] 재설계 ③ (1단계 통과 시에만): 최소 ZK 라이브러리(유한체+간단 회로만)
      + 소형 LLM 스파이크. 전 과정 코드·합의·checker pin·기존 Base 무변경.

---

# §ZK-POR — zk-proof-or-refute.v0 Phase 0 (2026-07-20, 운영자 지시문 "시작해")

- [x] 사전등록 결정 5건 + 실험자-참가자 규율 동결
      (`local-docs/zk-proof-or-refute-phase0-prereg-2026-07-20.md`): 제출물
      문법 계약(∀+전제→결론, 반례=값+전제성립증명+결론부정증명 3종, 중첩
      existential 제외) / 모델 등급(AUTO·gemma4:26b·claude-fable-5=본 CLI,
      Claude 출력은 S3 전용·S9 학습 제외→C·D 둘 다 로컬 모델) / 킬 순서
      S0→S1→S2→S4/S5→S3→S6/S7→S8→S9 / 30문제 파일럿 / S9 중앙 합성 C 정의.
- [x] 하네스 `scripts/bench/zk_proof_or_refute_phase0/`: protocol-owned ZK
      라이브러리(Fp·비트·range·boolean·다항식·R1CS) + 커널 검증 기초 정리
      20개(pinned v4.29.1, 기존 checker 무변경) / 결정적 mutation 생성기
      (라벨·증명·반례 미심음) / 독립 brute-force oracle(측정 전용) / 실제
      intake 규율 Lean 검증기 / 고정 자동화 포트폴리오 + S1 구조 공격자.
- [x] **판정: NO-GO — zk-proof-or-refute.v0 (후보 한정, S2 조기 종료)**.
      S0 PASS(결정성) / S1 PASS(구조 공격자 라벨 정확도 0.65 < 다수 0.72 <
      임계 0.90 — 누출 약함) / **S2 FAIL(자동화 92%, 전 밴드 ≥90%, 라벨 100%
      판정 — 임계 50%)**. 근본 원인: 소형 유계-산술 ZK 라이브러리는 결정절차
      (omega/decide+열거)의 본진이라 자동화가 천장을 침 — 자동화 압도 난이도는
      무계·비선형·중첩 existential에서만 나오나 사전등록 v0가 배제(동결). S3~S9
      = `not_run_due_to_preregistered_early_kill`. "모든 유용 Family 불가능"
      확대 금지. 보고서 `local-docs/zk-proof-or-refute-phase0-report.md`, raw
      `...-raw-2026-07-20.json`. 재도전은 범위 확장(무계/회로 충족성)의 신규
      사전등록 필요 — 본 판정 사후 수정 아님. 코드·합의·checker pin·기존 Base
      무변경.

---

# §SHA3-IV — 독립 검증자 확보 (2026-07-30, SHA3-INDEPENDENT-VERIFIER-P0)

- [x] 봉투·승인 — proposal `deaf2a0d…` / envelope `988089aa…6b2c` /
      운영자 승인 텔레그램 message_id 3181 / 승인 이벤트 `74b86f6e…a2cc`.
      실행 전 발급 digest 8종·동결 본문 2종 무이동 재계산 확인.
- [x] 격리 작성자 — 신선한 문맥 서브에이전트가 계약 꾸러미
      (`sha3-independent-verifier-author-input.json`, `c2adb3ba…f514`)만
      읽고 검증자 B(`sha3_verifier_b.py`, `701cc6fc…2371`) 작성.
      도구 사용 2회(읽기 1·쓰기 1), 구현 A 소스·fixture 바이트·기대 판정
      비노출, 어시스턴트의 B 바이트 수정 0.
- [x] RED→GREEN — `tests/test_sha3_independent_verifier.py` 11/11
      (fixture 무결성 중단 / 불일치 비삼킴 / 합의 기준 / flip 규칙).
- [x] 교차 판정 — 1단계 fixture 9/9 일치(실패 관문 집합까지 동일),
      2단계 동결 본문 2/2 일치(proved·공리·정리 이름 동일), 음성 대조
      거절 실증(exit 1·공리 보고 없음). `independent_verifier_available
      = false → true`. 기록 `sha3-independent-verifier.json`(`21f42773…`).
- [x] 커서 갱신 — EXECUTION-ORDER.md ▶ 2026-07-30 / L1 master 착륙 절.
      상태 블록 불변: STRICT_READY=0(남은 사유 = 블라인드 표본 없음 하나) /
      REWARD_READY=0 / RP0-MD=HOLD / BF7=HOLD.

## Review
- **결과**: 공유 구현 오류라는 실패 유형 하나를 실측으로 좁힘. 한계 5건
  기록 유지(같은 Claude 계열 작성자·같은 기계·공유 커널·같은 언어·격리는
  하네스 강제). 제출물이 새로 참이 된 것 아님, solved 아님.
- **게이트**: local-docs 전용 research 슬라이스 — 커밋/푸시/CI 없음,
  Lean 컴파일 6회(상한 14), 새 모델 실행 0, 네트워크 0, paid API 0.
- **claim boundary**: closed local 검증. public mining/benchmark claim 아님.

---

# §SHA3-BS: 깨끗한 블라인드 표본 1건 (SHA3-BLIND-SOLVER-P0, 2026-07-30)

승인: 봉투 `dfd2f4e3…e738` → 운영자 "승인"(텔레그램 message_id 3185) →
승인 이벤트 `186b3c08…df7b`. 실행은 승인 후에만.

- [x] 실행 전 무결성 — 발급 digest 8종 재계산 일치, 동결 프롬프트
      `83a85844…`, 검증자 B `701cc6fc…`, 교차 기록 `21f42773…` 무이동,
      핀 Lean v4.29.1 읽기 확인.
- [x] 입력 동결 — preamble(`4be56ef8…`, 문제 내용·힌트 0) + packet
      = 전송 바이트 `1318c4fc…`(11,935B), 실행 전 동결.
- [x] RED→GREEN — `tests/test_sha3_blind_solver.py` 23/23
      (블라인드 무효 규칙 / 무응답 규칙 / A·B 불일치 비삼킴 / 판정표 5라벨).
- [x] solver 1회 — 신선한 문맥 서브에이전트, 단일 턴, 도구 호출 0회
      (usage + transcript 이중 관측), 전달 프롬프트·동결 응답 모두
      transcript 재해시로 바이트 일치 확인, 재시도 0.
- [x] 파이프라인 — intake whole_answer → 정규화 꼬리 개행 1건 →
      A1 admit = B1 admit → 커널 A proved = B proved,
      공리 [propext, Quot.sound] ⊆ 허용 3종. 판정 = **BLIND-PROVED**
      (사전 등록 판정표). solved 아님. 기록 `sha3-blind-solver.json`
      (`ed5d1979…8f06`).
- [x] 커서 갱신 — EXECUTION-ORDER.md ▶ 2026-07-30 / L1 master 착륙 절.
      상태 블록 불변: STRICT_READY=0(마지막 사유 해소 — 게이트 인상은
      운영자 결정 대기) / REWARD_READY=0 / RP0-MD=HOLD / BF7=HOLD.

## Review
- **결과**: 깨끗한 블라인드 표본 1건 확보 — 신선한 문맥·꾸러미만 입력·
  도구 0회(기계 관측)·사람 손 0회·사전 등록 규칙. 표본의 내용은
  BLIND-PROVED. 한계 6건 기록 유지(같은 Claude 계열 생성자·오염 확인
  불가·재현 불가·플랫폼 system prompt·관측 기반 도구 0회·같은 기계).
- **게이트**: local-docs 전용 research 슬라이스 — 커밋/푸시/CI 없음,
  Lean 컴파일 4회(상한 6), 커널 검사 2/2, gemma4 0, 네트워크 0, paid API 0.
- **claim boundary**: closed local 검증. public mining/benchmark claim 아님.

---

# §SR-PROMO: STRICT_READY 0→1 승격 (STRICT-READY-PROMOTION-P0, 2026-07-30)

이중 관문: 봉투 d19760bb…(3191) + 정정 ccd45943…(3193) → chain proposal
c6e6730e…(3195). 상태 블록 손 편집 0 — 전부 사슬 기계가 갱신.

- [x] 무결성 — 발급 8종·FIPS PDF·verify-final green·상태 블록 현행값.
- [x] 조항 대조 — 전사 조항 5/5 raw 실재, 인용문 포함 기록
      (sha3-spec-clause-crosscheck.json 51d53bfa…). 직전 브리프의
      "원문 미결박 공백"은 오독으로 정정(lessons.md 기록).
- [x] RED→GREEN — tests/test_strict_ready_promotion.py 9/9
      (strict_ready만 변경·필드 보존·추가 키 3개 한정·self hash·이중 승격 거부).
- [x] 권위 승계 — 원본 노드 불변, 차세대 v2(7c86a6bd…) 새 경로 배치,
      promotion 블록에 6조건 증거 digest 내장.
- [x] fail-closed 2건 정직 처리 — 노드 digest 보호/계약 값 고정에 걸림 →
      원상 복구(green) → 정정 봉투 승인 → 생성기 _status_contract 상수
      2개만 수정(versioned_snapshot 새 세대로 chain 승인 행 표면화).
- [x] 사슬 절차 — propose → verify-proposal ok(경고 0) → 운영자 승인(3195)
      → apply → 문서 3개 기계 갱신 → verify-final ok (9세대, tip 89940142…).
- [x] 결과 — STRICT_READY=1, REWARD_READY=0/BF7=HOLD/RP0-MD=HOLD/Base=false
      불변, 정책 필드 0건 변경. 기록 strict-ready-promotion.json(2d8da04d…).

## Review
- **결과**: 기준 6조건 통과 패킷 1건 존재가 상태 계약으로 결박됨. REWARD
  주장 아님, 공급 규모 주장 아님, 표본/검증자 한계 불소거.
- **게이트**: local-docs 전용 — 커밋/푸시/CI 없음, 네트워크 0, 모델 실행 0,
  Lean 0, focused 테스트 2회(상한 8), PDF 추출 1회(상한 3).
- **claim boundary**: closed local 검증. public mining/benchmark claim 아님.

---

# §S4: 독립 replay·변조·자원 계약 (SHA3-S4-STRICT-REPLAY-P0, 2026-07-30)

승인: 봉투 89c13afe…(message 3199) → 이벤트 705045de…. 판정
**S4-PASS-BUDGET-PENDING** (사전 등록 판정표).

- [x] 무결성 — 발급 8종·본문 3건·검증자 B·chain verify-final 전부 일치.
- [x] RED→GREEN — test_independent_replay + test_resource_contract 25/25
      (변조가 진짜 결함인지, 불일치 비삼킴, timeout=RETRYABLE, 판정표).
- [x] 자원 계약 동결 — resource-contract.json(3176d135…): 결정 7항목 /
      억제 2항목 분리, step meter 부재 = BUDGET_PENDING 명기.
- [x] 독립 replay 3/3 일치 — 컴파일 단위 digest 일치 선행, A·B 새로
      실행, 관문·공리·판정 동결 기록과 동일.
- [x] 변조 8/8 거절, false accept 0 — 변조본 원장 저장 0 (scratch 전용).
- [x] 기록 — sha3-s4-strict-replay.json(41582a80…),
      raw-strict-results.json(07dd090c…). 커서 3곳 갱신.

## Review
- **결과**: strict 판정이 재현 가능하고(3/3), 훼손에 저항하며(8/8),
  판정 결정 항목이 전부 결정적임을 실측. 미결 1건 = 합의용 step meter
  부재(BUDGET_PENDING) — STRICT_READY 재평가 여부는 운영자 판단 상신.
- **게이트**: local-docs 전용 — 커밋/CI 없음, lean 16/24, 커널 8/8,
  모델 실행 0, 네트워크 0, 상태 블록 무접촉, focused 테스트 2회(상한 10).
- **claim boundary**: closed local 검증. public claim 아님.

---

# §S5: 정확한 보상 자격 회계 (SHA3-S5-REWARD-ACCOUNTING-P0, 2026-07-30)

승인: 봉투 16b13473…(message 3209) → 이벤트 08bdaac6…. 판정
**S5-EXACT-ACCOUNTING** — 예고대로 숫자 불변, 회계 실측이 산출물.

- [x] 무결성 — 발급 8종·동결 5행·승격 기록·chain green.
- [x] RED→GREEN — tests/test_reward_accounting.py 13/13 (자동화 계상·
      부풀리기·뭉개기·답변만 solved·replay 계상·합계 오류 전부 거절).
      schema 동결(schemas/reward-result.schema.json).
- [x] 자동화 탐침 5종 — decide/rfl/omega/simp/simp_all 전부 실패
      (timeout 0, lean 5/12). 즉시 자동화로 안 풀림(전수 내성 주장 아님).
- [x] 회계 — 6행 합계 정확(NEEDS_SPEC 4·SOURCE_UNAVAILABLE 1·
      STRICT_READY 1), 중복 6→6, reward 0 ≤ strict 1. 소비된 작업 규칙
      첫 실적용(SHA3 답 = 원장 공개 → current stock 0). LLM 축
      INCONCLUSIVE-MODEL-DIVERSITY 병기.
- [x] 기록 — reward-results.json(eb99ee4f…),
      sha3-s5-reward-accounting.json(d65f216a…). 문서 4곳 갱신
      (EXECUTION-ORDER·L1 master·thesis §20.11·본 파일).

## Review
- **결과**: 부풀리기 방지 회계가 실측으로 섰다. reward-ready 0은 정직한
  결과 — 소비된 작업 규칙과 모델 다양성 미결이 근거로 기록됨.
- **게이트**: local-docs 전용 — 커밋/CI 없음, lean 5/12, 모델 0,
  네트워크 0, 상태 블록 무접촉, focused 2회(상한 10).
- **claim boundary**: closed local 검증. 보상액/가격 결정 아님.

---

# §S6: RP.5 흡수·RP.6 입력 (SHA3-S6-RP5-HANDOFF-P0, 2026-07-30)

승인: 봉투 3e7fdcbb…(message 3213) → 이벤트 da4903cb…. 판정
**S6-HANDOFF-COMPLETE** — RP.A2-STRICT-REWARD track S0~S6 마감.

- [x] 무결성 — 발급 8종·S4/S5/승격 기록 digest 전부 일치.
- [x] RED→GREEN — tests/test_rp_handoff.py 13/13 (이중 계상·전수인데
      외삽·strict→reward stock·소비 답→unsolved stock·상태 합≠5·hash
      불일치 전부 거절).
- [x] 흡수 — census 규칙(5<120 → 전수) 적용, 재실행 0, evidence hash
      7건 재계산 일치, 이중 계상 0.
- [x] 동결 — rp5-circom-handoff.json(1bb0bb6e…),
      result-summary-input.json(b267e30f…, stock/flow 분리 + 미결 2건
      동반), checkpoint-rp5.md, sha3-s6-handoff.json(4fa6f524…).
- [x] 문서 4곳 갱신 (EXECUTION-ORDER·L1 master·thesis·본 파일).

## Review
- **결과**: S0~S6 wave 마감. RP.6이 받을 입력이 추정 없이 동결됨.
  다음 관문(RP.6/RP.7/RP.A3)은 전부 미시작·별도 승인.
- **게이트**: local-docs 전용 — Lean 0, 모델 0, 네트워크 0, 커밋 0,
  상태 블록 무접촉, focused 2회(상한 8).
- **claim boundary**: closed local 검증. public claim 아님.
- **adoption 추적**: 유료 검증 구매자/LOI 수 0 (비게이트 지표).

---

# §RP6: 활주로 계산 (RP6-RUNWAY-P0, 2026-07-30)

승인: 봉투 fdbe4275…(message 3218) → 이벤트 12af2f64…. 판정
**RP6-CALCULATED**.

- [x] 무결성 — S6 입력(b267e30f…)·발급 8종 일치.
- [x] RED→GREEN — tests/test_runway.py 26/26: 원판 RED 10종 전부 거절
      + 손계산 golden 12/12 (10,950 앵커 포함) + 실입력 compute 3종.
- [x] 계산 — 실효 재고 0(실측) → 전 소비율(1~1,000/day) 활주로 0년,
      stock_only_3y 0/day, 유입 축 NOT_MEASURED(관측 창 미실행 —
      유입률을 지어내지 않음). Fraction 정밀 연산, float 없음.
- [x] 동결 — result-summary.json(9248e117…), schemas/result.schema.json,
      rp6-runway.json(9ef77187…). 문서 4곳 갱신.

## Review
- **결과**: "공급이 아직 성립하지 않았다"가 공식·검산 가능한 형태로
  못박힘. 공급 불가의 증명 아님, BF.7 판정 아님.
- **게이트**: local-docs 전용 — Lean 0, 모델 0, 네트워크 0, 새 측정 0,
  커밋 0, 상태 블록 무접촉, focused 2회(상한 8).
- **claim boundary**: closed local 검증. 가격/보상률 결정 아님.

---

# §RP7: closed-local 최종 판정 (RP7-FINAL-REPORT-P0, 2026-07-30)

승인: 봉투 3f8e5d3e…(message 3222) → 이벤트 b9d52f54…. 판정 첫 줄
**HOLD-BF7-SUPPLY** — 원판 RP.0~RP.7 파이프라인 종결.

- [x] 무결성 — RP.6 산출물 4종·발급 8종 digest 일치.
- [x] RED→GREEN — tests/test_attack_matrix.py 20/20: 공격 11종 각각
      고유 typed rejection + 깨끗한 모집단 통과 + §3.9 판정 매핑 +
      금지 문구 검사.
- [x] 재현성 — evaluate_results.py 새 프로세스 2회, 산출 바이트
      byte-identical (2b48c8b8…).
- [x] 입력 hash — rp7-frozen-inputs.sha256 6/6 재계산 일치
      (S0 동결 원본 무수정 — 신규 파일).
- [x] 보고서 — report.md 첫 줄 HOLD-BF7-SUPPLY, 경계 첫 장 반복
      (전체 중단 아님·출시 승인 아님), 세 소비율(중앙=실측·95% 밴드
      부재 명시), Economic ADR daily cap 0, 금지 claim 0 기계 검사.
- [x] 기록 — rp7-final-report.json(520df683…), raw-result.json,
      문서 4곳 갱신.

## Review
- **결과**: 측정 완전·재현 가능·조작 방어 실증 위에서 공급 조건 미충족이
  공식 판정으로 확정. HOLD의 경계가 기록 전반에 반복 명시됨.
- **게이트**: local-docs 전용 — 새 측정 0, Lean 0, 모델 0, 네트워크 0,
  커밋 0, 상태 블록 무접촉, focused 2회(상한 8).
- **claim boundary**: closed local 검증. public claim 0 (기계 검사).

---

# §SR-CORR: 감사 반영 정정 (STRICT-READY-CORRECTION-P0, 2026-07-31)

이중 관문: 봉투 aab30887…(3227) → chain proposal 02e1266a…(3229).
판정 **CORRECTED-TO-BUDGET-PENDING**. 과거 기록 무수정 — 전부 successor.

- [x] 교차 관문 신설 — tests/test_cross_gates.py: 정정 전 원장에서
      RED 3건(BUDGET_PENDING인데 STRICT_READY=1 위반 2건 + 라벨 -MD 아님
      1건) = 감사 지적의 기계 증명 → 정정 후 9/9 GREEN.
- [x] 권위 v3 — strict_ready 1→0, correction 블록(계획서 근거 행·SHA3
      state=BUDGET_PENDING·유효 잔존 목록) 내장. 사슬 10세대, 문서 3개
      상태 블록 STRICT_READY=0 복귀, verify-final ok.
- [x] 회계 v2 전파 — reward-results-v2(BUDGET_PENDING 1행, strict 0),
      result-summary-input-v2, result-summary-v2 (활주로 수치 불변).
- [x] RP.7 정정판 — report-v2.md 첫 줄 HOLD-REPLENISHMENT-P0-MD,
      BF7 매핑 분리, "재고 측정 완료·유입 미측정" 문구, R4 reward 공식
      병기, raw-result-v2 determinism 2회 byte-identical, 금지 문구 0.
- [x] 기록 — strict-ready-correction.json(3014cf75…). 문서 4곳 갱신.

## Review
- **결과**: 감사의 최소 정정안이 그대로 반영됨. 유효 결과는 전부 보존,
  잘못된 상태표와 하류 회계만 successor로 교체. 같은 종류의 단계 간
  모순은 교차 관문이 상시 감시.
- **다음 기술 작업**: 결정적 실행량 측정기(step meter) — BUDGET_PENDING을
  닫는 유일한 경로 (별도 설계·승인).
- **게이트**: local-docs 전용 — 새 실험 0, Lean 0, 모델 0, 네트워크 0,
  커밋 0, focused 3회(상한 10).

---

# §SM: step meter 채택 (STEP-METER-ADOPTION-P0, 2026-07-31)

승인: 봉투 bca8a021…(message 3233) → 이벤트 5cb68182…. 판정 **S4-PASS**.

- [x] 발견 — 측정기는 노드에 이미 완비(ADR-0016/SC.9a, boole-lean-runner):
      합의 상수 400,000/512 주입, 3상태 판정, budget_exceeded/override
      결정적 거절. BUDGET_PENDING의 실체 = 원장 harness 미채택.
- [x] RED→GREEN — tests/test_step_meter_adoption.py 11/11 (상수 읽기
      거부 규칙·예산 인자 주입·초과=결정적 거절(RETRYABLE 아님)·override
      정적 차단·2회 불일치=판정 금지).
- [x] 실증 — 양성 3/3 수락(합의 예산, 각 2회 일치, 공리 허용), 음성
      대조(예산 1) budget_exceeded 결정적 거절, override 컴파일 0 차단.
      lean 8/14, 커널 4/8. 노드 코드·동결 모듈 무수정.
- [x] 동결 — resource-contract-v2(2d39171e…, budget PASS + forbidden
      options 추가), sha3-s4-strict-replay-v2(ba749819…, S4-PASS),
      step-meter-adoption.json(b309db2d…). 문서 4곳 갱신.

## Review
- **결과**: L359의 pending 조건이 해소돼 strict-ready 6조건이 처음으로
  전부 충족 가능. 재승격은 사슬 이중 잠금의 운영자 결정으로 남김.
- **한계**: 예산 축은 단일 계열 실증(검증자 B는 예산 인자 없는 동결본).
- **게이트**: local-docs 전용 — 커밋/CI 없음, 모델 0, 네트워크 0,
  상태 블록 무접촉, focused 2회(상한 8).

---

# §SR-RE: 요건 충족 재승격 (STRICT-READY-REPROMOTION-P0, 2026-07-31)

이중 관문: 봉투 ce3f9d1a…(3237) + chain proposal 55cbeccf…(3239).
판정 **REPROMOTED-STRICT-READY-1**.

- [x] 요건 확인 — L8(S4-PASS 후 승격)·L359(pending 부재)·L183(6조건).
- [x] 교차 관문 일반화 — budget_gate 순수 함수, pending+1=False 합성
      증명, 최신 세대 해석. 10/10 GREEN (완화 없음).
- [x] 권위 v4 — strict_ready 1, 세대 4(모듈 고정값 부모+1 보정·기록),
      correction 역사 보존. 사슬 11세대, 상태 블록 STRICT_READY=1.
- [x] 회계 v3 — reward-results-v3(STRICT_READY 1행, strict 1, reward 0),
      summary-input/summary/raw-result/report v3. 활주로·판정 불변
      (HOLD-REPLENISHMENT-P0-MD).
- [x] 실행 중 결함 자체 발견·수정 — v3 emitter의 state 하드코딩 상속 →
      회계 파일 읽기로 교체, 3중 일치(in-proc 2 + subprocess 1, 상한
      3/3 준수) 재검증, 경위 기록.
- [x] 기록 — strict-ready-repromotion.json(d6def55b…). 문서 4곳 갱신.

## Review
- **결과**: 상태표가 계획서와 정합인 안정 상태 도달. 공급 증거는 불변 —
  바뀐 것은 정합성이다. 하루 동안 승격→감사→정정→요건충족→재승격의
  전 과정이 successor 기록으로 추적 가능.
- **게이트**: local-docs 전용 — Lean 0, 모델 0, 네트워크 0, 커밋 0,
  focused 4회(상한 10).

---

# §HYG: 기계용 기록 위생 정정 (LEDGER-SUCCESSOR-HYGIENE-P0, 2026-07-31)

승인: 봉투 722f038c…(message 3245) → 이벤트 47e5388f…. 판정
**HYGIENE-COMPLETE** — 2차 감사 4건 전부 확인·정리, 판정 번복 0.

- [x] 교차 관문 3건 신설 — 신설 시 RED 3건(지적의 기계 증명):
      집계 open_items↔budget 정합 / 기계용 rp7 verdict∈-MD /
      계약 budget 승인 출처↔채택 이벤트 → 정정 후 13/13 GREEN.
- [x] 집계 v4 — BUDGET_PENDING 제거(현행 미결 3건), evidence 최신
      세대 교체, 날짜 정정, 상속 필드 점검 목록 동봉.
- [x] rp7-final-report-v2.json — 기계용 판정 HOLD-REPLENISHMENT-P0-MD,
      BF7 매핑 분리. raw-result-v4 determinism 2회 일치.
- [x] resource-contract-v3 — 내용 불변, 승인 출처 정정
      (budget_attested_under_envelope=bca8a021…).
- [x] 기록 ledger-successor-hygiene.json, 문서 4곳 갱신, lessons.md
      점검 목록 규칙.

## Review
- **결과**: 사람용·기계용 기록이 모두 현행 일치. 자동화가 어느 최신
  파일을 읽어도 같은 결론. 같은 낡음은 교차 관문이 상시 감시.
- **게이트**: local-docs 전용 — 새 실험 0, Lean 0, 모델 0, 네트워크 0,
  커밋 0, focused 2회(상한 8).

## §HYG addendum (2026-07-31, 운영자 직접 지시 message 3247)
- [x] 3차 감사 반영 — "각 successor 점검 목록" 약속이 집계 2종에만
      이행됐음을 확인. 누락 3건(raw-result-v4·rp7-v2·contract-v3)의
      필드 점검표를 addendum 기록으로 보완. 상태·판정 불변, 파일 바이트
      무변경. lessons.md에 "봉투 약속 줄 단위 재독" 규칙 추가.

---

# §OW: 관측 창 RP.2~RP.4 (OBSERVATION-WINDOW-RP2-4-P0, 2026-07-31)

승인: 봉투 25acd622…(message 3257) → 이벤트 9491d87a…. 판정
**RP2-4-COMPLETE-PAUSED-COST-REVIEW**.

- [x] 무결성 — RP.1 동결물 72/72 재계산 + 오프라인 inventory 재생성
      byte-identical (이전 저장소 3곳 재해결 흐름 재현).
- [x] RP.2 — RED→GREEN 12/12, 전환 183 정규화(충돌 0·창세 제외 2·
      체크포인트 예상 일치). 창세 체인 버그 1건 테스트가 잡음.
- [x] 취득 — 스냅샷 199/199, 6.54GB(상한 20GB), 실패 0, 재개가능 fetcher.
- [x] RP.3 — RED→GREEN 11/11, gross 17,548, 격리 118,460, 게이트 clean.
      property 어휘 발명 시도가 동결 스키마 enum에 거절돼 판독으로 교체.
- [x] RP.4 — RED→GREEN 11/11, 7관문 3상태: eligible 0(NEEDS_SPEC·budget
      pending 반올림 없음), audit 풀 16,763, fail 785, 라이선스 결측 0.
- [x] 중간보고 2 — checkpoint-rp4.md(§7.2 의무 수치 + RP.5 120 층화
      표본 비용 추정) 발행 후 PAUSED-COST-REVIEW 정지. RP.5 미실행.
- [x] 기록 observation-window-rp2-4.json(fab8439a…), 문서 4곳 갱신.

## Review
- **결과**: 유입 축 측정의 기계 구간 완주. eligible 0은 명세 결박 전의
  정직한 상태 — RP.5(층화 120 표본 결박 시도)가 다음 관문.
- **게이트**: LLM 0, 유료 0, 상태 블록 무접촉, 커밋 0, 상한 전부 준수.
- **claim boundary**: gross≠공급. closed local 측정.

## §OW 후속 메모 (2026-07-31)
- 사슬 verify-final은 현재 coverage_gap(신규 작업 파일 미결박)으로
  fail-closed — R12 설계상 "새 세대(v7) 필요" 정상 신호. 결박은 사슬
  자체 승인 게이트가 있어 별도 진행(RP.5 봉투에 동반 또는 단독).
  상태 블록·기존 세대 검증은 무영향.

## §OW 후속 완료 — 사슬 v7 결박 (2026-07-31, proposal 4adbe961… 승인 3263)
- [x] 신규 파일 15개 결박(값 변경 0) → 사슬 12세대, verify-final
      **green 복원** (approved 203). 상태 블록 값 불변 재게시.

---

# §RP5: 기계-only 결박 측정 (RP5-EXECUTION-P0, 2026-08-01)

사전등록 v1→v2→v3 (운영자 감사 5구멍 수정, v2 표본 112건
INFEASIBLE 보존), 표본 120 동결(ef4aea52…), 실행 봉투 86e55ab9…(3282).
판정 **RP5-COMPLETE-120**.

- [x] 표본 — 2단 시드(dcf5bfc6…+72bc9e31…), 적응 상한 15/10/8/8,
      배치 4×30 층균형, v2 중복 3건 공개, 재현 결정성.
- [x] 하네스 — RED→GREEN 11/11: 탐색 순서 고정(아카이브 문서/벡터 →
      컴포넌트 내 고정 인용 → 표준기관), 모호=성공 불인정, 상한 거절,
      whitelist 밖 fetch 금지, 판단 주입 0.
- [x] 실행 — 배치 4개 전부(20.4초, HTTP 5회, 0.8MB). 분모 120 보존.
- [x] 결과 — BOUND 20 / AMBIGUOUS 95 / NOT_FOUND 5. 유일 허용 해석:
      기계-only 절차의 표본 결박률 16.7%, Wilson 95% [11.1, 24.3]%.
      층별 CI 병기. BOUND≠eligible≠공급.
- [x] 기록 — rp5-execution.json(eedb05df…), checkpoint-rp5-run.md,
      card-results 120행. 문서 4곳 갱신.

## Review
- **결과**: 유입 축 측정의 첫 실측 수치 확보. 다음 설계 지점 = AMBIGUOUS
  95(보조 측정, 모델 다양성 연동)·BOUND 20의 잔여 관문.
- **게이트**: LLM 0, 유료 0, 상한 대비 비용 미미, 상태 블록 무접촉,
  커밋 0. claim 경계 준수.

## §RP5 후속 — 4차 감사 정정 + 사슬 v8 (2026-08-01)
- [x] 결함 공개 successor — rp5-execution-addendum.json(8f49ba00…):
      배치 경계 정지권 생략·총 상한 기계적 미강제(실측 0.81MB 사후
      검증) 인정. 수치 판정 RP5-COMPLETE-120 유지(운영자 처분).
      lessons.md 재발 방지 2규칙.
- [x] 사슬 v8 — RP.5 산출물 10개 결박(값 변경 0, proposal be1d677c…
      승인 3286) → 13세대, verify-final green.

# §BOUND20-REVIEW: 잔여 관문 검토 (BOUND20-REVIEW-P0, 2026-08-01)

single-family fresh-context review. 봉투 1a8173ff…(승인 3292), 동결 설계
`bound20-residual-gates-v2-2026-08-01.md`. 판정 **BOUND20-REVIEW-COMPLETE-20**.

- [x] 하네스 — RED→GREEN 16/16: 검증자는 **형식만** 검사(양측 위치·인용
      없는 성공 주장은 DEFER 강등, 판정 편집·승격 0), 패키저는 digest 동결·
      절단 명명, tally는 CONTRACT-DRAFTABLE만 성공 계상.
- [x] 번들 — 20 카드 프로즌 bundle(digest 불일치 0, 절단 7건 상한 내),
      instructions_sha256 df658599…, manifest_sha256 3dcf2e54…
- [x] 검토 — 격리 fresh-context 세션 20개(카드별 고정 자료만), 재호출 0,
      서브에이전트 20/24, 전 카드 strict JSON 유효, 출력 그대로 보존.
- [x] 집계 — CONTRACT-DRAFTABLE 3(성공) / VECTOR-ONLY 1(불포함, 유한
      벡터 일치) / MAPPING-AMBIGUOUS 9 / SPEC-WEAK 7 / DEFER 0, 형식 강등
      0, 분모 20 보존. CD = 67f371ad(miden-vm)·83ea1ed6(garaga)·
      92b51438(kimchi). 층: circuit CD1·pf-sys CD1·zkvm CD1, tooling CD0.
- [x] 기록 — bound20-review.md·bound20-review.json·bound20-review/. 문서
      4곳 갱신.

## Review
- **결과**: RP.5 BOUND 20의 결박 품질을 판정 — 명세계약 초안 가능 후보
  3건 확보(다음 단계 = Lean 문면화·strict task 발급, 별도 승인). 후보 ≠
  공급·reward-ready. AMBIGUOUS 95·모집단 외삽 금지.
- **정직 고지**: a16635936e(android.rs) 도구 18회·143.4s(원문 16개·번들
  181KB, 판정 비성공 SPEC-WEAK라 위험 없음) / ed7a89d46c(web.rs)
  rationale 한국어(형식 유효, 보존).
- **게이트**: 같은 Claude 계열 = 독립 감사 아님(same-family caveat).
  LLM 유료 0, 네트워크 0, 상태 블록 무접촉, 커밋 0. claim 경계 준수.

# §BOUND20-CONTRACT: 명세계약 초안 → Lean 게이트 (2026-08-01)

single-family. BOUND-20 검토가 뽑은 CONTRACT-DRAFTABLE 3건(miden-vm·garaga·
kimchi)을 2조각으로 진행. 결과 문서는 local-docs(gitignored), 커밋 0.

## 조각 1 — 명세계약 초안 (BOUND20-CONTRACT-DRAFT-S1, 판정 S1-DRAFT-COMPLETE)
봉투 `1d39e792…`, 승인 이벤트 `90ff0a23…`.

- [x] 초안 — 카드별 fresh-context 서브에이전트가 사양만 보고 명세계약
      초안(5필드 + Lean 명제 스케치) 작성(기대 판정 미상속).
- [x] 형식 재검사 — 필수 5필드·금지 키토큰·Lean 명제 형식(증명 본문
      없음·∀ 일반성)만. 컴파일 안 함. DRAFTED 3 / DRAFT-BLOCKED 0 / 분모 3.
- [x] 초안 지문 — miden `002e842e…`·garaga `04fd0136…`·kimchi `1d07d361…`.
      참조 벡터 0, 결속 다이제스트 PENDING.
- [x] 오탐 1건 자체 교정 — 검사기가 주석 속 영단어 "by"를 tactic으로 오인.
      주석 blank 후 재스캔, 초안 내용·판정 무편집(운영자 교정 아님 → lessons 제외).
- [x] 기록 — bound20-contract-drafts.json/.md, bound20-contracts/<card>.draft.json.

## 조각 2 — Lean 명제 게이트 (BOUND20-CONTRACT-COMPILE-S2, 판정 S2-GATE-COMPLETE)
봉투 `f81b12e7…`(승인 3306), 승인 이벤트 `d1c107b2…`.

- [x] 게이트 — 조각 1 명제 3건을 바이트 그대로(수정 0) 진짜 검증기에 투입.
      2프로세스: boole_check elaborate + Audit.lean 공리 폐포 감사(예산
      400000/512, 핀 v4.29.1, checker `1dd3055a…`). 금지토큰 사전스캔만 Python 미러링.
- [x] 결과 — ELABORATED 3 / COMPILE-BLOCKED 0 / 분모 3, proved:false 고정.
      공리 miden `[]`·garaga `[Quot.sound, propext]`·kimchi `[]` 전부 허용목록
      {propext, Classical.choice, Quot.sound} ⊆.
- [x] statement_digest 동결 3건(초안 지문과 바이트 동일·게이트후 발행 의미).
- [x] 예고 위험 검증 — miden docstring `debug.mem`이 금지접두사 `debug.`에
      걸릴 뻔했으나 주석 blank로 통과.
- [x] 기록 — bound20-contract-compile.json/.md, bound20-contracts/<card>.compiled.json.

## Review
- **결과**: 초안 3건이 실제 Lean 검증기에서 elaborate + 공리 허용목록 통과.
  단 "형식 맞고 공리 깨끗"까지 — 증명·발급 아님. ELABORATED ≠ proven ≠
  strict ≠ 공급.
- **결속 다이제스트 제외**: source(고정 discovery row 부재)·target(릴리스
  아카이브 sha256 + 레지스트리 대조로 네트워크 필요)라 조각 2에서 뺌 →
  별도 슬라이스. PENDING-SLICE-LATER 유지.
- **한계**: 명제가 명세를 충실히 담았는지는 여전히 사람 판정(same-family).
  금지토큰 사전스캔은 검증기 규칙 재현(elaborate·공리감사는 실제 Lean).
- **게이트**: 커밋 0·네트워크 0·유료 0·consensus 코드 무수정·상태 블록 무접촉.
  상태 불변(STRICT_READY=1/REWARD_READY=0/RP0-MD=HOLD/BF.7=HOLD/Base=false).
  public/API benchmark claim 아님. closed-local.

## 조각 — garaga v2 명제 재작성 (BOUND20 garaga, 실행: 봉투 승인 3382)
2-artifact 승인: DESIGN `2516539e…`(승인 3380) → 동결 봉투 `5292352c…`(승인 3382).
승인 이벤트 `76fcd328…`, 제안서 `d3a662da…`. 전 gitignored, 커밋/네트워크 0.

- [x] v1 공허성 제거 — v1은 `expand_message_xmd_spec`이 자기 정의=자기 정의(rfl 항진)라
      반증 불가였고 오버사이즈 DST에 `none`(실코드 반대)을 반환. v2는 implModel(실제 Rust
      expand_message_xmd+construct_dst_prime 반영) ↔ specModel(RFC 9380 §5.3.1+§5.3.3에서
      독립 구성, 오버사이즈 DST 해시 분기 포함)의 일치를 묻는 형태로 재작성.
- [x] 추상 해시 H(옵션 A) — 일치는 임의 H에 대한 바이트 조립 알고리즘. 증거는 비-SHA256
      토이 해시로 #eval/decide 실행(SHA-256·RFC 벡터 주장 아님; Appendix K 미보유).
- [x] Lean 4종 증거 — GaragaExpandXmdV2.lean elaborate exit0 + `#eval sampleAgree`=true(6케이스,
      오버사이즈 DST 포함); GaragaXmdNonVacuity.lean elaborate exit0 + 두 반증정리
      badDrop_is_false/noOver_is_false 공리무의존(#print axioms, native_decide 미사용).
      XOR을 UInt8.xor 대신 구조적 비트연산으로 구현해 propext/Quot.sound 회피.
- [x] v2 계약 초안 방출 — `83ea1ed6….v2.draft.json`(record_type bound20_contract_draft_v2,
      schema 2.0), statement_digest=`93f82242…`(=lean 명제 sha), record_sha256=`3f3171c5…`,
      index=`cb6863a7…`. trust_root after_tree=`990c2b79…`(Rust 실내용 sha 일치).
- [x] RED→GREEN 음성검사 6종 — (a)Rust 변조 (b)명제 비elaborate (c)reward/answer 키주입
      (d)재공허화 (e)오버사이즈 DST 제거 (f)trust-root 불일치. 가드 무력화 시 6/6 통과(RED),
      실제 가드 시 6/6 거절(GREEN).

## Review — garaga v2
- **결과**: v1의 정의적 공허성 제거(반증 가능해짐). 단 명제 재작성일 뿐 — 증명·판정·후속
      id 아님. proved:false, changes_status:false, 상태 HOLD 불변.
- **한계**: 충실성 간극이 implModel↔Rust / specModel↔RFC 사람검토로 이동. 추상 H 조립
      일치를 토이 해시로 확인한 것이며 SHA-256/RFC 벡터 검증 아님. 독립검증 아님·보상 아님.
- **게이트**: 전 gitignored, 동결 v1 무편집, tracked tree(tasks/*.md 외) 무변화, HEAD 03ef1cc
      불변, 커밋/PR/네트워크/유료 0, consensus·상태 블록 무접촉. public/API benchmark claim 아님.

## 조각 — EVM 어댑터 대표 1건 (gate-fitness, 운영자 승인 msg 3459)
목표: 새 EVM 엔진 구현 금지 — 기존 검증 엔진(EELS)을 버전+lockfile로 고정해 연결.
공통 어댑터 인터페이스로 공식 EVM 대표 1건을 accept 시키고, 입력/기대출력/최종상태 변조가
각각 정확한 이유로 거절되게 한다. 성공 판정은 **EVM-ADAPTER-REPRESENTATIVE-PASS만** —
"7,125 검증/채굴가능/공급확정"으로 부풀리지 않는다. Solidity/Rust/Base/보상/공개벤치 범위 밖.

기준선(사전):
- ZK 대표 실행(항목1) — Lean 판정기가 오늘 이 머신에서 실제 실행 확인: 유효증명 Accepted
  (`lake exec boole_check` exit0 3.04s + 공리감사 `BOOLE_AXIOM_AUDIT_DONE` 3.62s),
  거짓증명 DeterministicReject(`decide proved 1+1=3 is false` exit1 0.43s). closed-local.
- EVM 엔진 feasibility — EELS 체크아웃 `2282c757…`(tests@v20.0.1-14), `ethereum-spec-evm t8n`,
  Cancun fork 존재, .venv 준비됨(sync 불요). 골든 실행: 대표 vector(statetest179)를 t8n으로
  돌려 공식 선언 post 정확 재현 — target.storage[0]=0x515480126a50a173506e066762129255,
  sender.nonce=0x1, rejected=[], stateRoot=0xf1bf91e7…. (closed-local, 공개 아님)

- [x] 새 경량 크레이트 `boole-evm-adapter` — boole-node `UsefulProductAdapter`/`PacketAuditOutcome`
      /`DeterministicBudget` seam을 미러링(corresponding). boole-node/Lean 경로 무접촉, 의존 최소
      (hex/serde/serde_json/sha2/thiserror만). publish=false, workspace lints 상속.
- [x] `EvmStateTestAdapter::audit` — packet(alloc/env/txs + input_digest + expected{post_alloc,
      state_root} + 엔진핀 Cancun/chainid1/commit) 읽고 핀 엔진 t8n 실행 → 3게이트:
      (a) 원시 파일 바이트 digest ≠ 핀 → InputDigestMismatch (입력 변조, 엔진 실행 전 거절)
      (b) 엔진 post_alloc ≠ expected → ExpectedPostAllocMismatch (기대출력 변조)
      (c) 엔진 stateRoot ≠ expected → ExpectedStateRootMismatch (최종상태 변조)
      엔진 부재/오류 → RetryableUnavailable/EngineError(환경실패 구분, 자동수정 루프 금지).
- [x] focused test `tests/evm_statetest_representative.rs` — 엔진은 gitignore라 env
      `BOOLE_EVM_ENGINE` 없으면 skip(CI 컴파일만). RED(스텁, `audit not implemented`로
      Accepted 실패)→GREEN: 유효 accept(stateRoot 0xf1bf91e7…898609) + 3변조 각각 거절 1 passed.
- [ ] fmt+clippy 2종 → NotoriAndo 커밋 → branch push → PR → CI self-test/supply-chain green → merge.

## Review — EVM 어댑터 대표
- **결과 (판정 = EVM-ADAPTER-REPRESENTATIVE-PASS, 그 이상 아님)**: 공식 EVM 대표 1건
      (statetest179, EELS `2282c757…` 엔진 `ethereum-spec-evm t8n`, Cancun/chainId1 핀)을
      새 엔진 구현 없이 기존 검증 엔진에 연결. 공통 어댑터 seam(boole-node 미러)으로 유효
      fixture accept + 3변조(입력/기대출력/최종상태) 각각 **다른 이유**로 거절 확인.
      focused test 1 passed (0.76s 실행, 컴파일 9m51s는 이 머신 IO 경합).
- **판정 분리**: driver/proof-intake/verify 의미 분리와 동일하게 — 이건 대표 1건 seam-fitness
      실행일 뿐. supply/mineable/7125-verified 아님. `mineable_now=0` 불변, consensus/runtime
      경로 무접촉, Lean 경로 판정 무변화.
- **엔진 핀**: EELS commit 2282c757b3699d506de112b8a48b6b538df7ed1f (tests@v20.0.1-14),
      python `ethereum` 2.19.0, .venv 고정. 엔진은 gitignored 로컬 체크아웃 → CI에서는
      `BOOLE_EVM_ENGINE` 미설정으로 test skip(컴파일만), 커밋된 fixture는 엔진-독립.
- **비밀키 미커밋(엔진 사전서명)**: txs fixture에 sender `secretKey`를 넣지 않는다 —
      gitleaks generic-api-key로 잡히는 것 회피 + 저장소에 개인키 0. 대신 핀 엔진으로 tx를
      한 번 서명(`--output.body`)해 엔진이 만든 (v,r,s)를 그대로 넣었다(손서명 위험 0).
      재실행 결과 stateRoot 불변(0xf1bf91e7…898609)으로 서명 정합 확인. gitleaks staged 스캔
      clean. `.gitleaks.toml`(보안설정) 무편집.
- **한계/승격 항목(범위 밖)**: 실행 바이너리를 핀 commit과 대조하는 provenance 검증,
      boole-node 실 seam 배선, Solidity/Rust/Base/보상/공개벤치는 모두 이번 범위 밖.
- **게이트**: fmt 통과, clippy 확인, `git diff --check` clean, pycache 없음,
      NotoriAndo author 커밋 → branch → PR → CI self-test/supply-chain green → merge 예정.
      public/API benchmark claim 아님. closed-local, not public-network.

## Review — EVM state-test adapter census (7,125 anchors, Option 1)
- **Scope (operator msg 3470/3473 frozen)**: measure ONLY "number of EVM
      cases verifiable by the current state-test adapter". No inflation to
      mineable / supply / independent verification. Pinned checkout
      `execution-specs` git `2282c757` (EELS `ethereum-execution` 2.19.0),
      reference t8n runs in-process (fill WITHOUT `--evm-bin`), dedicated
      venv from the checkout's own uv.lock, network OFF (blackhole proxies).
- **Ground truth (non-circular)**: pass gate = author-declared Account
      postcondition only (`verify_post_alloc` -> `check_alloc`, only
      `model_fields_set` ∩ {nonce,balance,code,storage}; None = must-not-exist).
      Engine `stateRoot` recorded but NEVER the pass gate. Vacuous author-post
      -> ORACLE-UNUSABLE, not a pass.
- **Result — CASE level (fill-executable set, 100% COMPLETE)**:
      collect-only catalog 62,025 state_test cases; fill deterministically
      deselects 1,979 collect-only phantoms (1,891 = non-deployed
      Constantinople fork, executed 0 across all 102 shards; 88 = fork/param
      combos excluded by fill markers — all reproduced as deselections in
      isolation, genuine not-run gap = 0). fill-executable set = 60,046, all
      executed to a verdict. Buckets: EXECUTABLE-PASS **52,875**,
      ORACLE-UNUSABLE 7,091, ENGINE-SKIP 80, EXECUTION-MISMATCH/
      INCOMPLETE-FIXTURE/UNSUPPORTED-FORK/TIMEOUT/HARNESS-ERROR all 0.
- **Headline (only claim)**: verifiable EVM case count = **52,875**
      (EXECUTABLE-PASS only). Run finished all 102 shards rc=0 in 666s,
      no kill, no wall-cap.
- **Result — ANCHOR level (7,125)**: src/ definitions 2,825 ->
      STATE-TEST-ADAPTER-NOT-APPLICABLE (candidates for other EVM problem
      types, not failures). tests/ 4,300 test functions ->
      HAS-STATE-TEST 2,681 / FORMAT-UNSUPPORTED 271 / NOT-COLLECTED 1,348
      (amsterdam/osaka future-fork EIPs + loader/genesis helpers). Of
      HAS-STATE-TEST: ALL-CASES-PASS 2,608, PARTIAL-CASES-PASS 5,
      NO-CASE-PASS 68. The 68 NO-CASE-PASS are NOT adapter failures: 64 are
      ORACLE-UNUSABLE functions (author post asserts tx-rejection, no Account
      state to check) + 4 ENGINE-SKIP; 0 mismatch, 0 harness-error.
- **Discipline**: RED->GREEN 13 classifier unit tests (all pass), small
      deterministic sample (homestead) confirmed executability, then bounded
      full run under the same code/approval — no per-problem fixes/retries.
      Raw facts stored per case; buckets DERIVED at aggregation (refinable
      without re-run; that is how the 80 ENGINE-SKIP were reclassified from
      HARNESS-ERROR without re-running). Artifacts in gitignored local-docs
      (`evm-execution-census-p0/`): CENSUS-SPEC.md, harness/, out/records/,
      out/census_summary.json.
- **Boundary**: closed-local measurement. NOT public/API benchmark, NOT
      mineable/supply/leaderboard, NOT independent verification. `mineable_now`
      unchanged; consensus/runtime/Lean paths untouched.

## 2026-08-05 — EVM verifier-only 대표 1건 수직 시제품 (ADR-0018, 운영자 msg 3513/3516/3518)
- **한 일**: `boole-evm-adapter`(합의 경로 밖, `publish=false`)에 sp1-verifier
      `=6.3.1`(`default-features=false`) 검증 전용 경로를 추가. 새 모듈
      `zk_verify`: `CompressedProofVerifier`가 미리 적재한 고정 vkey 해시로
      compressed STARK를 **검증만** 호출(`SP1CompressedVerifierRaw::verify_with_public_values`),
      prover/proving-key/증명생성 API 없음. 역직렬화(bincode) **이전에** proof 크기
      상한(`MAX_COMPRESSED_PROOF_BYTES` = 4 MiB) 적용.
- **공급망 preflight (msg 3516)**: 추가 crate 121개(전부 crates.io, git 0, yanked 0),
      라이선스/bans/sources 통과. cargo deny는 `unmaintained` 2건에서만 FAILED —
      RUSTSEC-2021-0139(ansi_term), RUSTSEC-2025-0141(bincode). cargo audit는 동일 2건을
      허용 경고로 통과(exit 0). 그래서 멈추고 보고 → 운영자 msg 3518이 이 2건만
      deny.toml 예외 승인(사유·의존 경로·재검토 2026-11-05 주석). 다른 advisory/취약점
      /yanked/git 없음 재확인 후 진행.
- **ADR-0018 1회 정정 (msg 3518)**: 결정 (f)에 (i) production은 검증 API만 호출·prover
      /proving-key/증명생성 없음(테스트로 고정), (ii) sp1-verifier 6.3.1 상류 패키징 탓
      `slop-basefold-prover`가 **간접 빌드 의존성**으로 포함되므로 "의존성 그래프 전체가
      verifier-only"라 주장하지 않음 — 명시. (ADR는 git-ignored 샌드박스, PR에 미포함.)
- **TDD**: RED(모듈 부재 컴파일 실패) → GREEN. `tests/zk_verify_bounds.rs` 5건 —
      초과→`ProofTooLarge`(역직렬화 전), at-cap/garbage/empty→`ProofRejected`, 직접
      의존성 verifier-only 고정. 전부 synthetic·CI 실행형(프로즌 샌드박스 불요).
- **게이트**: focused 5/5 PASS, 기존 `evm_statetest_representative` 회귀 없음,
      `cargo fmt --check` CLEAN, `cargo clippy -p boole-evm-adapter --all-targets -D warnings`
      0 경고(로컬에서 `manual_pattern_char_comparison` 1건 잡아 수정 → CI 반송 회피),
      `cargo deny` exit 0, `cargo audit` exit 0.
- **CI 빌드비용 실측**: sp1-verifier 트리(slop-*/p3-*/sp1-hypercube) 최초 풀 빌드
      10분+, clippy check-build ~1분(캐시 후). 워크스페이스 전체 빌드에 이 트리가 더해짐.
- **경계**: 합의 경로 밖·기본 OFF·기존 단건 증명 재사용(새 증명 0)·prover 의존성 0.
      `mineable_now`=0 유지, reward/Base 변경 없음, 합의 연결/활성화 없음. closed-local,
      public/API/leaderboard 주장 아님.

## 2026-08-06 — EVM 실제 동결 증명 ACCEPT + 신원 고정 (ADR-0018, 운영자 msg 3522/3526, A 승인)
계획 (한 슬라이스): 거절 안전장치(PR #109)에서 → "실제 증명을 받아들이는 검증기"로.

- [x] **1. 샌드박스 1회 변환** (git-ignored `local-docs/evm-zkvm-feasibility/`, SDK 有):
      `proof-single.bin`(SDK wrapper `SP1ProofWithPublicValues`)을 verify-only
      `sp1-verifier`가 소비 가능한 3종 바이트로 변환 — (a)`proof-compressed.bin`
      = `bincode(SP1Proof)` (b)`vk-hash.bin` = `bincode([SP1Field;8])`(KoalaBear
      vk 해시) (c)`public-values.bin`(782B). 변환 도구(`project/transcode`)가
      **정확히 production 크레이트** `SP1CompressedVerifierRaw::verify_with_public_values`
      로 round-trip(Ok)을 증명한 뒤에만 파일 기록. SDK는 Boole 워크스페이스에
      추가하지 않음(샌드박스 전용). 원본 wrapper digest·SP1 SDK 6.3.1·circuit v6.1.0·
      변환 전후 digest 기록.
- [x] **2. CI fixture 커밋**: 3종을 `fixtures/evm-zkvm/`에 + `SHA256SUMS` + `PROVENANCE.md`.
      proof digest는 **fixture 무결성 확인 전용** — "이 증명만 수락"하는 production
      조건 아님(다른 유효 증명 원천거절 안 함). gitleaks 예외는 실제 탐지 시에만 좁게.
- [x] **3. 검증기 API 변경**: `CompressedProofVerifier::frozen()` — 고정 vk를 상수로
      내장(`include_bytes!` vk-hash.bin), 호출자가 임의 vk 주입 불가(vk 받는 생성자는
      `#[cfg(test)] pub(crate)`로만). 파이프라인: size gate → **입장검사(admission,
      비암호)** → **암호 검증(crypto)**. 암호결박 범위 = public values 바이트까지로만
      표기. task_contract(pv off 0)·fork(64)·author_oracle(128) 3종을 프로즌 digest
      상수와 비교하는 admission은 "외부 입장검사"로 분리 표기(암호결박이라 부르지 않음).
- [x] **4. RED→GREEN 매트릭스**: ACCEPT(실제 증명+실제 pv) / REJECT 7종 —
      wrong proof·wrong vk·wrong public-values(비-admission 필드 변조→crypto)·
      wrong task_contract·wrong fork·wrong author_oracle(→admission)·oversized.
      가능하면 ACCEPT도 CI가 검사(fixture 동봉, env-gate 스킵 아님).
- [x] **5. 게이트→머지**: focused test + fmt + clippy 2종 → 최신 main 새 브랜치 →
      PR → CI green → **rebase merge**(author noreply 보존). consensus·`mineable_now`·
      reward·Base 무변경. `mineable_now`=0 유지, closed-local, public/API/leaderboard 주장 아님.

### Review (2026-08-06)

**한 일** — 거절-전용(PR #109)에서 "실제 동결 증명을 받아들이는 검증기"로 승격.
- fixture 3종 + `SHA256SUMS` + `PROVENANCE.md`를 `fixtures/evm-zkvm/`에 추가
  (proof-compressed 1,272,546 B / vk-hash 32 B / public-values 782 B; `shasum -c` OK).
- `zk_verify.rs`: `CompressedProofVerifier::frozen()`가 vk-hash.bin을 `include_bytes!`로
  내장, vk-주입 생성자는 `#[cfg(test)] pub(crate)`. 3-gate = size → admission(비암호,
  task_contract/fork/author_oracle 프로즌 상수 대조) → crypto(`SP1CompressedVerifierRaw`).
- `tests/zk_verify_accept.rs`(신규): ACCEPT 1 + REJECT 6(tampered proof·비-admission pv
  변조→crypto·task_contract·fork·author_oracle·short pv). `zk_verify_bounds.rs`: oversized +
  parse-gate + "sp1-verifier verify-only(Cargo.toml)" 계약 유지. REJECT 7종 요건 충족.

**로컬 검증(정직 기재)** — 무거운 codegen 테스트는 **환경 차단**.
- `cargo fmt -p boole-evm-adapter --check` PASS, `git diff --check` clean.
- `cargo check -p boole-evm-adapter --tests` **EXIT 0** — lib+두 테스트파일 타입/보로우/
  `include_bytes!` 경로/`sp1-verifier` 시그니처 정합 확인(codegen 없어 wedge 회피).
- **런타임 ACCEPT/REJECT 로컬 실행 불가**: macOS `syspolicyd` 러너웨이(~73–90% CPU)로
  sp1-verifier STARK codegen이 반복 wedge(rustc ~18s 후 0% CPU 정지, 4회+). 로컬 heavy
  게이트 포기 → **CI(clean ubuntu-latest)가 구속 게이트**(CLAUDE.md: full 검증은 CI).
- ACCEPT crypto 경로는 **독립 입증됨**: 변환 도구가 production 함수
  `SP1CompressedVerifierRaw::verify_with_public_values`로 round-trip Ok(0.924s).

**머지 완료(item 5)** — 로컬 커밋 `af8a4d5` → `feat/evm-zkvm-accept-slice` push → PR #111 →
**CI self-test + supply-chain + corpus 4종 green** → **rebase 자동 머지** → main 커밋 `cb6a817`.
검증: main author = `NotoriAndo <…@users.noreply.github.com>`(noreply 보존, PR #109 squash 교훈 반영),
local main == origin/main == `cb6a817`, working tree clean, feature 브랜치 삭제(로컬+원격).
gitleaks git-mode 재검사 → leak 없음 → `.gitleaks.toml` 예외 미추가. consensus·`mineable_now`·
reward·Base 무변경. mineable_now=0. public/API/leaderboard 주장 아님. CI run:
`actions/runs/31060765040`.

### 종결 (2026-08-06, 운영자 msg 3530 승인)

이 in-repo verifier-only ACCEPT 실험을 **CLOSED**로 종결한다. 성공 기록은 ADR-0018
(`local-docs/adr/0018-…`, git-ignored 설계문서)의 "Follow-on record (2026-08-06)" 섹션에
남겼다 — 검증경로를 샌드박스→추적 크레이트 `boole-evm-adapter`로 이관, ceiling
`EVM-ZKVM-FEASIBILITY-PASS` 도달, 여전히 Stage-1(consensus 밖·default-OFF·mineable_now=0),
ADR 게이트 1–2를 부분 진전(node-path 게이트 3–7은 미접촉). **다음 단계 — default-OFF
검증기의 런타임/노드 배선(ADR 결정(e) Stage 2 + 게이트 1 "node path"·3–7)은 별도 운영자
승인 필요(msg 3530)**. 그 설계·구현은 이번 종결 범위 밖이며 승인 전 진행 금지.

---

# EVM 검증기 node-side bridge qualification — ADR-0019 Stage 1 (2026-08-06, 운영자 msg 3538→3540 승인)

한 줄: 동결 EVM 압축-STARK 검증기를 노드의 블록-판정 결과타입(`BlockReverifyOutcome`)에
맞춰 **자격검증(qualify)**만 했다 — 실제 블록 판정 경로·블록 EVM 필드는 **전혀 손대지
않았다**. 라벨 `EVM-NODE-BRIDGE-QUALIFIED, live node-path gate pending`. "node path
integrated" 아님, "ADR-0018 게이트 1 완료" 아님.

## 승인 경위
- msg 3538: ADR-0019 v2 중 C1–C4·D2·D4 Stage 1·Stage 1 구현계획만 승인. production
  활성화 통로·genesis 변경·block.v4·룰버전·consensus/reward/Base/mineable_now 변경 불승인.
  ADR 상태 "Accepted — Stage 1 only; Stage 2 Proposed".
- msg 3540(좁힌 Stage 1 확정): Stage 1은 live funnel 연결이 **아니라** node-side verifier
  bridge qualification이라고 ADR에 정정 기록. 실제 블록 판정 경로·블록 EVM 필드 무수정.
  고정 vk 게이트 함수 + 결정적 합격/거절 변환기를 한 PR RED→GREEN. 증명 바이트 부재에
  따른 `RetryableUnavailable`은 Stage 2 전송 경로 책임(Stage 1엔 그 경로 없음).

## 한 일 (한 PR, RED→GREEN)
- 신규 leaf 모듈 `crates/boole-node/src/evm_bridge.rs` (in-module `#[cfg(test)]` 테스트만).
  - `evm_verify_outcome_to_reverify(VerifyOutcome) -> BlockReverifyOutcome`: 결정적
    합격/거절 변환기. `Accepted→Verified`, 각 `Rejected(_)→DeterministicReject{게이트명 detail}`.
    **`RetryableUnavailable` arm 없음** — pure in-process verify라 가용성 실패 불가(Stage 2).
  - `qualify_evm_proof(&CompressedProofVerifier, proof_bytes, public_values) -> …`:
    고정 vk(`CompressedProofVerifier::frozen()`) 게이트 함수.
- `boole-node`에 `boole-evm-adapter` 정규 의존성 추가(Cargo.toml/Cargo.lock 1줄). deny.toml/
  `.cargo/audit.toml`은 이미 워크스페이스 수준으로 sp1 트리 advisory allowlist 중이라 무변경.
- `lib.rs`: `mod evm_bridge;` + 두 함수 re-export.
- **기존 판정 경로·블록 struct 무수정** → C1(판정 불변)은 구성상 성립(건드린 경로 없음).

## 검증 (로컬 focused gate)
- RED: `accepted_maps_to_verified`가 stub에서 실패(예상) 확인. (sp1 트리 최초 로컬 빌드 54분 —
  1회성 캐시 비용, wedge 아님.)
- GREEN: `cargo test -p boole-node --lib evm_bridge` → **11/11 PASS**(0.90s). 매핑 5 +
  실제 fixture ACCEPT 1 + 거절 매트릭스 5(oversized·short pv·admission-header 변조·
  pv 본문 변조→crypto·proof 바이트 변조→crypto). 실제 동결 증명이 `Verified`로 자격통과.
- `cargo fmt -p boole-node --check` PASS, `git diff --check` clean, `scripts/__pycache__/` 없음.
- clippy 2종(`--workspace --all-targets` / `+features`) 게이트 통과 확인.
- **머지 경로**: `feat/evm-node-bridge-qualify` → PR → CI `self-test`+`supply-chain` green →
  **rebase merge**(author `NotoriAndo <…@users.noreply.github.com>` 보존). 머지 후 origin/main
  으로 검증. (커밋/PR/CI run 링크는 최종 보고에 기재.)

## 경계
- consensus·`mineable_now`(0 유지)·reward·Base 무변경. EVM 검증기 default-OFF 유지.
- Stage 2로 이월(별도 승인 필요): funnel 호출부(`reverify_block_selected_shares`), 블록
  EVM-evidence 필드, `RetryableUnavailable` 전송 경로, genesis 핀·block.v4·룰버전 3→4.
- public/API/mining/leaderboard 주장 아님. closed-local qualification.

---

# 결정 기록: EVM 전용 Stage 2 보류 → BF.7 공통 multi-adapter receipt 경로 편입 (2026-08-06, 운영자 msg 3544)

한 줄: EVM 전용 Stage 2·`block.v4`·genesis pin 계획을 **보류**하고, 도메인별 합의 경로를
새로 만들지 않는다 — BF.7 공통 multi-adapter receipt 경로에 편입해 **합의 변경은 한 번만**
수행한다. 코드·genesis·룰버전·`mineable_now` 무변경(문서 결정만).

## 결정
- **EVM 전용 Stage 2 보류**: ADR-0019의 EVM 전용 Stage 2·`block.v4` 판정-바인딩·genesis
  `EvmVerifierPin`(D3)을 독립 합의 경로로 구현하지 않는다. ADR의 해당 설계문은 역사적
  맥락으로만 보존하고 이 결정이 상위(supersede).
- **이유 — 합의 변경 1회 원칙**: EVM 전용 합의 경로를 만들면 기존 BF.7 공통 receipt 경로와
  **두 경로**가 병존한다. 대신 순서: (a) 전 도메인 통합 채굴자격(`mineable`) census →
  (b) 공통 task/receipt 형식 확정 → (c) EVM Stage 2를 BF.7 공통 경로에 편입 → (d) 도메인
  adapter만 교체 가능 → (e) 합의 변경은 그때 **한 번만**. 도메인별 합의 경로 신설 금지.
- **현재 고정 vk·public-values 헤더 = feasibility 대표 fixture일 뿐**: Stage 1이 쓰는
  `FROZEN_VKEY_HASH`와 192B public-values/admission-header 배치는 `EVM-ZKVM-FEASIBILITY-PASS`
  대표 fixture이며 **합의 형식도 genesis pin도 아니다**. BF.7 공통 형식이 확정되면 배치가
  바뀔 수 있으므로 그 전까지 genesis 못박기·receipt 형식 동결 금지(잠정 유지).
- **Stage 1(머지본)은 무영향**: `crates/boole-node/src/evm_bridge.rs`(PR #113 / `0a11afab`)는
  default-OFF·미배선·adapter 모양 자격검증기라 되돌릴 것 없음 — BF.7 통합이 나중에 소비할
  도메인 adapter 모양 그대로 유지.

## 경계
- 코드·genesis·`consensus_rule_version`·`mineable_now`(0 유지)·reward·Base **무변경**. 문서
  기록(ADR-0019·todo·lessons)만 갱신하는 docs-only 결정.
- ADR-0019 로컬 문서(git-ignored)에 동일 결정 기록(Status + "Stage 2 HELD — folded into
  BF.7" subsection). public/API/mining/leaderboard 주장 아님.

---

# boole-emitter 공통 계약 검사기 + Rust 대표 anchor zero 경로 (2026-08-07, 운영자 msg 3570·3573)

한 줄: 신규 leaf 크레이트 `boole-emitter`를 **공통 계약 검사기 + zero 결과 생성기**로 도입 —
canonical-JSON·digest·XOR 불변식을 RED→GREEN으로 고정하고, Rust 대표 anchor(attributes.meta)
1건을 `TASK-CONTRACT-UNMATERIALIZED` zero 결과(Stage A `TASK-CONTRACT-MISSING`)로 결정적 변환.
consensus·census 집계·문제 수 발표·mineable_now 무변경.

## 한 일 (RED→GREEN)
- **신규 크레이트** `crates/boole-emitter`(workspace member 추가, `[lints] workspace = true`).
  boole-core만 의존(파생 identity는 BLAKE3 `h_protocol` 재사용, 증거는 SHA-256).
- `canonical_json.rs` — **신규 규격** `boole.canonical-json.v1`: 키 정렬(코드포인트),
  compact separators, `allow_nan=false`, duplicate-key 거절, 정수 전용, ensure_ascii(비BMP는
  UTF-16 surrogate pair). 고정 test vector 11개(3개 ERROR 케이스 포함).
- `digest.rs` — 태그드 `Digest{algorithm, hex}`, `raw()`, `push_field`(u64_le(len)++bytes),
  `protocol_digest`(BLAKE3), `sha256_digest`(증거).
- `identity.rs` — `rust_anchor_binding_digest`(§3 결정적 preimage: domain_tag=rust,
  source_commit, source_path, line_start(u64_le), kind, raw(semantic_digest), identity).
- `result.rs` — `EmitterResultV1` envelope + XOR 불변식(`validate_xor`), `ZeroReason`(+`missing`),
  `ZeroReasonCode`(msg 3573: `TASK-CONTRACT-UNMATERIALIZED`), `StageA`(`TASK-CONTRACT-MISSING` 추가).
- `rust_domain.rs` — `convert_rust_anchor`: Rust Reference anchor를 zero 결과로 변환
  (tasks=[], code=TASK-CONTRACT-UNMATERIALIZED, missing=[statement,oracle,work_product_contract],
  근거 digest=SHA-256(canonical-JSON 증거), Stage A=TASK-CONTRACT-MISSING; 승격 금지).
- 프로즌 계약서(local-docs) §10/§1에 msg 3573 정정 반영: 코드명·`missing` 필드·Stage A 상태.

## 검증 (로컬 focused gate)
- **RED**: canonical_json 11개 전부 `not yet implemented`(todo! panic)로 실패 확인.
- **GREEN**: `cargo test -p boole-emitter` → **21/21 PASS**(canonical_json 11 + contract 10, 0.07s).
  contract = digest 결정성/순서민감성 4 + XOR 4 + Rust 대표 zero 경로 2.
- `cargo fmt -p boole-emitter --check` / `cargo clippy -p boole-emitter --all-targets -D warnings`
  게이트(신규 leaf 크레이트 스코프 — sp1 트리 무접촉). full workspace clippy 2종은 CI가 구속.
- `git diff --check` clean, `scripts/__pycache__/` 없음.

## 경계
- consensus·admission·replay·reward·Base·`mineable_now`(0 유지) **무변경**. 크레이트는
  NON-consensus 오프라인 census 유틸(온체인 산출물 없음).
- Rust 3,293 전수 실행·census 집계·문제 수 발표 **안 함**. 이 zero 결과는 "Rust 문제 0개"가
  아니라 "현재 Rust Reference anchor에는 runnable task 계약이 아직 없음".
- public/API/mining/leaderboard 주장 아님. closed-local 계약 검증만.

## 다음 (별도 제출)
- 대표 검증 완료 후: **Rust runnable task family 설계안**(anchor ↔ 실제 rustc 구현·공식
  입력/기대결과·제출 산출물·checker·난이도 축 짝짓기). 문제별 수동 수정 금지. 문서 먼저.

---

# Input-Recovery Successor Slice (operator msg 3627 — v1.2 지속-worker 보류)

상태 정정 기록: `local-docs/evm-execution-census-p0/INPUT-RECOVERY-STATE-v1.md`
(`RECORDED-7306` / `INPUT-ARTIFACTS-MISSING` / `REPRODUCIBILITY-BLOCKED`, additive — 기존 기록 무변경).

## 배경 (한 줄)
7,306 문제 수가 가리키는 **실제 입력 파일(canonical_input)과 두 pinned 원장이 물리적으로 없음** →
문제 수를 입력에서 독립 재현 불가. 그래서 v1.2 지속-worker 구현을 멈추고 **입력 복구를 먼저** 한다.

## 제약 (binding)
- **네트워크 금지.** 로컬 고정본만: `execution-specs` 편집설치 checkout + census venv + census 원장
  (`out/records/*.jsonl`, `out/mining_ledger_*.json`) + 8개 제외 fixture(`local-docs/evm-zkvm-feasibility/project/cases/`).
- **문제별 예외·수동수정 금지.** canonicalizer는 결정적이어야 하고 8개 fixture 전부 byte-identical 재현.
- 기존 기록 무변경(v1.1 BLOCKED-BUDGET 등 보존). 새 증명·consensus·reward·Base 변경 금지. commit 없음(gitignored).
- git 복원 우선(재발명보다) — history에 canonicalizer가 남아있으면 그걸 복원.

## 계획 항목
- [x] archaeology 서브에이전트: lost canonicalizer는 세션 scratchpad에 byte-identical 생존 확인
      (git 이력 아님 — local-docs 전체가 gitignore). canonical_input 포맷은 guest engine.rs 역직렬화기로 확정.
- [x] canonicalizer RED→GREEN (T1): 8 fixture 알려진 canonical_input을 oracle로, `canon.py`가 8/8 byte-identical.
- [x] 7,838 선별 독립 재도출 (T2): 스키마 분류기가 10,174→정확히 7,838 (pinned nodeid 집합 완전 일치).
- [x] 7,838 canonical input 재생성 (T3): `canon.py`가 manifest를 byte-identical 재현, per-case 불일치 0.
- [x] 7,306 원장 정합 (T4): 모든 instance_payload_sha256 == sha256(재생성 canonical_input), 불일치 0.
- [x] pinned 해시 정확 대조: `full_runinput_7838.json == 959617fe…` MATCH,
      `census_ledger_7306_v1.json == a59e4bb2…` MATCH (둘 다 일치).
- [x] **`RECOVERED-BYTE-IDENTICAL`** 확정 + 실제 파일 + SHA-256 manifest 영구 보존:
      `local-docs/evm-execution-census-p0/recovered/` (canon.py·두 원장·선별·fixture 소스·재생성 harness·manifest·README).
- [ ] (다음, operator 승인 하) v1.2 지속-worker Stage-1 재개 — msg 3625 구조(worker 1회 준비 + supervisor 60s/case).

## 보고
Telegram chat_id 1311067056, 한국어 쉬운 말. public/API/mining/leaderboard 주장 아님 — closed-local만.

---

# EVM-P0 eligibility freeze → tracked evidence record (operator msg 3708)

## 결론
`EVM-P0-MINEABLE-ELIGIBLE = 6,767` 확정 산출물(계약·보존식·원장/증명 해시·경계)을 추적 문서
`docs/evm-census-p0-eligibility-freeze.md`에 append-only로 고정. 원장·증명 **원본은 gitignored
샌드박스에 유지**, 해시 + 계보만 in-tree. 발급 가능 수이며 네트워크 활성화 아님(mineable_now=0).

## 계획/진행
- [x] v1.5 계약 Accepted → emitter 1회 재실행 → 보존식 `6,855 = 6,767 + 79 + 7 + 2` 성립 → FROZEN.
- [x] S9 일반 검증기 RED→GREEN (admission 10/10, proof-layer 11/11): 잘못된
      task/input/oracle/fork/policy/acd/vk/proof + cross-case pv#1 전부 거절. consensus/BF.7 미연결.
- [x] S10 양성 증명 1건(정렬상 첫 duplicate non-survivor, 발급 6,767 밖): 압축 STARK 1회·재시도 0,
      4h/48GiB 이내. verify-probe ACCEPT, 음성 거절표 전부 통과.
- [x] 전수 census 6,767: 9개 축 완비, distinct pv#1 = 6,767, CERTIFIED.
- [x] 추적 문서 작성 + docs-smoke 핀(ceiling label·보존식·mineable_now=0·non-claim) → docs-smoke PASS.
- [x] branch → PR #116 → CI self-test(11m49s) + supply-chain(3m24s) green → main 머지
      (merge commit `ab5c4dea…`) → 원격 검증 local HEAD == origin/main → working tree clean.

## 경계
public/API/mining/leaderboard 주장 아님. 합의·BF.7 미연결. reward/Base 미변경. mineable_now=0 유지.
원본은 샌드박스에만, in-tree는 해시+계보만.

---

# Solidity P0 실물 family — 생성형(generative) 방법론 (operator msg 3711 step 1 + msg 3714)

## 목표
Solidity 발급 수 0을 **정직하게 회수**. 공개 테스트를 정답 문제가 아니라 **새 문제 template**로만 사용.
직답(direct-answer) family는 정답 co-location 때문에 이미 NO-GO(2026-08-07). 생성형 family는 정답
유출을 원천 우회한다. 결과: `SOLIDITY-P0-MINEABLE-ELIGIBLE=N` (또는 구조적 실패 시 =0).

## 방법론 (msg 3714, 8개 조항)
1. solc+Z3 오프라인 = 준비 관문일 뿐, 문제 수에 안 셈.
2. 공개 테스트 = 새 문제 template로만.
3. 서로 겹치지 않는 두 sub-family, anchor당 최대 1 task:
   - A. syntax/semantic → typed-hole compile-repair (front-end only, EVM 실행 불필요)
   - B. SMT → bounded expression synthesis (SMTChecker/Z3, EVM 실행 불필요)
4. 각 sub-family 결정적 대표 패턴 최대 3개만 시험. 문제별 수동수정 금지.
5. 필수 6조건: 원본 공개답 REJECT / 새 문제 답이 task·checker 바이트에 없음 / worker 합성 + verifier
   compiler 1회 / trivial 전부 REJECT / 대표 답은 fixture로 제외 / 전수 답 미리 생성·저장 안 함.
6. 한 sub-family라도 대표 관문 통과 → 승인 대기 없이 해당 전체 anchor 정확히 1회 변환·검증·집계.
7. 둘 다 구조적 실패 → `SOLIDITY-P0-MINEABLE-ELIGIBLE=0` 종결.
8. 성공 → 보존식·최종 원장 동결 → `SOLIDITY-P0-MINEABLE-ELIGIBLE=N` 확정.

## 진행
- [x] 준비 관문(prep gate) — 핀 soljson.js 0.8.36(sha256 `ccb677d5…`), Node v22, 오프라인.
      front-end 정상 컴파일/오류 거절 확인; SMTChecker 위반=반례·안전=증명 확인(Z3-in-WASM 작동).
      → sub-family A/B 둘 다 실행 가능. (숫자에 안 셈)
- [x] 대표 패턴 설계 확인 — msg 3714/3716 8조항 + 8조정으로 확정, 승인 대기 없이 cascade 진행 사전 허가.
- [x] sub-family A 대표 관문 TDD 구현·검증 — 3패턴×8행 = 24/24 GREEN (원본 int식·리터럴·단일변수·
      미사용변수·문법외 조건식·산술 전부 REJECT).
- [x] sub-family B 대표 관문 TDD 구현·검증 — 4패턴×9행 = 36/36 GREEN (합성 등가식 ACCEPT, 원본 P·
      여집합·항진·모순·단일변수·문법외 `^`·무관변수 전부 REJECT; host 진리표 교차검증 일치).
- [x] 전체 anchor 1회 cascade — A: `3,547 = 2 + 2,322 + 1,223`, B: `1,435 = 2 + 28 + 1,405`,
      WITNESS-REJECTED=0 (둘 다). 후보 anchor N_A=2, N_B=2 (아직 최종 아님 — trivial 관문 대상).
- [x] trivial-construction 관문 (운영자 msg 3722) — B 2건: 진리표→표준 DNF/CNF/QM-최소화 3종을 기계
      생성해 제출, 셋 다 ACCEPT(문법·크기 내) → `INELIGIBLE-TRIVIAL-CONSTRUCTION`. A 2건: 고정 보편식
      `p0 < p1` 하나가 두 anchor 모두 ACCEPT → 인스턴스별 합성 부재, 동일 triviality. A 24/24 battery는
      여전히 통과(soundness는 정상). SMT hard-error 0 / unknown-timeout 0.
- [x] 운영자 판정(msg 3724) — 옵션 1 승인: 후보 4건 전부 trivial → 최종
      `SOLIDITY-P0-MINEABLE-ELIGIBLE = 0`. 문서·docs-smoke 핀 N=0 재작성 → PR #117 → CI green → main.

## 리뷰 (Entry 1 완료 — N=0)
- 결과: **SOLIDITY-P0-MINEABLE-ELIGIBLE = 0**. 후보는 A 2 + B 2 = 4건 나왔으나, 운영자가 요구한 필수
  trivial-construction 관문에서 4건 전부 탈락. B는 진리표에서 표준 DNF/CNF/QM-최소화가 전부 문법 내
  ACCEPT(문법 `! && || == !=`가 함수완전 + 크기 상한 없음), A는 단일 보편식 `p0 < p1`이 모든 anchor를
  ACCEPT(요구가 "임의의 유효 bool 식"이라 목표관계 부재) → 둘 다 기계적으로 풀려 정직한 난이도 바닥이
  아님. 그래서 문자 그대로의 2가 아니라 일관된 0으로 종결.
- 이는 Solidity가 영구히 불가능하다는 뜻이 아니라, **현재 동결 P0 코퍼스 + 이 컴파일러 전용 A/B 계열**이
  정확히 0이라는 뜻. semanticTests(1,670, `DEFERRED-EVM-REQUIRED`)와 후속 테스트(709,
  `SUCCESSOR-OUT-OF-SCOPE`)는 폐기가 아니라 후속 범위. 계열 재설계는 이번 wave 범위 아님.
- 보존식(FROZEN): Stage A `12,931 = 6,652 + 5,726 + 288 + 214 + 51`; Stage B
  `6,652 = 0 + 2,322 + 28 + 2,628 + 1,670 + 4`; `709 = 7,361 − 6,652 SUCCESSOR-OUT-OF-SCOPE`;
  semanticTests `1,670 = 1,498 + 172` (전부 DEFERRED-EVM-REQUIRED). 5,450→0 = 직접경로 답 유출 +
  생성경로 triviality/생성불가.
- 동결 문서: 최종 원장 0행(`final_task_ledger.json`=`[]`, 답 저장 없음 vacuous), trivial 증거 원장 4건
  (발급 태스크 아님, 배제 증거), 코퍼스 fingerprint(syntax `ec0fdc62…`, smt `b05d389d…`,
  semantic `e0b9d4c3…`), 핀 soljson.js 0.8.36 sha256 `ccb677d5…`, 모듈/원장 해시 in-tree. 원본은
  gitignored 샌드박스.

## 경계
public/API/mining/leaderboard 주장 아님. 합의·BF.7 미연결. closed-local only. mineable_now=0 유지.
원본은 샌드박스에만, in-tree는 해시+fingerprint+계보만.

## sandbox / 도구
- sandbox: `local-docs/langspec-universe-p0-2026-07-23/solidity-family-impl/` (gitignored).
- 핀 컴파일러: `…/anchored-obligation-p2-2026-07-23/solidity-runtime/` node_modules의 solc 0.8.36
  (soljson.js sha256 `ccb677d54dfab2a9b30084eec6bb396c93eb86d58b42cc00267fd0f54f391f32`).
- native solc 소스 빌드는 오프라인 불가(submodule 비어있음·Boost/Z3/emscripten 없음) → soljson.js 경로 채택.
- anchor 코퍼스: `current-solidity.json` (syntaxTests 3,547 / semanticTests 1,670 / smtCheckerTests 1,435).
  실제 universe는 "핀 solc로 깨끗이 컴파일되는 base" 부분집합 → N은 cascade에서 창발.

## 경계
public/API/mining/leaderboard 주장 아님. 합의·BF.7 미연결. closed-local only. 아직 숫자·커밋 아님.

---

# zk-native release-audit 인구조사 P0 — 단일 wave 종결 (2026-08-10, 운영자 msg 3727/3728/3730)

목표: 동결된 zk-native release-audit anchor에서 채굴 가능(mineable-eligible) 문제 수 N을 확정하거나
N=0을 증명. 방향 검증에서 코퍼스↔방법 불일치를 발견(운영자 msg 3729 direction-check) → 운영자 확정
(msg 3730): **동결 16,763건은 Lean 정리 문제가 아니라 zk-native 소스 release-audit anchor**. Lean 전용
tactic 배터리 미적용. 최종 라벨 분리 확정.

## 실행 (docs-only, append-only)
- [x] STOP 게이트 확인: audit-pool anchor 수 = **16,763** (기존 동결과 일치);
      `observation-window-rp2-4.json` sha256 `fab8439a…` 일치; 보존식 일치 → STOP 미발동.
- [x] 동결 입력 digest 전수 재계산(`shasum -a 256`): gate-results `0dd847fb…`, gross-candidates
      `fd584ec0…`, source-universe(54 repos) `54aa8fb1…`, canonical-transitions(183) `14abf11c…`,
      archive-manifest(199) `e9ea9b54…`, window-end-presence `a1c5d620…`, checkpoint-rp4 `9361be51…`,
      emitter `19058c28…`. Lean toolchain `leanprover/lean4:v4.29.1`.
- [x] anchor→source 진짜 결속 교차검증: gross-candidates row0와 gate-results row0가 동일 `task_id`
      (`05a3eeff…`)·동일 input/target digest(`2ea93dbd…`/`0e8fbe0a…`) 공유 → 실제 저장소
      (`0xMiden/miden-vm`)·경로·커밋·내용해시 결속. 가짜 seed 아님.
- [x] N=0 근거: 16,763건 전부 `spec_fixed`/`deterministic_budget`/`generic_theorem_exclusion` 3개
      spec 게이트 pending 16,763/16,763 + 오라클/체커 부재 → `eligible = 0`.
- [x] 동결 문서 작성: `docs/zk-native-release-audit-census-p0-eligibility-freeze.md` (append-only Entry 1).
- [x] docs-smoke.sh 핀 블록 추가 → `docs-smoke: PASS`, `git diff --check` PASS.
- [ ] commit(NotoriAndo, AI attribution 없음) → branch push → PR → CI self-test·supply-chain·verdict-corpus
      green → **rebase merge** → local main == origin/main → working tree clean.

## 리뷰 (Entry 1 완료 — N=0)
- 결과: **ZK-NATIVE-RELEASE-AUDIT-P0-MINEABLE-ELIGIBLE = 0**, **LEAN-P0 = CORPUS-NOT-MATERIALIZED**
  (엄격히 분리, "ZK-NATIVE/LEAN-P0 = 0" 표현 금지). 16,763 anchor는 실제 소스 결속은 진짜지만 검증 계약
  (고정 spec·오라클/체커·결정론적 예산·일반성 근거)이 전무 → 발급 가능 태스크 0.
- 보존식(FROZEN): 첫 미충족 조건만 primary bucket으로 — `16,763 = 0 MINEABLE-ELIGIBLE + 16,763
  NEEDS-SPEC`. NO-DETERMINISTIC-BUDGET·GENERALITY-UNRESOLVED는 같은 16,763행의 **보조 상태**로만 기록
  (분모에 중복 가산 금지).
- 기존 Lean 체커(`v1-lenbound` 계열)는 이 소스-audit anchor와 배선 경로가 없어 미적용. 코퍼스에 .lean
  파일 0·Lean 정리 태스크 0이므로 Lean 문제 수를 0으로도 주장하지 않음(CORPUS-NOT-MATERIALIZED).
- 경계: N>0은 새 audit spec·오라클/체커·예산 설계가 필요 — 이번 P0 범위 밖. mineable_now=0.
- 도메인 소계: `EVM 6,767 + Solidity P0 0 + zk-native release-audit P0 0 = 6,767`. Lean은 미합산.

## 경계
public/API/mining/leaderboard 주장 아님. 합의·BF.7·reward·Base 미변경. closed-local only. 원본 대형 원장
(anchor 원장·소스 스냅샷·emitter)은 gitignored 샌드박스에만, in-tree는 해시+계보+보존식만.

---

# Solidity semantic P1 — EVM 실행증명 인구조사 종결 (2026-08-10, 운영자 msg 3736/3739/3742)

목표: compile-only Solidity P0가 유보(`DEFERRED-EVM-REQUIRED`)한 1,670 semanticTests를 **실제 EVM 실행
케이스**로 물질화해, 각 케이스에 대해 "정확한 EVM 실행"의 압축 zkVM 증명(SP1)을 만들 수 있는지로 채굴가능
문제 수 N을 확정. 공개된 기대출력은 답 유출 아님(채굴 산출물은 출력값이 아니라 실행 증명). 실제 증명이 이미
생성된 fixture만 제외.

## 실행 (docs-only, append-only)
- [x] 입력 동결(`input-freeze.json`): semanticTests 코퍼스 aggregate sha256 `f0af98e6…`(1,670 파일,
      freeze 시점 byte-exact 재현 확인), 핀 solc 0.8.36 `ccb677d5…`, 엔진 `sp1=6.3.1 / revm=38.0.0 /
      alloy=1.5.6`, 자원정책(8M cycle 상한·network 0·retries 0).
- [x] Level 0 파일원장(`file-ledger-step2.json`): `1,670 = 1,519 CASES-MATERIALIZED + 109
      HARNESS-UNSUPPORTED + 42 NO-RUNNABLE-CASE + 0 ERROR`.
- [x] Level A 컴파일분류(`compile-ledger-v2.json`, 각 test 저자지정 컴파일설정 준수·viaIR 전역적용 금지 —
      운영자 msg 3739): `1,519 = 1,474 CANDIDATE + 20 EVM-VERSION-OUT-OF-PIN + 23 ABI-ENCODER-V1-REQUIRED
      + 2 SOLC-0.8.36-INCOMPATIBLE`. 후보 1,510→**1,474** 정정.
- [x] 대표 관문: `array/fixed_arrays_in_constructors.sol` 실제 압축증명 1회 생성(`vk 0x004a7485…`,
      circuit `v6.1.0`, `verify: accepted`, proof.bin `96a7ef57…`). → 답확정 fixture로 최종 후보에서 제외.
- [x] 태스크 단위 동결(실행 전 확정 — msg 3742): 1 `.sol` + 순서있는 전체 call 번들 = 1 태스크.
      legacy/viaIR "also"는 같은 의미태스크의 컴파일 파이프라인 변형 → 분할 안 함. 파라미터 스윕 없음.
- [x] 전수 실행(1,474): native revm/Cancun 왕복검사(`verify-ledger-all.json`: 1,418 ACCEPT / 42 MISMATCH
      / 14 ORACLE_UNSUPPORTED) + SP1 cycle 측정(`cycles-all.json`: ≤8M 1,453 / >8M 21).
- [x] Level B 케이스원장(`case-ledger-v2.json`, 단일 primary bucket 우선순위 fixture→oracle-fail→cost→
      dup→eligible, 보존식 OK·0 error): `1,474 = 1,396 MINEABLE-ELIGIBLE(N) + 1 EXCLUDED-PROOF-FIXTURE
      + 0 DUPLICATE + 44 UNSUPPORTED-ORACLE + 12 EXECUTION-MISMATCH + 21 DEFERRED-HIGH-COST`.
- [x] 경계 무모호성: 최대 eligible 7,308,504 cycle(`bytesx_v2.sol`, 8M 아래 691,496 여유), 최소 deferred
      8,700,111 → 8M 선 넘는 gap ~1.39M ≫ placeholder/real digest drift ≤~1,000.
- [x] 동결 문서(`docs/solidity-semantic-p1-execution-proof-eligibility-freeze.md`, append-only Entry 1)
      + docs-smoke.sh 핀 블록.
- [x] docs-smoke.sh PASS + `git diff --check` PASS → commit(NotoriAndo, AI attribution 없음) → branch push
      → PR → CI self-test·supply-chain green → merge → local main == origin/main → working tree clean.
      **merged 3ec0111** (`docs(census): freeze Solidity semantic P1 EVM execution-proof eligibility (N=1396)`).

## 리뷰 (Entry 1 완료 — N=1,396)
- 결과: **SOLIDITY-EVM-EXECUTION-PROOF-P1-MINEABLE-ELIGIBLE = 1396**. N은 엄격한 하한 — N에 든 모든
  케이스는 저자오라클 왕복검사 통과 **and** 8M cycle 이하. 공개 기대출력은 답 유출 아님(P0 compile-only의
  answer-leakage와 구분): 채굴 산출물은 출력값 O가 아니라 "EVM이 바이트코드를 실행해 O를 냈다"는 압축 STARK
  증명이라, O를 알아도 유효 실행증명 생성을 지름길로 만들지 못함.
- 3개 배제군: `UNSUPPORTED-ORACLE 44`(host globals·CREATE nonce·isoltest account·anonymous events·
  sub-4byte revert 비교기 한계·indexed-dynamic) + `EXECUTION-MISMATCH 12`(host 무관·faithful 물질화인데
  관측≠오라클 — 후속 포렌식, 일부는 오라클 기록 아티팩트일 수 있음) + `DEFERRED-HIGH-COST 21`(>8M, 11건은
  16M 측정캡). 셋 다 N에서 보수적으로 제외.
- 엔진 동결: guest = 진짜 EVM(revm, Cancun, deployer nonce 0, basefee 0). isoltest MockedHost quirk는
  재현하지 않음 → host 의존 오라클은 UNSUPPORTED-ORACLE로 버킷팅(엔진·vk 동결 유지). 증명 결속 =
  guest ELF(`1599d54f…`) + vk(`0x004a7485…`) + SP1 verifier digest.
- 도메인 소계 갱신: `EVM 6,767 + Solidity-compile P0 0 + Solidity-semantic P1 1,396 + zk-native P0 0 =
  8,163`. (Solidity P0와 P1은 같은 코퍼스의 다른 계열 — P0가 유보한 1,670을 P1이 물질화.)

## 경계
public/API/mining/leaderboard 주장 아님. 합의·BF.7·reward·Base 미변경. mineable_now=0. closed-local only.
guest/host 구현·핀 컴파일러·코퍼스·물질화 케이스·run 원장·대표 증명은 gitignored 샌드박스에만, in-tree는
해시+코퍼스 지문+보존식+계보만. DEFERRED-HIGH-COST 21·EXECUTION-MISMATCH 12는 폐기가 아니라 유보(후속).

---

# Solidity semantic P1 — Entry 2: EXECUTION-MISMATCH 12건 읽기전용 원인감사 + reclaim (운영자 msg 3744)

지시(msg 3744): 12건 **읽기전용** 원인감사. 기존 1,396 동결기록 **수정 금지**, 회수건은 **successor 부록**으로
추가. 원인별 5버킷 분류(AUTHOR-ORACLE-MISREAD / CANONICALIZER-DEFECT / PINNED-ENGINE-DIVERGENCE /
HARNESS-DEFECT / UNRESOLVED). **결함 확인 범위만** 재계산. 그다음 Rust·Ethereum-consensus로.

- [x] 12건 전수 분류: **5 AUTHOR-ORACLE-MISREAD + 7 HARNESS-DEFECT + 0 CANONICALIZER-DEFECT +
      0 PINNED-ENGINE-DIVERGENCE + 0 UNRESOLVED**. 12건 모두 우리 census 툴링 결함(엔진은 매건 정답) →
      전건 false-exclusion, 전건 reclaim 가능(엔진발산 0이라 비회수 사유 없음).
- [x] HARNESS 7건 증명(실행, 동결 바이너리): 오손된 calldata를 샌드박스에서 정정(`reclaim/build_corrected_input.mjs`,
      isoltest 원칙 — `hex""`는 raw unpadded, 빈 `""`는 offset+length) → **동결** native-exec 재실행 →
      **동결** `verify.mjs` 채점 = **7/7 ACCEPT**. 정정입력 cycle 재측정(**동결** guest ELF `1599d54f…`
      `exec-many`) = 751,326~3,461,571(max 3.46M, 8M 아래 4.54M 여유) → 전건 eligible.
- [x] MISREAD 5건 증명(읽기전용, 엔진 재실행 없음): isoltest `// ----` 스펙에서 올바른 `abi.encode(bytes)`
      독립 재구성(`reclaim/reconstruct_misread.mjs`, 반환 blob 내부의 4바이트 selector는 워드패딩 아님) =
      **동결 관측출력과 5/5(13행) 일치**. 기록 오라클은 자기모순(길이헤더 vs 워드패딩 내용), 관측은 정합 ABI.
      입력 무오손이라 동결 census cycle 유효 = 1,278,924~3,065,090(max 3.07M ≤8M) → 전건 eligible.
- [x] reclaim 항등식: `Entry 1 N 1396(불변) + reclaim 12 = successor 1408`. `EXECUTION-MISMATCH 12→0`,
      `MINEABLE-ELIGIBLE`에 +12. 다른 버킷 불변.
- [x] 동결문서 append-only Entry 2 작성(`…-SUCCESSOR = 1408`, Entry 1 한 줄도 안 건드림) + docs-smoke.sh
      Entry 2 핀 4줄 추가. `docs-smoke: PASS`, `git diff --check` PASS.
- [x] commit(NotoriAndo, AI attribution 없음) → feature branch push → PR → CI self-test·supply-chain green
      → merge → local main == origin/main → working tree clean → 텔레그램(chat 1311067056) 한국어 완료보고.
      **merged 004e932 (#120)**. (2026-08-20 backfill 시 사후 체크 — 당시 완료 후 todo.md 갱신 누락.)
- [x] (다음) Rust·Ethereum-consensus 도메인 census — rust-exec-proof-p1 Entry 2~4 (#122, #124),
      eth-consensus Entry 3~5 (#128, #130)로 완료. (2026-08-20 backfill 시 사후 체크.)

## 리뷰 (Entry 2 — reclaim / successor N=1,408)
- 결과 한 줄: EXECUTION-MISMATCH 12건은 **전부** 우리 census 툴링 결함(materializer 오손 7 + extractor 오독 5),
  **엔진 발산 0**. 동결엔진을 정정입력에 재실행(7/7 ACCEPT)·독립 ABI 재구성(5/5 일치)으로 증명 → 전건 회수.
  Entry 1의 1,396은 불변 v1 동결, 1,408이 정정 후속치.
- 읽기전용 규율: 동결 ELF/vk/코퍼스/1,396 원장 무변경. "읽기전용"=동결물 불변; **동일** 동결 바이너리를
  **정정** 입력에 재실행하는 건 vk가 ELF에 결속(입력 아님)이라 동결 위반 아님. 정정입력·재구성은 새 `reclaim/`
  샌드박스에만 기록.
- 도메인 소계(후속치 반영): Solidity-semantic P1 = 1,396(v1 동결) → **1,408**(successor). 다른 도메인 불변.

---

# LLM-mineable eligibility census P1 — 통합 원장 Entries 1~28 (2026-08-16~21) — backfill·현재 진행 기록

이 구간은 진행 당시 todo.md에 기록되지 않고 텔레그램 보고 + 원장
(`docs/llm-mineable-eligibility-census-p1.md`)으로만 추적되었다. 운영자 지시(msg 4137)로 사후 backfill.
수치·지문·상세의 단일 출처는 원장이며, 이 섹션은 진행 추적 요약이다.

## Entries 1~17 (한 줄씩 — 원장이 상세 보유)

- [x] E1~2 (08-16): 라벨 교정 — `EXECUTION-PROOF-ELIGIBLE-SUBSET = 12,880`로 개명(수치 불변),
      `LLM-MINEABLE-ELIGIBLE = NOT-YET-DETERMINED` 선언; 중단 wave 챌린지 7 CANCELLED / 0 CONSUMED.
- [x] E3~6 (08-16): 첫 gated LLM 패밀리들 + gemma 기준 검정 — EVM/Solidity 모두 REFERENCE-UNSOLVED,
      LLM-TASK-ELIGIBLE = 6,755 → 7,954.
- [x] E7~11 (08-16): LLM-MINER-INTERFACE V1 → V1.2 동결 + agentic 기준 측정(EVM 3/12 @epoch 2 등) —
      여전히 미검정, 수치 불변.
- [x] E12 (08-17): MULTI-DOMAIN-LLM-FAMILY-V1 정체성 교정, 87,235 templates —
      **LLM-MINEABLE-ELIGIBLE-V1 = 2,040** (gemma 시대).
- [x] E13 (08-17): rust anchor-coupled fresh-repair 검정 2/12 **FAILED** — census 미실행, 정직 기록.
- [x] E14 (08-17): Opus 5 런타임 substitution → **MODEL-SUBSTITUTION-HARD-STOP**, 채점 0건 채택,
      OPUS48 wave 사전등록.
- [x] E15 (08-17): claude-opus-4-8 격리 기준 검정 — EVM/Solidity/Rust 각 **12/12 PASS**.
- [x] E16 (08-17): 전 도메인 frontier 종결(87,235 전수) — **LLM-MINEABLE-ELIGIBLE-V2 = 10,702**,
      UNRESOLVED = 0. fingerprint 규칙 명문화: 다른 cut에서 잰 12/12는 이전 안 됨.
- [x] E17 (08-19): S-1 semanticTests census (모델 0회) — V3-CANDIDATE = 1,583 / 1,670.

## Entries 18~28

- [x] E18 (08-19, census msg 4070 / docs msg 4119, merged fda199c #148): W2 electra/fulu census
      (모델 0회) — V3-CANDIDATE = 1,581 / 2,880. 교차포크 decode ~204행은 W2b로 명시 유보.
- [x] E19 (08-19, wave msg 4114 / docs msg 4119, merged fda199c #148): V3 승격 검정 —
      W1 12/12 + W2 12/12 (24 에피소드, claude-opus-4-8 격리, $2.93 / 상한 ~$5), 두 풀 통째 승격 →
      **LLM-MINEABLE-ELIGIBLE-V3 = 13,866** = 10,702 + 1,583 + 1,581.
- [x] E20 (08-19, census msg 4121 / docs msg 4124, merged 297e4e5 #149): W2b 교차포크 decode census
      (모델 0회) — 98 ERROR 행 중 **W2B-CANDIDATE = 97** (electra-dir→deneb 46, fulu-dir→electra 51),
      1 ORACLE-OR-CHECK-FAILED(COLLATERAL-DISTURBANCE) 정직 제외.
- [x] E21 (08-20, wave msg 4129 / 명명 msg 4133, merged 52c2bcc #150): W2b 승격 검정 **12/12**
      (12 에피소드, $0.5409 / 상한 $2, 자기검사 10/10 NC1~NC8, 모델 증거 12/12 전부 claude-opus-4-8,
      substitution 없음) → **LLM-MINEABLE-ELIGIBLE-V4 = 13,963** = 13,866 + 97.
- [x] E22 (08-20, merged 2de2b16 #152): V4 13,963의 anchor-coupling 소재 감사 봉인 —
      **MATERIAL-PROJECTION-UNIQUE 2,398 + H-SYNTHETIC 9,537 + MATERIAL-PROJECTION-DUPLICATE
      2,028 = V4 13,963**. 이 분해는 V4 전용이며 이후 V5 전체 분해로 확대하지 않는다.
- [x] E23 (08-20, merged 34eecf9 #153): Solidity successor v2 — syntax Stage A **1/3 FAIL**,
      SMT 0/3 미측정·미집계, 전수조사 없음, 증가분 0. 실패 후 같은 wave 재수정·재측정 없음.
- [x] E24 (08-21, PR #154): `RUST-TUPLE-STRUCT-PROJECT-V1` 사전등록 — 320 후보·대표 12건·
      generator/checker/prompt/toolchain 지문과 3/3→12/12→전수 1회 규칙을 모델 호출 전에 동결.
- [x] E25 (08-21, merged d58ca70 #154): Rust tuple family 대표 **12/12 PASS** 후 320행 전수 1회 —
      raw issuable 199, 내부 중복 초과 2를 제거한 고유 신규 **197** →
      **LLM-MINEABLE-ELIGIBLE-V5 = 14,160** = V4 13,963 + 197.
- [x] E26 (08-21, merged 6a6c970 #155): zk-native P0 소재 조사 종료 —
      `NO-CLEAN-A-ROOTED-FAMILY-UNDER-P0`, 증가분 0. 이번 조사 범위의 종료일 뿐 영구 불가능 주장이 아님.
- [x] E27 (08-21, merged 6a6c970 #155): 실제 모델 답안 1건을 ProofIntake→frozen checker→
      miner-local receipt·개발 장부까지 연결. census·V5·합의·보상 변화 0.
- [x] E28 (08-21, 이번 docs sync): E27 범위 정정 — 실제 의미 판정은 외부 frozen checker가 먼저 냈고
      `boole_miner`가 그 verdict를 결박·전달했다. **실제 `boole-node`의 독립 checker 재실행은 NOT-RUN**.
      기존 E27 증거는 보존하고 과장된 node-verifier 해석만 append-only로 교정.
- [x] E1~E27 docs 단계 게이트: docs-smoke + `git diff --check` → NotoriAndo author 커밋 →
      feature branch → PR → CI self-test·supply-chain green → squash merge → remote 검증(local == origin).
- E28 게이트 규칙: docs-smoke·diff-check→CI green→main merge→remote 검증이 실제 완료되어야만
  완료로 보고한다. 이 문서는 PR 번호·merge SHA를 미리 지어내지 않으며 실제 값은 최종 보고에 남긴다.

## 리뷰 (census P1 구간 종합)

- 현재 공식 수치: **V1 = 2,040 / V2 = 10,702 / V3 = 13,866 / V4 = 13,963 /
  V5 = 14,160**. V5 도메인 합은 **EVM 6,755 + Solidity 2,782 + Ethereum-consensus 3,718 +
  Rust 905 + zk-native 0 = 14,160**. 전부 family 보정과 전수 materialization을 통과한 발급 가능
  템플릿 수이며 14,160개를 모델이 개별 풀이했다는 뜻이 아니다. `mineable_now = 0` 불변.
- 승격 프로토콜 확립(E19·E21에서 2회 실증): 사전등록 동결(승인 원문 내장, placeholder hard-stop) →
  자기검사 음성통제(NC 조작 주입 전건 자체 정지) → 격리 opus-4-8 에피소드(재시도 0, 수동 수정 0,
  이중 모델 검증) → seal(구성요소 지문 동결→봉인 무변경 확인) → 원장 append-only Entry.
- 실패도 원장에 남긴다: E13(2/12 FAILED), E14(substitution HARD-STOP), E20의 1 제외행 — 재시도·은폐 없음.
- 완료된 옛 backlog: Solidity successor는 E23에서 실패 봉인, Rust tuple successor는 E24~25에서 +197,
  zk-native P0는 E26에서 범위 한정 종료.
- [x] **Native shadow Phase 1 — PR #166, `131244f`**: registry/identity/row-owned
      `registryDigest` + `Disabled`/terminal-history bootstrap 데이터 계층.
- [x] **Native shadow Phase 2 — PR #167, `4e19d1e`**: challenge 상태기계, durable journal,
      boot replay/recovery 데이터 계층.
- [x] **Native shadow Phase 2C — PR #168, `eff95658`**: evidence-first terminal,
      single-journal consumption/exhaustion projection, strict replay, stuck `InFlight` fail-closed.
- [x] **Native shadow Phase 2D — PR #170, `33dcc025`**: durable row는 `Consumed`로
      유지하고 같은 terminal event의 exhaustion projection이 맞을 때만 route-free resolver가
      `challenge_exhausted`를 파생한다. journal에서 도달 불가능한 stored/bootstrap
      `ChallengeState::Exhausted` 경로를 제거했고 mismatch는 revival 없이 fail-closed.
- [x] **Phase 3A.1 same-FD journal authority — PR #171, `6cc34b4`**: 같은 저널 inode/파일 descriptor를
      전 수명 동안 잡는 nonblocking `flock` 권위 아래 replay·torn-tail 절단·append·fsync를
      모두 같은 descriptor로 수행한다. symlink/비정규 파일/경로 교체/다른 authority·재개방
      이어쓰기는 fail-closed. 이는 route 연결 전 portable foundation이며 실제 두 node-process
      통합 관문이나 full RED matrix GREEN을 뜻하지 않는다.
- [x] **Phase 3A.2 route-free `native_busy` single-slot primitive**: 향후 AppState가
      노드 단위로 단 하나를 소유할 비대기 1-slot permit을 구현했다. 획득 실패는 exact
      `native_busy`, 정상·오류·panic 모든 경로에서는 permit이 해제되고 경쟁 thread에서는
      정확히 하나만 획득한다. route가 상태/저널 변경 전에 이를 호출해야 한다는 순서 fixture도
      고정했지만, 그 순서를 실제 route가 아직 강제하지는 않는다.
      아직 AppState가 이 permit을 단 하나 소유하거나 실제 route가 stage 5에서 획득하는 결선은
      없으므로 full gate 11 GREEN은 아니다. 이 체크도 현재 slice의 required CI·merge가
      완료되어 이 항목 자체가 main에 도달할 때만 권위가 생긴다.
- [ ] **Native shadow Phase 3B — Linux containment + route/checker execution**: delegated cgroup
      v2 권한이 있는 named Linux runner, 전용 UID/GID, privilege-drop/ownership 모델을 먼저
      고정한 후 cgroup/tmpfs/seccomp/Landlock, cleanup/recovery, non-Linux startup refusal,
      raw-answer route를 RED→GREEN. generic `ubuntu-latest` skip/fake backend로 GREEN 선언 금지.
- [ ] **Native shadow 최종 관문**: 실제 node-process raw-answer 1회 + 전체 거절/replay
      matrix. 이때까지 `NATIVE-SUBMISSION-SHADOW-ADMISSION-V1-GREEN` 미획득. 기본 OFF,
      `boole-node→boole-miner` 의존·기존 `/submit`/`/receipts` 재사용·`boole-core`/SharePool/
      block/reward/P2P/BF.7 변경 금지.

## 경계

public/API/mining/leaderboard 주장 아님. 측정 원본(스크립트·동결·transcript)은 gitignored `local-docs/`
샌드박스에만, in-tree는 원장의 지문·보존식·계보만. 유료 실행은 매 wave 운영자 승인 + 상한 내 구독 CLI로만.
`mineable_now = 0`.
