# 개발용 MCP 문제 조회와 VM 제어

운영자가 준비한 개발용 tuple 문제를 **파일 경로 전달 없이 MCP로 조회하고 제출**하는 경로다.
`development-task-admission`이 켜진 별도 Linux 서비스만 지원한다.
Lima 개발 환경이며, Mac `product ...-direct-boot` 제품 설치 경로나 공개 배포판과 같지 않다.

깨끗한 Mac 검증은 사용자의 2026-09-15 결정에 따라 마지막 설치 검증으로 미룬다.
현재 Mac·CI 개발을 막지 않되, `CURL.3`은 여전히 미통과다.

## 준비 조건

- Linux VM에 [개발용 tuple-task 접수](development-tuple-task-admission.md)의 서명·소유권·
  격리 조건을 만족하는 runtime과 문제를 운영자가 설치한다. 문제별 후보 1개·checker 최대 1회다.
- VM 이름은 `boole-mcp-dev-*`이며 이미 존재해야 한다. 명령이 VM을 새로 만들지 않는다.
- Linux, `plain: true`, 공유 폴더 없음, SSH agent/개인 SSH 키/X11 전달 없음,
  proxy 환경 및 추가 guest 환경 전달 없음, `127.0.0.1:8082`의 동일 guest 포트만 전달하는 설정이다.
- guest에는 Python 3와 비대화형 `sudo`로 제어 가능한 `boole-dev-network.service`,
  `boole-native-shadow-launcher.service`, `boole-native-shadow-replay-node.service`가 있다.
  네트워크 제한 서비스는 운영자가 설치·검증한 개발용 출력 방화벽을 적용해야 한다.
- MCP client가 이번 조회 도구를 포함하는 `boole-mcp` 실행 파일을 사용해야 한다.
  이전 바이너리를 등록한 client는 새 도구가 보이지 않는다.

이 명령은 VM 내용 전체나 방화벽 규칙을 새로 인증하는 도구가 아니다. 준비된 이미지·서비스를
전제로 위험한 host 공유 설정과 전달 경로를 검사하며, 실제 checker의 격리·서명 검사는 기존
qualified launcher와 node가 계속 소유한다. 키나 grant를 생성·교체하지 않는다.

## 1. 동일한 명령으로 시작·상태 확인·종료

저장소에서 Python 3와 `limactl`이 있는 환경을 사용한다. 아래 예시 이름을 **운영자가 준 실제
전용 VM 이름**으로 바꾼다. 없는 이름이면 거부되며 기본 VM을 선택하거나 생성하지 않는다.

```sh
BOOLE_DEV_VM='boole-mcp-dev-example'
python3 scripts/boole_dev.py start --vm "$BOOLE_DEV_VM"
python3 scripts/boole_dev.py status --vm "$BOOLE_DEV_VM"
```

`limactl`이 PATH에 없으면 각 명령에 `--limactl /실제/경로/limactl`을 추가한다.

시작은 VM 설정 확인 → VM 시작 → 네트워크 제한 서비스 확인 → launcher/node 시작 →
guest와 host의 준비 상태 대조 순서다. 이미 정상 실행 중이면 서비스를 다시 시작하지 않는다.
준비를 기다릴 때는 읽기만 반복하며, 실패했다고 키·grant·예산을 초기화하지 않는다.
시작 실패 시 VM은 보존되므로 상태를 확인하거나 아래 정상 종료 명령을 사용한다.

```sh
python3 scripts/boole_dev.py stop --vm "$BOOLE_DEV_VM"
```

종료는 node/launcher 정지 후 일반 `limactl stop`을 사용한다. 강제 종료·VM 삭제·디스크 삭제·
예산 초기화는 없다. 이미 정지한 VM에 다시 실행해도 읽기만 한다.

상태 출력에서 `ok: true`는 조회 명령이 정상 완료됐다는 뜻이지 제출 허가가 아니다.

| 표시 | 뜻 |
|---|---|
| `serviceReady=true`, `submissionAllowed=true`, `taskState=unused` | 해당 시점에는 첫 후보를 제출할 수 있음 |
| `serviceReady=true`, `submissionAllowed=false`, `taskState=consumed` | 서비스는 정상이나 이 문제의 후보는 이미 사용됨 |
| `serviceReady=false`, `taskState=unavailable` | 검증 서비스가 준비되지 않았거나 안전하게 사용할 수 없음 |
| `vmState=Stopped`, `taskState=unknown` | VM은 정지됨. 디스크의 예산을 미사용으로 추정하지 않음 |
| `ok=false` | 연결·설정·host/guest 대조 실패. 제출하지 말고 오류를 해결해야 함 |

host와 선택한 guest의 응답이 다르면 준비 완료로 표시하지 않는다. 과거 `prepared.json`의
0회 기록은 현재 상태가 아니며, 재시작으로 사용된 문제를 다시 열 수 없다.

## 2. MCP에서 문제와 상태 조회

필요하면 host용 MCP 바이너리를 빌드하고 기존 설치 절차로 등록된 실행 파일을 갱신한다.
개인 MCP 설정은 위 VM 명령이 자동 변경하지 않는다.

```sh
cargo build --locked --release -p boole-mcp --bin boole-mcp
```

기존 MCP 설정에 도구 허용 목록이 있다면 아래 세 도구가 허용되는지 확인한다.
기존 제출 승인 정책을 없애거나 다른 도구까지 허용할 필요는 없다.

