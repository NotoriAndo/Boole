# 내 Mac에서 Boole 따라 해 보기

대상: Apple Silicon Mac에서 **준비된 개발용 테스트 번들**로 설치 → 실행 →
고정 답안 판정 → 재시작 → 제거를 확인하려는 사용자.

현재 상태: **가이드 작성 완료 / 이 가이드 그대로의 새 실기 실행은 미검증**.
공개 서명 릴리스가 없으므로 아직 누구나 바로 실행하는 원라인 설치는 아니다.
아래 준비물을 받은 뒤에만 1단계부터 진행한다. 준비물이 없다면 여기서 멈추고
“이 가이드에 맞는 Mac 테스트 번들을 준비해 줘”라고 요청하면 된다.

이 과정은 기존 개발용 Mac의 로컬 체험이다. Mac 초기화는 필요 없으며,
깨끗한 일반 사용자 Mac 설치 검증(CURL.3) 통과로 기록하지 않는다.
새 모델 호출·API 키·공개 채굴·실제 보상·지갑 자금 이동은 없다.

## 0. 준비물 확인

터미널에서 아래 읽기 전용 명령을 실행한다.

```sh
uname -m
sw_vers -productVersion
command -v python3
command -v curl
lsof -nP -iTCP:8082 -sTCP:LISTEN
lsof -nP -iTCP:18084 -sTCP:LISTEN
```

`arm64`여야 한다. macOS 버전이 번들 제작자가 검증한 버전인지 확인한다.
Python 3와 curl 경로가 있어야 하고, 두 포트의 조회 결과는 비어 있어야 한다.
포트가 사용 중이면 기존 프로세스를 임의로 종료하지 말고 진행을 멈춘다.

제작자에게 다음 세 가지 **절대 경로**를 받는다.

| 준비물 | 받아야 하는 내용 |
|---|---|
| `BUNDLE` | 로컬 HTTP로 제공할 완성된 테스트 배포 폴더. 서명 메타데이터, Mac arm64 호스트 4종, 부팅 가능한 Linux 게스트 11종 및 올바른 전송 경로를 포함 |
| `CLI` | 해당 번들의 `host-cli`와 동일한 신뢰 가능한 `boole` 실행 파일 |
| `ROOTS` | 제작자에게 별도로 확인한 개발용 공개 신뢰 루트 JSON. 비밀키가 아님 |

이 가이드의 추가 인계 규약으로 `BUNDLE/accepted-request.json`도 받아야 한다.
이는 현재 공개 배포 파일명이 아니라 **제작자가 준비해야 하는 요청 파일명**이다.
번들에 설치되는 고정 replay grant의 accepted 사례와 정확히 일치해야 하며,
사용자는 답안이나 epoch를 편집하지 않는다. 제작자는 파일 해시·소스 커밋·검증한
macOS 버전·필요 여유 공간도 함께 제공한다. 서명 검사를 끄거나 격리 속성을
강제로 제거하는 방식으로 준비 부족을 우회하지 않는다.

`ROOTS`의 필드는 `productKeyId`, `productPublicKeyHex`, `guestKeyId`,
`guestPublicKeyHex`다. 다운로드 서버가 알려 준 키만 믿는 구조가 아니다.
여기서 쓰는 직접 루트 지정은 **개발 테스트 전용**이며 운영 키 배포 절차를
대신하지 않는다. 기존 공개 `install.sh`는 소스/개발 도구 설치용이라 실행하지 않는다.

## 1. 설치

터미널 창을 두 개 연다. 아래 `/실제/...` 경로를 전달받은 경로로 바꾼다.

**터미널 A — 번들 서버.** 이후 이 창은 그대로 둔다.

```sh
BUNDLE='/실제/테스트-배포-폴더'
python3 -m http.server 18084 --bind 127.0.0.1 --directory "$BUNDLE"
```

**터미널 B — 설치와 확인.** 이후 명령은 모두 같은 B 창에서 실행한다.
먼저 `zsh`를 실행하여 체험용 셸을 열고, 다음 블록을 붙여 넣는다.

```sh
zsh
```

```sh
set -eu
umask 077
BUNDLE='/실제/테스트-배포-폴더'
CLI='/실제/boole'
ROOTS='/실제/별도확인한-공개루트.json'
test -d "$BUNDLE"
test -x "$CLI"
test -f "$ROOTS"
test -f "$BUNDLE/accepted-request.json"

root_value() {
  python3 -c 'import json,sys; print(json.load(open(sys.argv[1]))[sys.argv[2]])' "$ROOTS" "$1"
}
trust_flags=(
  --product-trust-root-key-id "$(root_value productKeyId)"
  --product-trust-root-public-key "$(root_value productPublicKeyHex)"
  --guest-trust-root-key-id "$(root_value guestKeyId)"
  --guest-trust-root-public-key "$(root_value guestPublicKeyHex)"
)
TRIAL=$(mktemp -d "${TMPDIR:-/tmp}/boole-mac-tryout.XXXXXX")
printf '이번 체험 폴더: %s\n' "$TRIAL"

"$CLI" product install-direct-boot \
  --base-url http://127.0.0.1:18084 \
  --install-root "$TRIAL/install" \
  --download-staging "$TRIAL/download" \
  --first-product-minimum 1 --first-guest-minimum 1 \
  "${trust_flags[@]}" > "$TRIAL/install.json"
python3 -m json.tool "$TRIAL/install.json"
```

