# 등록 서비스 제출 문구 정책 v6

숫자 표기와 금지 문구의 2,200개 조합을 Lean 4.34.1, v5 및 v6에서 대조했다. 무해한 후속 선언 200건은 별도 대조다. 아래에는 로컬 문구 재검사와 Linux 회귀 절차를 구분해 적었다.

## 저장소 선택과 재현

새 catalog 이벤트에는 v6와 SHA-256 `999dc86304a54fd77bd1ff3dee7d967d7cb50a35ee4c053779f85c74bd8aba32`가 들어간다. 기존 v5 이벤트는 계속 v5 파일 `56f693d89a1a07751404e987457712bd4776f82aac921d6cf74bc329309aecc3`을 선택한다. descriptor 없는 과거 catalog는 v4로 해석한다. 각 제출 검사에서 기록된 버전과 파일 해시를 검증하며, 현재 작업자가 받는 job에는 store descriptor가 명시된다. descriptor 필드 자체가 없는 옛 worker job은 종전 기본값 v5를 쓴다. 과거 gate 지문으로 기록된 시도는 해당 지문에 등록한 기존 verifier로 재생한다. [생성, 검사 및 재생](../scripts/zk_registry/service.py) · [정책 읽기](../scripts/zk_registry/check.py) · [worker](../scripts/zk_registry/service_check.py)

v6 JSON은 왼쪽 경계, 두 기호 명령의 오른쪽 경계, 속성 앞과 목록 안의 주석 거부를 명시한다. v4·v5 고정 corpus는 각자의 정책에서 검사한다. [정책 파일](../scripts/zk_registry/data/submission_policy_v6.json) · [재생 시험](../scripts/test_zk_registry_det2b_review.py)

## 왼쪽 경계를 정한 근거

Lean 4.34.1의 `Lean/Parser/Basic.lean` 817–919행은 기수, 숫자 구분자, 소수점과 지수의 소비 순서를 정의한다. 921–943행과 975–1009행은 식별자 부분·점과 등록 토큰 길이의 우선순위를 정의하고, 1027–1046행은 숫자로 시작하면 먼저 숫자 파서를 호출한다. `Init/Meta/Defs.lean` 120–138행은 ASCII 식별자 첫 글자와 후속 글자를 나눈다. `Lean/Elab/Syntax.lean` 250–260행은 등록할 고정 토큰의 선두 숫자와 내부 공백을 제한한다. `Lean/Parser/Module.lean` 135–150행은 뒤따르는 명령을 파싱하는 위치다. 소스 기준은 동일한 4.34.1 툴체인이다.