| 도구 | 입력 | 실제 경로 / 효과 |
|---|---|---|
| `boole.problem_native` | `{}` | `GET /native-shadow/problem`: 공개 문제·scaffold·제출 식별정보만 조회 |
| `boole.status_native` | `{}` | `GET /native-shadow/status`: 서비스 준비와 후보 사용 상태만 조회 |
| `boole.verify_native` | 기존의 정확한 6개 필드 | `POST /native-shadow/submissions`: 후보를 접수하고 검증·영수증 반환 |

새 대화에서 다음과 같이 **읽기만** 요청할 수 있다.

> Boole의 `boole.status_native`와 `boole.problem_native`로 준비 상태와 공개 문제를 확인해.
> 파일 경로나 식별정보를 추측하지 말고, 아직 답안을 생성하거나 제출하지 마.

문제는 서명 검증을 마친 설치 grant에서 다시 생성한 공개 자료의 투영이다. 전역 편집 계약,
family, 공식 anchor/helper, 출력 형식, 정확한 5개 제출 식별 필드, 문제의 wrapping-i64 의미와
scaffold를 포함한다. 임의 파일 경로, 개인 키, 예산 파일, 정답, 숨겨진 테스트, 내부 task seed는
반환하지 않는다. 과거 고정 replay/canary 서비스에는 이 개발용 조회 경로가 없다.

조회는 후보·실행·재전달 예산과 verdict journal을 쓰지 않는다. 반환값은 그 시점의 관측이지
제출 예약이나 새 실행 권한이 아니다. 다른 호출과 경쟁하면 기존 제출 경로가 최종적으로 거부할 수 있다.

## 3. 별도 실행 승인 후 첫 후보만 제출

실제 모델 실행은 이 구현의 자동 동작이나 검증 범위가 아니다. 사용자가 별도로 풀이·제출을
요청한 경우에만 공개 문제의 `submissionIdentity` 5개 필드에 `rawAnswer` 하나를 더해
`boole.verify_native`로 보낸다. 다른 task 식별정보나 경로를 혼합하지 않는다.

정상 응답의 `outcome`, `reasonCode`, BF.3 `receipt`, `evidenceDigest`, `redelivered`를
그대로 보존한다. 이후 상태 조회는 가능하지만 결과 영수증을 새로 가져오거나 재전달하지 않는다.
`checkerExecutionReserved=true`는 node의 실행 예산이 예약됐다는 뜻이며, 실제 checker 완료나
ACCEPT 횟수의 독립 증명이 아니다. 빈 답안/입력 거절도 후보를 소모할 수 있다.

통신 결과가 불명확하면 새로운 후보·자동 재제출·상태 초기화를 하지 않는다. MCP는
`retryAuthorized=false`와 task별 재전달 권한을 확인하라는 안내를 반환한다. 개발용 문제의
동일 결과 재전달은 기존대로 별도의 운영자 서명을 요구한다. legacy `receipt.get`은 이
BF.3 결과 조회 도구가 아니며, `boole.status`도 이 VM의 준비 상태 도구가 아니다.

## 검증 범위

동작은 테스트 우선으로 추가했다. 로컬 직접 소비자 검사는 공개 자료·identity 대응,
조회 전후 예산 바이트 불변, 첫 제출, 빈 답안 소모, 재시작 후 사용 상태, 미승인 재전달 거부,
오래된 서비스의 조회 거부, HTTP/stdio 배선과 제어 메시지 응답성을 확인한다.
MCP 조회도 기존 전용 loopback client만 사용하며, 5초·64 KiB 제한, proxy/redirect/자동 재시도 금지를 유지한다.

포함 PR의 full CI는 실제 Linux MCP → 문제/상태 조회 → 합성 후보 → qualified contained checker →
판정/영수증, 사용 상태와 restart/redelivery 불변을 기존 두 아키텍처 gate에서 확인한다.
합성 답안은 MCP로 받은 공개 자료에서 만들며, 실제 모델 실행 결과로 세지 않는다.
VM 제어 순서·실패·보존 경계는 외부 Lima CLI를 대체하는 계약 테스트로 검사한다.

현재 Mac에서는 기존 정지 VM에 새 `status` 명령을 읽기 전용 실행했다. 이번 구현 과정에서
그 VM을 재부팅하거나 사용된 문제·키·디스크를 변경하지 않았다. 새 runtime으로 기존 VM을
자동 업그레이드했다거나 전체 새 사용 흐름을 이 Mac에서 다시 실행했다는 뜻은 아니다.
clean-Mac 설치, 공개 서명 release, 추가 모델 실행, 공개망·채굴·보상은 완료 주장에 포함하지 않는다.
유료 검증 구매자/LOI의 최신 확인값은 이번 개발에서 확보하지 않았다.

### 공급망 검사에서 발견한 의존성 수정

첫 [CI 실행의 supply-chain 실패](https://github.com/NotoriAndo/Boole/actions/runs/34866798198/job/104052589257)는
기존 `rustls 0.23.40`에 대한 새 보안 권고
[RUSTSEC-2026-0285 / GHSA-2mjx-qc3c-rqvc](https://github.com/rustls/rustls/security/advisories/GHSA-2mjx-qc3c-rqvc)였다.
검사 예외를 추가하지 않고 `rustls 0.23.45`와 필요한 `rustls-webpki 0.103.15`로 잠금 파일만 갱신했다.
최신 권고 데이터베이스로 로컬 `cargo deny`와 `cargo audit --deny warnings`가 통과했으며,
직접 소비자 MCP·miner·CLI의 통신 회귀 검사와 수정 commit의 full CI를 별도로 적용한다.
원래 실패한 실행은 삭제하거나 성공으로 재분류하지 않는다.