정상: `ok: true`, `command: "product.install-direct-boot"`.
오류가 나면 다음 단계로 넘어가지 않는다. 출력된 체험 폴더 경로를 보관한다.
기존 `~/Library/Application Support/Boole`은 설치·상태 경로로 사용하지 않는다.

## 2. 실행하고 고정 답안 영수증 받기

B에서 실행한다. 로그는 체험 폴더에 남고 명령 프롬프트는 돌아온다.

```sh
"$CLI" product run-direct-boot \
  --install-root "$TRIAL/install" --state-root "$TRIAL/state" \
  "${trust_flags[@]}" > "$TRIAL/node.log" 2>&1 &
NODE_PID=$!
printf '이번 노드 PID: %s\n' "$NODE_PID"
```

다음 상태 확인을 실행한다. 부팅 중이면 잠시 뒤 같은 **상태 조회만** 다시 한다.
3분이 지나도 준비되지 않으면 답안을 제출하지 말고 로그를 확인한다.

```sh
if "$CLI" product status-direct-boot --timeout-seconds 2 > "$TRIAL/status.json"; then
  python3 -m json.tool "$TRIAL/status.json"
else
  printf '아직 준비되지 않았습니다. node.log를 확인하세요.\n'
fi
```

정상 조건은 모두 참이어야 한다: `ok`, `result.live.live`, `result.ready.ready`.
그때만 아래 제출 블록을 **한 번** 실행한다.

```sh
HTTP_CODE=$(curl --silent --show-error --max-time 120 \
  --output "$TRIAL/receipt.json" --write-out '%{http_code}' \
  -H 'Content-Type: application/json' \
  --data-binary "@$BUNDLE/accepted-request.json" \
  http://127.0.0.1:8082/native-shadow/submissions)
printf 'HTTP %s\n' "$HTTP_CODE"
python3 -m json.tool "$TRIAL/receipt.json"
```

정상: HTTP `200`, `outcome: "accepted"`, `reasonCode: "accepted"`.
`receipt.json`은 판정 응답 원문이다. 기존 고정 답안을 검증했다는 뜻이지,
새 문제를 모델이 풀었거나 실제 블록·보상이 발생했다는 뜻은 아니다.
타임아웃·거절·다른 응답이면 자동 재제출하지 않고 로그와 응답을 보존한다.

## 3. 재시작 확인

B에서 자신이 시작한 노드에만 정상 종료 신호를 보낸다.

```sh
kill -TERM "$NODE_PID"
wait "$NODE_PID"
```

정상 종료 후 같은 설치·상태 경로로 다시 실행한다.

```sh
"$CLI" product run-direct-boot \
  --install-root "$TRIAL/install" --state-root "$TRIAL/state" \
  "${trust_flags[@]}" > "$TRIAL/node-restart.log" 2>&1 &
NODE_PID=$!
```

2단계의 **상태 확인 블록만** 다시 실행하여 live/ready를 확인한다.
답안 제출은 반복하지 않는다. 이 간단한 재시작 확인만으로 crash recovery나
checker exactly-once의 새로운 실기 검증까지 통과했다고 주장하지 않는다.

## 4. 종료하고 제거

B에서 종료한다.

```sh
kill -TERM "$NODE_PID"
wait "$NODE_PID"
open "$TRIAL"
```

A에서는 `Ctrl+C`로 번들 서버를 종료한다. 노드 종료가 멈추거나 오류가 나면
폴더를 삭제하지 말고 로그와 PID를 전달한다. 무작정 다른 프로세스를 종료하지 않는다.

Finder에서 `install.json`, `status.json`, `receipt.json`, `node*.log`를
필요하면 다른 폴더에 복사한다. 그다음 **이번에 출력된 `boole-mac-tryout.*`
폴더 하나만** 휴지통으로 옮긴다. 휴지통을 비우기 전에는 복원 가능하다.
이 폴더의 로컬 테스트 상태도 함께 제거되므로 로그부터 보관한다.
받아 둔 번들·CLI·공개 루트와 기존 Boole 데이터는 건드리지 않는다.
`reset-direct-boot`는 전체 제거 명령이 아니므로 이 절차에서 사용하지 않는다.

## 막혔을 때 전달할 내용

멈춘 단계, macOS 버전, 번들 소스 커밋, 터미널 오류, 위 로그/응답 파일을 전달한다.
개인 경로 등은 필요한 만큼 가리고 비밀키·토큰·전체 환경 변수는 보내지 않는다.

제작자 참고: 실제 설치/상태/요청 계약은
[`native_shadow_installed_mac_e2e_v1.py`](../scripts/native_shadow_installed_mac_e2e_v1.py),
재시작·재전달의 상세 검증은
[`native_shadow_installed_mac_crash_restart_e2e_v1.py`](../scripts/native_shadow_installed_mac_crash_restart_e2e_v1.py)를 따른다.
`accepted-request.json`의 필드는 `schema`, `familyVersion`, `templateId`,
`challengeSha256`, `epoch`, `rawAnswer`이며, 위 E2E의 accepted payload와 동일하게
생성한다. 기존 E2E의 KAT 메타데이터 생성만으로 payload 전송 폴더가 완성되는 것은
아니다. `materialize_transport_layout`에 해당하는 호스트/게스트 배치까지 필요하다.
이 문서 작성은 번들 제작·VM 실행·공개 배포 완료를 의미하지 않는다.

전체 설치 계약은 [설치 문서](install.md), 현재 한계는
[현재 개발 위치](current-development-status.md)를 참고한다.