v6은 이전에 왼쪽 경계였던 위치를 모두 유지한다. 새 판단이 필요할 때에만 원문에서 공백 또는 입력 시작점, 그리고 확인된 내장 숫자의 끝을 기준으로 ASCII 식별자 내부를 표시한다. 그 내부로 확정할 수 없는 위치에서는 금지어 검사를 적용한다. 숫자 자체가 잘못되거나 Unicode·사용자 등록 기호 때문에 경계가 불확실한 경우에도 적용한다. 따라서 Lean이 거부할 텍스트나 문장부호 뒤 일반 이름의 접미사에도 정책 진단이 나올 수 있다. 새 부록 시험은 기수별 끝, 일반 식별자, 점이 있는 이름과 원래 정책 데이터 보존을 별도로 확인한다. [숫자 끝과 식별자 처리](../scripts/zk_registry/check.py#L241)

## 오른쪽 경계와 속성 주석 거부

Lean 4.34.1의 `Lean/Parser/Basic.lean` 1027–1045행은 가장 긴 등록 기호를 찾는다. 943–957행은 기호 끝에서 곧바로 다음 토큰으로 진행하고, 1102–1116행의 `symbolNoAntiquot`은 파서 표기의 바깥 공백을 제거한다. `#`로 시작하는 `#eval`, `#exit`, `#print`, `#check`, `#reduce`, `#synth` 여섯 항목에 오른쪽 단어 경계 없이 리터럴 검사를 적용한다. 이 기준으로 `#eval`과 `#exit`의 규칙을 바꿨다. Lean의 토큰 표와 별개로 동작하는 보수적인 문구 검사이므로, 등록된 더 긴 기호가 같은 접두사로 시작할 때에도 검사한다.

식별자형 단어는 998–1009행의 `isIdRest` 소비를 근거로 ASCII 오른쪽 경계를 사용한다. 예를 들어 `macroX`는 식별자 하나다. `register_`, `declare_`, `debug.`는 정책상 접두사이고, `skipKernelTC`와 `addDeclWithoutChecking`은 부분 문자열 규칙이다. `@[`는 `Lean/Parser/Term.lean` 590행의 속성 목록 기호로 별도의 허용 목록을 적용한다. `«…»`는 984–997행의 이스케이프 식별자이며 이름 내부의 금지 문자를 따로 검사한다. [정책 표기](../scripts/zk_registry/data/submission_policy_v6.json)

### 속성 주석의 원문 검사

검토에서 등록된 보간 접두사와 같은 철자의 일반 함수가 있을 때 문자열 끝을 잘못 판단할 수 있었다. 명령의 귀속과 보간 목록을 추적하던 구현을 제거하고, 기존 v5 계열의 원문 속성 허용 목록 검사에 v6 거부 두 가지를 더했다.

1. 원문 `attribute` 뒤에 공백만 두고 `--` 또는 `/-`가 나오고 이후 원문에 `[`가 있으면 `attribute`의 줄을 `attribute-comment`로 보고한다. 첫 주석 시작 표식부터 다음 원문 여는 대괄호까지는 해석하지 않는다. 이 때문에 그 대괄호가 실제 속성 목록과 무관할 때에도 진단할 수 있다.
2. 원문 `@[` 또는 `attribute [`로 연 목록에 `--` 또는 `/-`가 나오면 목록 시작 줄을 같은 라벨로 보고한다. 허용 목록 검사와 같은 원문 괄호 스택을 사용하므로 내부의 짝이 닫혀도 바깥 목록은 열린 상태다. 목록의 괄호가 어긋나거나 입력 끝까지 닫히지 않으면 `forbidden-attribute`를 보고한다.

Lean은 `«…»`를 식별자 하나로 읽는다. v6은 원문의 이스케이프 이름 안에서 U+005B(`[`)와 U+005D(`]`)를 금지한다. 진단은 `forbidden-name-character`이며 이름이 시작한 줄을 가리킨다. 따라서 이스케이프 이름 안의 닫는 대괄호를 원문 목록 검사가 먼저 만나는 경우에도 제출은 이름 검사에서 거부된다. 괄호가 없는 일반 이름과 이스케이프 이름을 인자로 받는 `specialize` 대조는 순수 문구 검사를 통과했다. [v6 금지 문자](../scripts/zk_registry/data/submission_policy_v6.json) · [목록 검사](../scripts/zk_registry/check.py)

두 규칙에 문자열·주석 본문을 구분하는 토큰 처리는 없다. 문자열이나 주석에 인용된 `attribute`와 문서 주석도 원문 시작 표식으로 취급하므로 보수적인 오탐이 생길 수 있다. 실제 목록의 속성이 허용 목록에 있어도 주석 시작 표식이 있으면 거부한다. [원문 거부 구현](../scripts/zk_registry/check.py) · [회귀 시험](../scripts/test_zk_registry_det2b.py)

주석과 허용 목록 검사에 같은 순방향 순회와 스택을 사용한다. 앞선 줄과 후보는 진단 한도 안에서 보관한다. 원문 방문·인덱싱, 접두사·문자열 검색, 정규식 호출과 범위 및 파이썬 줄 이벤트를 계측했다. 공유 속성 검사에서 `find`와 `count`는 24개 입력 모두 0회였고, 왼쪽 경계와 목록 항목 정규식도 계측했다. 연속 최대 작업량 증가율은 2.0036배였다. 이 계측의 범위는 공유 속성 검사이며 정규식 엔진 내부 단계나 경과시간을 별도로 측정하지 않았다.

| 크기 | 목록 앞 | 여러 목록 | 대기 중인 목록 앞 후보 | 숫자 뒤 경계 | 한 목록에 조밀한 주석 | 깊은 중첩 후 주석 |
|---:|---:|---:|---:|---:|---:|---:|
| 64 | 29,534 | 32,906 | 17,551 | 41,636 | 17,509 | 4,024 |
| 128 | 59,166 | 65,930 | 34,512 | 83,364 | 34,661 | 7,352 |
| 256 | 118,430 | 131,978 | 68,432 | 166,820 | 68,965 | 14,008 |
| 512 | 236,958 | 264,074 | 136,272 | 333,732 | 137,573 | 27,320 |

[계측 도구](../scripts/measure_registry_policy_v6.py) · [작업량 JSON](../fixtures/submission-policy/v6-text-measurement.json)

### 속성 명령의 Lean 대조 계획

향후 Lean 표에는 일반 블록·줄 주석이 앞에 있는 형태, 목록 내부 주석, 주석 없는 허용 목록을 넣는다. 이 작업에서 측정한 값은 순수 문구 결과다. 아래 소스는 평범한 정의 `probe`에 적용하는 형태다.

| 입력 | v6 순수 문구 결과 |
|---|---|
| `attribute /- note -/ [noinline] probe` | `attribute-comment` |
| `attribute -- note` 뒤 다음 줄에 `[noinline] probe` | `attribute-comment` |
| `attribute [simp /- note -/] probe` | `attribute-comment` |
| `attribute   [simp] probe` | 진단 없음 |

## 무해한 후속 선언의 Lean 대조

동일한 40개 숫자 표기에 다섯 열을 적용했다. 줄바꿈 열은 숫자 자체의 대조군이다. 후속 선언과 그 이름 사용까지 오류 없이 통과해야 성공으로 셌다. 숫자 표기에는 불완전한 표기 다섯 가지와 BitVec 폭 표기·타입 주석 같은 복합 표현도 들어 있다. BitVec 폭은 `Init/Data/BitVec/Basic.lean`의 표기법이다.

| 열 | 선언까지 성립 | 구문 오류 |
|---|---:|---:|
| 줄바꿈 후 theorem | 35 | 5 |
| theorem | 34 | 6 |
| def | 28 | 12 |
| abbrev | 28 | 12 |
| end | 19 | 21 |
| 계 | 144 | 56 |

각 표기가 다섯 열에서 낸 성립·오류 수는 다음과 같다. 전체 메시지와 행·열 위치는 실측 JSON에 있다. [입력 40종](../fixtures/submission-policy/numerals-v6.json) · [셀별 원자료](../fixtures/submission-policy/lean-numerals-v6.json)

| 성립 | 오류 | 표기 ID |
|---:|---:|---|
| 5 | 0 | binary-lower, binary-upper, octal-lower, octal-upper, binary-separator, binary-prefix-separator, octal-separator, octal-prefix-separator, e-unsigned, E-unsigned, e-plus, E-plus, e-minus, E-minus, fraction-plus, fraction-minus, exponent-separator, bare-dot-exponent, typed-numeral |
| 4 | 1 | zero, decimal, decimal-leading-zero, decimal-separator, decimal-repeated-separator, fraction, fraction-separator, bitvec-width, bitvec-hex-width |
| 2 | 3 | hex-lower, hex-upper-digits, hex-upper-prefix, hex-upper, hex-separator, hex-prefix-separator |
| 1 | 4 | bare-dot |
| 0 | 5 | decimal-trailing-separator, hex-no-digits, binary-no-digits, octal-no-digits, exponent-no-digits |

Mac 재현 명령은 새 출력을 배타적으로 연다. 각 호출 전 hold가 false인지 확인하고 nice 19에서 `env -i`로 Lean을 한 번에 하나씩 실행한다. [실측 도구](../scripts/probe_lean_numerals.py)

```sh
cd "${V6_WORKTREE:?set the absolute worktree path}"
/usr/bin/nice -n 19 "${PYTHON312:?set the absolute Python 3.12 path}" -I -B scripts/probe_lean_numerals.py \
  --lean "${LEAN_4341:?set the pinned Lean binary path}" \
  --hold "${CHAIN_CALL_HOLD:?set the chain hold record path}" \
  --hold-schema "${CHAIN_CALL_HOLD_SCHEMA:?set the chain hold schema}" \
  --output "$PWD/lean-numerals-v6-recheck.json"
```

## 금지 문구 55종과의 Lean 대조

실측은 숫자 표기 40종과 금지 문구 55종의 조합 **2,200/2,200**을 Lean 4.34.1에서 확인했다. 실행 파일 SHA-256은 `1b370cfcbf44e80d1b004ab1b1ab9a4c73951f9f7c242140bcff9bc577576554`다. 저장 fixture는 숫자, 문구, 합성 입력의 형태, Lean 판정과 메시지 분류, v5·v6 판정을 담는다. 원문 메시지·trace와 호스트 경로, 실행 메타데이터는 제거했다. [추적 fixture](../fixtures/submission-policy/lean-v5-v6-table.json) · [핀된 문구 재검사](../scripts/test_zk_registry_text_policy_v6_lean_table.py)

명령 열은 선언식의 숫자 바로 뒤에 문구를 붙였다. tactic 여섯 열은 실행되지 않는 `first` 대안에서 파싱만 확인했다. 단독 명령을 시작하지 않는 속성명·옵션명·상수명은 붙인 경우와 구문 안에서 읽히는 별도 대조군을 구분했다. 입력에 import는 없다. 표의 v5·v6 열은 지정 라벨의 진단 여부다. Lean 판정은 trace에서 예상 구문과 그 시작 열이 일치할 때 해석으로 분류한다. 같은 셀에 앞선 구문 오류가 있어도 복구 뒤 해석이 확인되면 해석으로 센다.

`register_item`과 `declare_item`은 기존 접두사 문구 검사의 대조군으로 남겼다. 같은 접두사 규칙의 실제 명령 `register_tactic_tag`와 `declare_syntax_cat`은 별도 열에서 40개 숫자 모두와 대조했다.

표에는 저장된 메시지를 최종 규칙으로 다시 분류한 값이 들어 있다. 원시 출력 12셀의 초기 `classification.status`는 다르다. 머리 판정 보정 전의 `#reduce`·`@[not_allowlisted]` 10셀은 다른 구문에서 해석으로, 구문 오류 판정 보정 전의 16진 `dsimproc` 계열 2셀은 구문 오류에서 다른 구문으로 바뀌었다.

| 전체 셀 | Lean 해석 | 구문 오류 | 다른 구문 | v5 누락 | v6 검출 | 해석·v5 누락 | 해석·v6 누락 |
|---:|---:|---:|---:|---:|---:|---:|---:|
| 2,200 | 1,405 | 781 | 14 | 1,628 | 2,200 | 967 | **0** |

- Lean이 해석한 1,405개 셀 중 209개에는 구문 오류도 있었다. `0x#eval 7`은 불완전한 16진 리터럴에 `unexpected character`를 보고한 뒤 복구해 `7`을 출력했다.
- 16진 리터럴이 `dsimproc` 또는 `dsimproc_decl`의 선두 `d`를 흡수한 14개 셀은 시작 열이 이동해 `simproc` 또는 `simproc_decl`로 해석됐다. 표에는 다른 구문으로 분류했다. v6는 실제로 읽힌 명령 라벨도 14/14 검출했다.
- `declare_syntax_cat`을 붙인 40셀 중 33셀에서 명령을 해석했다. 나머지 16진 행 7셀에서는 리터럴이 첫 `d`를 흡수했고, Lean은 남은 토큰에 `unexpected identifier; expected command`를 보고했다. v6는 40셀 모두 검출했다.
- 본 측정 전 guard가 아직 없을 때 1.7초짜리 시간 확인 컴파일 한 건이 공유 잠금 없이 실행됐다. 이후 컴파일은 모두 guard를 거쳤다.

추적 fixture에는 표를 재검사하는 데 필요한 합성 입력, 메시지 분류와 판정만 담았다. 검사기와 두 정책 파일의 해시는 fixture에 기록했다.

### 숫자 표기별 집계

각 행은 55개 문구다.

| 숫자 표기 | Lean 해석 | 구문 오류 | 다른 구문 | v5 누락 | v6 검출 | 해석·v5 누락 | 해석·v6 누락 |
|---|---:|---:|---:|---:|---:|---:|---:|
| zero (`0`) | 34 | 21 | 0 | 44 | 55 | 25 | 0 |
| decimal (`17`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| decimal-leading-zero (`017`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| hex-lower (`0xaf`) | 26 | 27 | 2 | 44 | 55 | 17 | 0 |
| hex-upper-digits (`0xAF`) | 26 | 27 | 2 | 44 | 55 | 17 | 0 |
| hex-upper-prefix (`0Xaf`) | 26 | 27 | 2 | 44 | 55 | 17 | 0 |
| hex-upper (`0XAF`) | 26 | 27 | 2 | 44 | 55 | 17 | 0 |
| binary-lower (`0b101`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| binary-upper (`0B101`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| octal-lower (`0o71`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| octal-upper (`0O71`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| decimal-separator (`1_234`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| decimal-repeated-separator (`1__23`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| decimal-trailing-separator (`12_`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| hex-separator (`0xA_f`) | 26 | 27 | 2 | 44 | 55 | 17 | 0 |
| hex-prefix-separator (`0x_Af`) | 26 | 27 | 2 | 44 | 55 | 17 | 0 |
| binary-separator (`0b10_01`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| binary-prefix-separator (`0B_10`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| octal-separator (`0o7_1`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| octal-prefix-separator (`0O_17`) | 41 | 14 | 0 | 44 | 55 | 32 | 0 |
| hex-no-digits (`0x`) | 23 | 30 | 2 | 44 | 55 | 14 | 0 |
| binary-no-digits (`0b`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| octal-no-digits (`0o`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| fraction (`1.25`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| bare-dot (`1.`) | 35 | 20 | 0 | 0 | 55 | 0 | 0 |
| e-unsigned (`1e2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| E-unsigned (`1E2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| e-plus (`1e+2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| E-plus (`1E+2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| e-minus (`1e-2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| E-minus (`1E-2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| fraction-plus (`1.25e+2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| fraction-minus (`1.25E-2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| fraction-separator (`1.2_5`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| exponent-separator (`1e1_2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| bare-dot-exponent (`1.e+2`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| exponent-no-digits (`1e+`) | 35 | 20 | 0 | 0 | 55 | 0 | 0 |
| bitvec-width (`7#8`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| bitvec-hex-width (`0xaf#8`) | 35 | 20 | 0 | 44 | 55 | 26 | 0 |
| typed-numeral (`(17 : UInt8)`) | 35 | 20 | 0 | 0 | 55 | 0 | 0 |

### 금지 문구별 집계

각 열은 40개 숫자 표기다.

| 문구 | Lean 해석 | 구문 오류 | 다른 구문 | v5 누락 | v6 검출 | 해석·v5 누락 | 해석·v6 누락 |
|---|---:|---:|---:|---:|---:|---:|---:|
| `sorry` | 19 | 21 | 0 | 37 | 40 | 19 | 0 |
| `admit` | 13 | 27 | 0 | 37 | 40 | 13 | 0 |
| `axiom` | 33 | 7 | 0 | 37 | 40 | 30 | 0 |
| `native_decide` | 19 | 21 | 0 | 37 | 40 | 19 | 0 |
| `unsafe` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `implemented_by` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `extern` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `skipKernelTC` | 0 | 40 | 0 | 0 | 40 | 0 | 0 |
| `addDeclWithoutChecking` | 0 | 40 | 0 | 0 | 40 | 0 | 0 |
| `ofReduceBool` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `ofReduceNat` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `#exit` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `#eval` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `run_cmd` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `run_elab` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `run_meta` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `initialize` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `builtin_initialize` | 32 | 8 | 0 | 37 | 40 | 29 | 0 |
| `trustCompiler` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `elab` | 33 | 7 | 0 | 37 | 40 | 30 | 0 |
| `elab_rules` | 33 | 7 | 0 | 37 | 40 | 30 | 0 |
| `simproc` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `dsimproc` | 33 | 0 | 7 | 37 | 40 | 30 | 0 |
| `simproc_decl` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `dsimproc_decl` | 33 | 0 | 7 | 37 | 40 | 30 | 0 |
| `builtin_simproc` | 32 | 8 | 0 | 37 | 40 | 29 | 0 |
| `builtin_dsimproc` | 32 | 8 | 0 | 37 | 40 | 29 | 0 |
| `builtin_simproc_decl` | 32 | 8 | 0 | 37 | 40 | 29 | 0 |
| `builtin_dsimproc_decl` | 32 | 8 | 0 | 37 | 40 | 29 | 0 |
| `macro` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `macro_rules` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `syntax` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `run_tac` | 19 | 21 | 0 | 37 | 40 | 19 | 0 |
| `register_item` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `declare_item` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `maxHeartbeats` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `maxRecDepth` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `native` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `bv_decide` | 12 | 28 | 0 | 37 | 40 | 12 | 0 |
| `bv_check` | 12 | 28 | 0 | 37 | 40 | 12 | 0 |
| `addDecl` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `debug.item` | 0 | 40 | 0 | 37 | 40 | 0 | 0 |
| `#print` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `#check` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `#reduce` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `#synth` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `set_option maxHeartbeats 10` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `set_option maxRecDepth 10` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `set_option trace.probe true` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `set_option profiler.probe true` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `set_option diagnostics.probe true` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `attribute [not_allowlisted]` | 33 | 7 | 0 | 37 | 40 | 30 | 0 |
| `@[not_allowlisted]` | 40 | 0 | 0 | 0 | 40 | 0 | 0 |
| `register_tactic_tag` | 40 | 0 | 0 | 37 | 40 | 37 | 0 |
| `declare_syntax_cat` | 33 | 7 | 0 | 37 | 40 | 30 | 0 |

## 시험 범위와 기존 정상 입력

기준 대비 새 unittest 메서드는 기존 문구 회귀 12개와 추적 표 재검사 3개다. 순수 문구 측정에서 숫자 경계 **2,120/2,120**, 해시 기호 경계 **24/24**, 속성 주석 **14/14**, 이스케이프 이름·목록 괄호 **11/11**, 작업량 **24/24**가 기대와 일치했다. 기존 순수 문구 측정 JSON의 `text_matrix`는 40행 × 53열 결과다. Lean·v5·v6 대조는 위 추적 표에 있다. 이 커밋에서 추가한 세 시험은 핀된 두 문구 검사와 저장된 Lean 판정만 사용하며, Lean과 전체 등록 서비스 suite를 실행하지 않는다. [시험](../scripts/test_zk_registry_det2b.py) · [측정 JSON](../fixtures/submission-policy/v6-text-measurement.json)

### 기준판 대비 판정 변화

이스케이프 이름의 대괄호와 목록 깊이 검사를 추가하기 전과 비교한 공개 표본 **4,195건**에서 신규 거부는 **10건**, 신규 허용은 **0건**이다. 각 검사는 순수 원문 검사다.

| 바뀐 사례 | 새 진단 |
|---|---|
| `inline_escaped_close`, `command_escaped_close`, `escaped_open` | `forbidden-name-character` |
| `inline_nested_comment`, `command_nested_comment` | `attribute-comment` |
| `nested_mismatch` | `forbidden-attribute` |
| `nested_lists/64`, `nested_lists/128`, `nested_lists/256`, `nested_lists/512` | `attribute-comment` |

고정 corpus 206건을 포함한 나머지 4,185건의 판정과 진단은 동일했다. v5 정책 바이트와 고정 corpus 206건의 v5 진단도 일치했다. [개별 괄호 대조 및 작업량](../fixtures/submission-policy/v6-text-measurement.json)

### 앞선 주석 규칙의 판정 변화

속성 주석 검사를 추가하기 직전의 기준과 비교했다. 기준에는 `#eval`과 `#exit`의 경계를 `suffix`에서 `any`로 바꾼 규칙이 적용됐다. 중복 표본을 포함한 공개 표본 **4,180건 중 신규 거부 70건, 기존 거부에서 허용으로 바뀐 사례 0건**이었다. 모든 신규 거부의 새 라벨은 `attribute-comment`였다.

| 표본군 | 변한 모든 사례 |
|---|---|
| 종전 경계 33건 | `line/denied`, `line/allowed`, `block/denied`, `block/allowed`, `nested/denied`, `nested/allowed`, `shared-bracket` |
| 종전 귀속 13건 | `comment_before`, `comment_after`, `alternating`, `deep`, `comment_only`, `doc_token`, `raw_string`, `ordinary_string`, `interpolated_s`, `interpolated_f`, `interpolated_m`, `qualified_identifier` |
| 종전 보간 45건 | `unlisted-allowed` |
| 종전 작업량 12건 | `nested`, `alternating`, `interpolated` 각각 크기 64·128·256·512 |
| 종전 단위 시험 18건 | `block/not_allowlisted`, `block/simp`, `line/not_allowlisted`, `line/simp`, `line-contains-attribute/simp`, `nested/not_allowlisted`, `nested/simp`, `interruption/doc`, `interruption/module-doc`, `interruption/unterminated` |
| 새 속성 대조 14건 | `block_prefix`, `line_prefix`, `allowed_name_with_comment`, `command_list`, `inline_list`, `list_with_raw_bracket`, `line_opener_in_list`, `raw_string_text` |
| 새 작업량 20건 | `prefixes`, `lists`, `pending_prefixes`, `numeral_left_boundary`, `dense_list` 각각 크기 64·128·256·512 |

판정 변화가 0건인 나머지 표본군은 고정 corpus 206건, 추적 Lean fixture 4개, 숫자 경계 2,120건, 이름 1,692건, 별도의 `#eval` 대조 3건이다. v5 정책 바이트와 고정 corpus 206건의 v5 진단도 일치했다. VM에서 만들어지는 제출은 VM 회귀 범위다. [v4 고정 corpus](../fixtures/submission-policy/cases-v4.json) · [v5 고정 corpus](../fixtures/submission-policy/cases-v5.json)

## 일반 이름의 추가 오탐

Lean 4.34.1의 `Init/Data/{List,Option,Nat,Array}/Basic.lean` 네 파일에서 열 1로 시작하는 단일 행 선언 패턴으로 ASCII 이름을 추출했다. 파일별 고유 이름은 각각 96, 7, 261, 85개이고, 파일 간 중복을 제거하면 415개다. 각 이름을 합성 선언, 일반 참조, 괄호 안 참조, 괄호 바로 뒤 바인더에 한 번씩 넣어 같은 검사기에 v5와 v6 정책을 각각 선택했다. 별도 표본은 일반 단어와 정책 철자가 겹치는 경우를 살피려고 미리 고른 8개 이름이다.

| 위치 | 표준 라이브러리 이름 415개 중 추가 오탐 | 별도 이름 8개 중 추가 오탐 |
|---|---:|---:|
| 선언 이름 | 0 | 0 |
| 일반 참조 | 0 | 0 |
| 괄호 안 참조 | 0 | 5 |
| 괄호 바로 뒤 바인더 | 0 | 5 |
| 합계 | 0/1,660 | 10/32 |

예를 들어 `(alternative)`는 v6에서 `native-evaluation`, `(expand_macro)`는 `macro` 진단을 냈다. 둘 다 하나의 일반 이름을 참조하는 텍스트다. 같은 두 위치에서 `concrete_syntax`, `current_axiom`, `is_unsafe`도 추가 오탐이었다. 10/32는 접미사를 살피기 위해 고른 표본의 결과다. 실제 제출의 오탐률은 이 측정으로 추정할 수 없다.

추가로 직접 고른 일반 이름 **22개**를 다섯 문맥에서 비교했다. 표의 수는 v5에는 없고 v6에서 추가된 진단의 개수다. 이름을 골라 만든 표본이므로 실제 제출의 오탐률로 해석할 수 없다.

| 위치 | 추가 진단 수 |
|---|---:|
| 공백 뒤 참조 | 0/22 |
| 괄호 안 참조 | 17/22 |
| 여는 괄호 직후 바인더 | 17/22 |
| 목록 원소 | 17/22 |
| 쉼표 바로 뒤 | 17/22 |
| 합계 | 68/110 |

기존 순수 문구 측정을 새 파일 없이 재현하려면 다음 명령으로 고정 JSON과 출력 바이트를 비교한다. [입력 핀·표본·진단](../fixtures/submission-policy/v6-text-measurement.json)

```bash
set -o pipefail
cd "${V6_WORKTREE:?set the absolute worktree path}"
/usr/bin/nice -n 19 "${PYTHON312:?set the absolute Python 3.12 path}" -I -B scripts/measure_registry_policy_v6.py \
  --lean-source-root "${LEAN_4341_SOURCE:?set the Lean source path}" \
  --hold "${CHAIN_CALL_HOLD:?set the chain hold record path}" \
  --hold-schema "${CHAIN_CALL_HOLD_SCHEMA:?set the chain hold schema}" \
  | /usr/bin/cmp - fixtures/submission-policy/v6-text-measurement.json
```

## 전체 VM 회귀

오프라인 Linux 게스트에서 전체 프로세스 그룹에 시간 한도를 적용한다. 기준과 후보의 깨끗한 checkout에 동일한 기존 시험 도구와 Lean을 준비한다. `V6_REVIEW_COMMIT`에 검토한 후보 커밋을 넣는다. 스크립트는 각 checkout의 등록 서비스 시험과 정규 보조 모듈을 실행한 뒤 measuring-stick descriptor를 검사한다. [Linux 진입점](../scripts/run_registry_policy_suite.sh) · [정규 스위트의 대상](../scripts/self-test.sh#L62)

```bash
# Provisioned Linux test VM, under its normal non-root test account with sudo for systemd-run.
set -euo pipefail
vm_root=${V6_VM_ROOT:?set the absolute VM staging path}
candidate="$vm_root/candidate"
base_sha=46582051b9d15bcc6b5049077f46f407664929c9
candidate_sha=${V6_REVIEW_COMMIT:?set the reviewed 40-digit branch commit}
[[ "$candidate_sha" =~ ^[0-9a-f]{40}$ ]]
git -C "$candidate" merge-base --is-ancestor "$base_sha" "$candidate_sha"
test "$(git -C "$candidate" rev-parse HEAD)" = "$candidate_sha"
test -z "$(git -C "$candidate" status --porcelain=v1 --untracked-files=all)"
sudo -n systemd-run --wait --pipe --collect --unit=registry-text-policy-v6-regression \
  --uid="$(id -un)" --gid="$(id -gn)" --property=Delegate=yes \
  --property=PrivateNetwork=yes --property=KillMode=control-group \
  --property=RuntimeMaxSec=3600 --property=TimeoutStopSec=15 --property=SendSIGKILL=yes \
  -- /bin/bash -c '
    test "$(id -u)" != 0 || exit 2
    status=0
    /bin/bash "$1/candidate/scripts/run_registry_policy_suite.sh" "$1/base" "$2" || status=1
    /bin/bash "$1/candidate/scripts/run_registry_policy_suite.sh" "$1/candidate" "$3" || status=1
    exit "$status"
  ' _ "$vm_root" "$base_sha" "$candidate_sha"
```

예상 경과시간은 checkout당 15–30분, 두 판 합계 30–60분이다. 이 수치는 전체 VM 스위트의 추정치이며 준비 시간은 별도다. 게스트의 `-v` 출력에서 양쪽의 동일 시험 ID와 성공·건너뜀·실패를 대조하고, 신규 15개가 실행됐는지 확인한다. 중단, 필수 시험의 건너뜀, 새 실패, 측정 누락은 결과에 명시한다.
