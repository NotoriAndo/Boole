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
- [ ] PR → CI green(신규 스테이지가 클린 러너에서 실제 실행) → 머지 →
      remote 검증 → N3 closure 기록 → 보고

## Review
(작업 완료 후 기록)
