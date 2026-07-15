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
