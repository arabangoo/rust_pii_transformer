# rust_pii_transformer

> 한국어 텍스트에서 개인정보(PII, Personally Identifiable Information)를 **결정적으로** 탐지하고,
> **원문 복원이 보장되는** 마스킹을 수행하는 **모델 없는 순수 Rust** 라이브러리.

모델 파일 0개, GPU 0, 외부 API 호출 0, 네트워크 접근 0. 설치 후 폐쇄망에서 그대로 돈다.
기본 빌드의 의존성은 `thiserror` 와 `serde` 둘뿐이다.

**핵심 차별점은 변형 표기 인식이다.** 순수 숫자열뿐 아니라 한글 수사 표기(`팔팔공일공일`),
구분자와 띄어쓰기 변형(`880101 - 1234567`), 전각 표기(`８８０１０１`), 자모 분리형 한글,
숫자를 닮은 영문자(`88O1O1`), 이미 가려진 값(`880101-1******`), 음성 전사의 말끝
(`구공오삼일삼이래요`)을 정규화 층에서 흡수한 뒤 탐지하고, 찾은 구간을 **원문 좌표로 정확히
되돌린다.**

**한국어 텍스트 전용이다.** 엔티티가 한국 제도의 식별번호이고 문맥 단서 사전이 한국어 낱말로 되어
있다. 좁은 것이 목적이다. 기존 도구가 영어권에서 이미 잘하는 일을 따라가는 대신 **그 도구들이
한국어에서 못 하는 것**을 한다. 다만 쓰는 사람까지 한국어 사용자일 필요는 없다. API 식별자와 JSON
이름은 전부 영문이고 마스킹 자리표시자도 영문으로 고를 수 있다.

## 실측 정확도

합성 코퍼스 3,160건(양성 2,680 / 음성 480)에 대한 실측이다. 씨앗이 고정이라 누구든 같은 수를 얻는다.

| 축 | 값 |
| --- | --- |
| 전체 재현율 | **99.9%** (TP 2676 / FN 4) |
| 전체 정밀도 | **99.3%** (FP 20) |
| 마스킹 원문 복원 실패 | **0건** (3,160건 전량 왕복) |
| 음성 표본 오탐률 | 3.3% (480건 중 16건) |

표기 변형별로는 한글 수사·전각·공백·유사문자·부분마스킹·말끝이 100%, 숫자만 99.8%, 하이픈 99.4%다.
엔티티별로는 계좌번호가 95.0%, 나머지 아홉 종이 100%다. 재현 방법은
[16. 빌드와 테스트](#16-빌드와-테스트)에 있다.

```bash
cargo test --test accuracy -- --nocapture
```

## 구성

| 계층 | 하는 일 |
| --- | --- |
| 오프셋 매핑 (`span`) | 정규화 전후를 잇는 단조 구간 정렬 테이블 |
| 정규화 (`normalize`) | 자모 조합, 전각 폴딩, 한글 수사 역변환, 유사문자 교정, 구분자 흡수 |
| 탐지 (`detect`) | 검증식, 문맥 점수, 판정 근거 |
| 마스킹 (`mask`) | 정책 4종과 복원 맵 |
| 합성 검증 코퍼스 (`synth`) | 검증식이 유효한 표본과 변형 표기 생성 |
| 명령줄 도구 (`rpit`) | `--features cli` |
| Python 바인딩 | 네 층 전부 노출. `--features python` |

---

## 목차

1. [핵심 특징](#1-핵심-특징)
2. [빠른 시작](#2-빠른-시작)
3. [설치와 Cargo Feature](#3-설치와-cargo-feature)
4. [아키텍처](#4-아키텍처)
5. [공통 타입 참조](#5-공통-타입-참조)
6. [공개 API 참조](#6-공개-api-참조)
7. [엔티티별 동작과 성숙도](#7-엔티티별-동작과-성숙도)
8. [정규화 동작](#8-정규화-동작)
9. [판정 규칙과 설정](#9-판정-규칙과-설정)
10. [마스킹](#10-마스킹)
11. [합성 검증 코퍼스](#11-합성-검증-코퍼스)
12. [명령줄 도구 rpit](#12-명령줄-도구-rpit)
13. [성능](#13-성능)
14. [Python 바인딩](#14-python-바인딩)
15. [알려진 한계](#15-알려진-한계)
16. [빌드와 테스트](#16-빌드와-테스트)
17. [라이선스](#17-라이선스)

---

## 1. 핵심 특징

| 특징 | 의미 |
| --- | --- |
| **변형 표기 인식** | 한글 수사 표기, 구분자와 띄어쓰기 변형, 전각 표기, 자모 분리형 한글, 숫자를 닮은 영문자, 이미 가려진 값, 음성 전사의 말끝을 정규화 층에서 흡수한다 |
| **원문 복원 보장** | 정규화와 동시에 역방향 구간 정렬 테이블을 만든다. 탐지 스팬은 항상 원문 바이트 구간으로 정확히 되돌아간다 |
| **판정 근거 출력** | 어떤 규칙이 걸렸고, 어떤 검증식을 통과했고, 어떤 문맥 단서가 잡혔고, 어떤 정규화가 적용됐는지를 함께 낸다 |
| **미탐 사유 출력** | 형식은 맞았는데 떨어진 후보를 사유와 함께 낸다. "왜 안 걸렸는가"를 조사할 수 있다 |
| **모델 없음** | 모델 파일 0, GPU 0, 외부 API 0, 네트워크 0 |
| **결정적** | 같은 입력이면 항상 같은 출력. 캐싱, 테스트, 감사 추적이 성립한다 |
| **자기완결** | 순수 Rust. 외부 함수 인터페이스(FFI) 0, 하위 프로세스 호출 0 |

---

## 2. 빠른 시작

### 탐지

```rust
use rust_pii_transformer::detect::{detect, Config, Certainty, EntityKind};

let text = "주민등록번호 팔팔공일공일 - 1234567 입니다";
let report = detect(text, &Config::default()).unwrap();

let finding = &report.findings[0];
assert_eq!(finding.entity, EntityKind::Resident);

// 한글 수사로 적힌 앞자리까지 원문 구간으로 정확히 되돌아온다.
assert_eq!(finding.source.slice(text), "팔팔공일공일 - 1234567");

// 판정 근거가 함께 나온다.
assert_eq!(finding.evidence.cost.expanded_syllables, 6);   // 숫자로 펼친 한글 음절
assert_eq!(finding.evidence.cost.absorbed_whitespace, 2);  // 흡수한 공백
assert!(finding.certainty >= Certainty::Probable);

// 왜 안 걸렸는지도 함께 낸다.
for rejection in &report.rejections {
    println!("{:?} {:?} {:.2}", rejection.entity, rejection.reason, rejection.score);
}
```

### 정규화만 쓰기

탐지 없이 정규화 층만 쓸 수 있다. 다른 탐지기의 전처리로 붙이는 용도다.

```rust
use rust_pii_transformer::{normalize, NormalizeConfig, Span};

let out = normalize("팔팔공일공일 - 1234567", &NormalizeConfig::default()).unwrap();
assert_eq!(out.text, "8801011234567");

// 정규화문 스팬을 원문 좌표로 되돌린다.
// 한글 6음절 18바이트 + 구분자 3바이트 + 숫자 7바이트 = 28바이트.
let src = out.map.to_source(&Span::new(0..13, 0..13));
assert_eq!(src.span.byte, 0..28);
assert!(src.rules.contains(&"hangul.digit_reading"));
```

### 실행 가능한 예제

```bash
cargo run --example demo
```

상담 기록 형태의 문서에서 무엇이 잡히고 무엇이 안 잡히는지, 그리고 떨어진 후보의 사유까지 출력한다.

---

## 3. 설치와 Cargo Feature

아직 게시 전이므로 소스에서 받는다.

```toml
[dependencies]
rust_pii_transformer = { git = "https://github.com/arabangoo/rust_pii_transformer" }
```

```toml
[features]
default = []                             # 코어. 순수 Rust, FFI 0
hash    = ["dep:sha2", "dep:hmac"]       # 해시 가명화 마스킹 정책
cli     = ["dep:clap", "dep:serde_json"] # 명령줄 도구 rpit
python  = ["dep:pyo3", "dep:serde_json"] # PyO3 abi3 확장 모듈
```

**코어는 오프셋 매핑, 정규화, 탐지, 마스킹, 합성 검증 코퍼스 전부를 포함한다.** 셋만 옵트인이다.

| 의존성 | 조건 | 용도 |
| --- | --- | --- |
| `thiserror` 2 | 항상 | 에러 타입 |
| `serde` 1 | 항상 | 판정 결과 직렬화 |
| `sha2` 0.10, `hmac` 0.12 | `hash` | 해시 가명화 정책 |
| `clap` 4 | `cli` | 명령줄 인자 파싱 |
| `pyo3` 0.29 | `python` | Python 확장 모듈 |
| `serde_json` 1 | `cli`, `python` | JSON 내보내기 |

합성 검증 코퍼스에 난수 크레이트를 쓰지 않고 선형 합동 생성기를 직접 둔 이유가 있다. 코퍼스는
**재현 가능해야** 하고, 그러려면 씨앗을 고정한 결정적 생성기가 필요하다. 통계적 난수 품질이
필요한 용도가 아니다. 덕분에 이 기능은 의존성 없이 기본 빌드에 들어간다.

해시 가명화만 `hash` 로 뺀 것은 기본 빌드의 의존성 두 개 약속을 지키기 위해서다. RustCrypto 는
순수 Rust 라 FFI 는 여전히 0 이지만, 직접 구현하지 않고 검증된 것을 쓰되 켠 사람만 지불하게 했다.

### 최소 Rust 버전

기본 빌드는 **1.71** 이면 된다(`thiserror` 2 가 상한). `--features python` 은 PyO3 0.29 때문에
**1.83** 이 필요하다. 옵트인 경로 하나 때문에 코어 소비자 전체를 올리지 않으려고
`package.rust-version` 은 1.71 로 둔다.

---

## 4. 아키텍처

3층 파이프라인이다. 각 층은 독립적으로 테스트 가능하고 개별로 쓸 수 있다.

```text
원문 텍스트
   |
   v
[1층] 정규화  nfc -> fold -> hangul -> separator
   |  출력 2개: 정규화문, SpanMap(역방향 구간 정렬 테이블)
   v
[2층] 탐지    숫자 런 스캔 -> 엔티티 후보 -> 검증식 -> 문맥 점수 -> 등급 산정
   |  출력: Finding 목록 + Rejection 목록
   |  SpanMap.to_source() 로 원문 스팬 복원 (경계 스냅 포함)
   v
[3층] 마스킹  정책 적용 (전체 치환 / 부분 노출 / 해시 / 토큰화)
   |  출력: 가려진 텍스트 + 복원 맵(토큰화일 때)
   v
원문과 바이트 단위로 같은 바깥 + 가려진 구간
```

핵심은 **1층과 2층 사이에 `SpanMap` 이 끼어 있다는 점**이다. 탐지는 깨끗한 정규화문 위에서 하고
마스킹은 원문 위에서 한다. 그 둘을 잇는 것이 `SpanMap` 하나뿐이라 **복원 정확성의 책임이 한 곳에
모인다.**

### 오프셋 매핑이 푸는 문제

정규화는 문자를 지우고(하이픈 흡수), 바꾸고(전각을 반각으로), 늘리고(`일억` 이 숫자 9자리로),
줄인다(`구십일` 이 숫자 2자리로). 그래서 원문과 정규화문 사이에 1대1 대응이 성립하지 않는다.
그러나 **순서는 항상 보존된다.** 이 단조성 위에 세그먼트 테이블을 세운다.

문자별 인덱스 배열 대신 세그먼트 테이블을 쓰는 이유는 셋이다. 항등 구간을 하나로 접으므로 실텍스트에서
항목 수가 수십 개 수준이고, 이진 탐색으로 조회하며, **어떤 규칙이 이 구간을 만들었는지**를 함께 담을 수
있다. 판정 근거 출력이 여기서 공짜로 나온다.

### 바이트와 문자 오프셋을 둘 다 저장한다

Rust 문자열 슬라이싱은 바이트 인덱스를 쓰고, Python 과 JavaScript 소비자는 문자 인덱스를 쓴다.
한국어는 음절 1자가 UTF-8 로 3바이트라 둘이 항상 다르다. 나중에 변환하면 매번 O(n) 스캔이 붙고
그 변환이 새로운 오프셋 버그의 출처가 되므로, 세그먼트에 처음부터 함께 넣는다.

### 검증 하네스

네 개의 불변식으로 정확성을 강제한다.

| 불변식 | 내용 | 상태 |
| --- | --- | --- |
| **피복** | 모든 오프셋이 정확히 하나의 세그먼트에 속한다. 세그먼트를 이어 붙이면 양쪽 텍스트가 바이트 단위로 복원된다 | 검증됨 |
| **합성 결합법칙** | `compose(compose(a, b), c)` 와 `compose(a, compose(b, c))` 가 같다 | 검증됨 |
| **왕복** | 탐지 스팬을 원문으로 되돌린 뒤 그 조각을 다시 정규화하면 원래 구간이 나온다 | 부분 검증. [15절](#15-알려진-한계) 참조 |
| **무손실 마스킹** | 마스킹 결과에서 탐지 스팬 바깥의 바이트가 원문과 완전히 같다 | 미검증(마스킹 층 없음) |

무작위 테스트는 고정 시드 선형 합동 생성기로 400라운드를 돌린다. 외부 속성 테스트 크레이트를 쓰지
않은 것은 의존성 0 원칙을 개발 의존성에도 적용했기 때문이고, 시드가 고정이라 실패가 항상 재현된다.

---

## 5. 공통 타입 참조

### 구간과 매핑

```rust
pub struct Span {
    pub byte: Range<u32>,   // Rust 슬라이싱용
    pub char: Range<u32>,   // Python, JavaScript 소비자용
}

pub struct SourceSpan {
    pub span: Span,                  // 원문 기준 스팬
    pub snapped: bool,               // 세그먼트 경계 바깥으로 넓혀졌는가
    pub rules: Vec<&'static str>,    // 적용된 정규화 규칙. 이름 오름차순
    pub cost: NormalizationCost,     // 신뢰도 감점 근거
}

pub struct NormalizationCost {
    pub absorbed_whitespace: u16,    // 흡수한 공백 수. 가장 위험하다
    pub absorbed_separators: u16,    // 흡수한 구분자 수
    pub expanded_syllables: u16,     // 숫자로 펼친 한글 음절 수
    pub replaced_chars: u16,         // 치환한 문자 수
}

pub struct Segment {
    pub src: Span,                   // 원문 쪽 구간
    pub dst: Span,                   // 정규화문 쪽 구간
    pub kind: SegmentKind,           // Identity | Replace | Delete | Expand | Insert
    pub rules: Vec<&'static str>,
    pub cost: NormalizationCost,
}
```

오프셋을 `u32` 로 둔 이유는 둘이다. 4기가바이트를 넘는 단일 텍스트는 이 라이브러리의 용도가 아니고,
세그먼트 하나가 오프셋을 8개 들고 다니므로 절반 크기가 그대로 메모리 절감이 된다.

`rules` 는 적용 순서가 아니라 **이름 오름차순**으로 정규화된다. 순서를 그대로 두면 파이프라인을 어떻게
조립했느냐에 따라 출력이 달라져 결정성이 깨진다. 판정 근거로 필요한 것은 "어떤 규칙들이 관여했는가"라는
집합이지 순서가 아니다.

### 탐지 결과

```rust
pub struct Report {
    pub findings: Vec<Finding>,        // 통과한 결과. 원문 위치 오름차순
    pub rejections: Vec<Rejection>,    // 떨어진 후보와 사유
    pub normalized_text: String,       // 근거 확인용
}

pub struct Finding {
    pub entity: EntityKind,
    pub source: Span,          // 원문 기준. 마스킹은 이 구간에 적용된다
    pub normalized: Span,      // 정규화문 기준
    pub certainty: Certainty,  // Possible < Probable < Certain
    pub score: f32,
    pub evidence: Evidence,
}

pub struct Evidence {
    pub rule: &'static str,                  // 걸린 패턴 규칙 이름
    pub checksum: ChecksumResult,            // Passed | Failed | NotApplicable(사유)
    pub context_hits: Vec<ContextHit>,       // 잡힌 단서와 거리
    pub normalizations: Vec<&'static str>,   // 적용된 정규화 규칙
    pub cost: NormalizationCost,             // 감점 근거
    pub snapped: bool,
}

pub struct ContextHit {
    pub cue: &'static str,   // 잡힌 단어
    pub distance: u32,       // 스팬 경계로부터의 문자 거리
    pub weight: f32,         // 거리 감쇠까지 반영한 무게
}

pub struct Rejection {
    pub entity: EntityKind,
    pub source: Span,
    pub reason: RejectReason,  // ChecksumFailed | NoContext | BelowThreshold | Outranked | BusinessContext
    pub score: f32,
}
```

`EntityKind` 는 `Resident`, `ForeignerRegistration`, `BusinessRegistration`, `CreditCard`,
`BankAccount`, `Phone`, `Email`, `DriverLicense`, `BirthDate`, `Passport` 열 종이다. `label()` 이
한국어 이름을 낸다.

`Finding` 은 `serde` 직렬화를 지원하므로 그대로 JSON 으로 낼 수 있다. 단 **역직렬화는 지원하지
않는다.** 규칙 식별자를 `&'static str` 로 두어 힙 할당을 없앴는데 그것을 역직렬화하려면 입력 수명이
`'static` 이어야 하고 그것을 만족시킬 방법이 없다. 이 라이브러리의 용도는 판정 결과를 내보내는
것이지 되읽는 것이 아니다.

---

## 6. 공개 API 참조

### 탐지 (`detect`)

```rust
pub fn detect(text: &str, cfg: &Config) -> Result<Report>;

pub struct Config {
    pub normalize: NormalizeConfig,
    pub context_window: u32,   // 문맥 단서를 찾을 창 크기(문자). 기본 24
    pub min_score: f32,        // 이 점수 미만이면 결과로 내지 않는다. 기본 0.5
    pub min_context: f32,      // 문맥 필수 엔티티의 최소 문맥 총점. 기본 0.3
    pub min_veto: f32,         // 부정 문맥이 후보를 버릴 수 있게 되는 최소 총점. 기본 0.5
    pub weights: Weights,
}
```

### 정규화 (`normalize`)

```rust
pub fn normalize(text: &str, cfg: &NormalizeConfig) -> Result<Normalized>;

pub struct Normalized {
    pub text: String,
    pub map: SpanMap,
}

pub struct NormalizeConfig {
    pub nfc: bool,             // 한글 자모 조합
    pub fold: bool,            // 전각 폴딩
    pub hangul: bool,          // 한글 수사 역변환
    pub lookalike: bool,       // 유사문자 교정
    pub separator: bool,       // 구분자 흡수
    pub numeral: NumeralConfig,
}

impl NormalizeConfig {
    /// 문맥에 의존하지 않는 패스(nfc, fold)만 켠 설정.
    pub fn context_free() -> Self;
}
```

패스를 개별로 끌 수 있다. 소수점이 많은 회계 문서에서는 `separator` 패스가 `1.5` 를 `15` 로 만들고
그것이 오탐의 출처가 되므로, 그럴 때 그 패스만 꺼서 쓴다.

### 오프셋 매핑 (`span`)

```rust
impl SpanMap {
    pub fn identity(text: &str) -> Self;
    pub fn compose(inner: &SpanMap, outer: &SpanMap) -> Result<SpanMap>;
    pub fn to_source(&self, dst: &Span) -> SourceSpan;
    pub fn validate(&self) -> Result<()>;
    pub fn segments(&self) -> &[Segment];
    pub fn is_identity(&self) -> bool;
}

impl SpanMapBuilder {
    pub fn keep(&mut self, text: &str);                                    // 통과
    pub fn replace(&mut self, src: &str, dst: &str, rule: &'static str);   // 문자 폴딩
    pub fn numeral(&mut self, src: &str, dst: &str, rule: &'static str);   // 한글 수사
    pub fn absorb(&mut self, src: &str, rule: &'static str, class: Absorbed); // 구분자 흡수
    pub fn finish(self) -> (String, SpanMap);
}
```

정규화 패스를 직접 만들어 붙이려면 `SpanMapBuilder` 로 조각을 넘기고 `SpanMap::compose` 로
기존 파이프라인에 합치면 된다. 각 메서드가 대응하는 비용 필드를 자동으로 채우므로 패스 구현자가
비용 계상을 잊을 수 없다.

### 검증식 (`detect::checksum`)

```rust
pub fn analyze_resident(digits: &[u8]) -> Option<ResidentAnalysis>;  // 주민, 외국인 공통
pub fn business_registration(digits: &[u8]) -> ChecksumResult;
pub fn luhn(digits: &[u8]) -> ChecksumResult;
pub fn gender_code(digit: u8) -> Option<GenderCode>;
pub fn is_valid_date(year: u16, month: u8, day: u8) -> bool;
pub fn to_digits(text: &str) -> Option<Vec<u8>>;
```

검증식만 따로 쓸 수 있다. 이 모듈에는 **근거가 확인된 계산만** 들어간다. 운전면허번호에 함수가 없는
것이 그 원칙의 표현이다.

### 스캐너와 문맥 (`detect::scanner`, `detect::context`)

```rust
pub fn scanner::digit_runs(text: &str) -> Vec<Span>;
pub fn scanner::emails(text: &str) -> Vec<Span>;
pub fn context::find(text: &str, span: &Span, window: u32, cues: &[Cue]) -> Vec<ContextHit>;
pub fn context::total(hits: &[ContextHit]) -> f32;
```

문맥 단서 사전은 엔티티별로 상수로 공개된다(`context::RESIDENT`, `context::ACCOUNT` 등).

### 에러

```rust
pub enum Error {
    SpanMapInvariant { index: usize, detail: String },
    SpanMapMismatch { detail: String },
}
```

둘 다 **사용자 입력으로는 발생하지 않는다.** 정규화 패스 구현 버그를 뜻하므로 발생하면 그 패스를
고쳐야 한다.

---

## 7. 엔티티별 동작과 성숙도

검증 수단은 모두 **근거를 확인한 뒤** 구현했다. 근거의 강도를 함께 적는다.

| 엔티티 | 검증 수단 | 최고 등급 | 근거의 강도 |
| --- | --- | --- | --- |
| 주민등록번호 | 날짜 유효성 + 성별코드 + 체크섬 | `Certain` | **확인됨.** 가중치 `2,3,4,5,6,7,8,9,2,3,4,5`, `(11 - 합 mod 11) mod 10` |
| 외국인등록번호 | 같은 구조, 성별코드 5에서 8 | `Certain` | **확인됨.** 같은 가중치에 `+2` 보정 |
| 사업자등록번호 | 체크섬 | `Certain` | **부분 확인.** 공식 문서가 알고리즘을 공개하지 않는다. 실측으로 확인 |
| 카드번호 | Luhn | `Certain` | **확인됨.** 유일하게 국제 표준(ISO/IEC 7812)으로 공개된 검증식 |
| 전화번호 | 형식 (휴대전화, 서울, 지역, 대표번호) | `Probable` | 검증식 없음 |
| 이메일 | 구조 | `Probable` | 검증식 없음 |
| 운전면허번호 | 형식 12자리 + 지역코드 | `Probable` | 검증 자릿수는 존재하나 **계산식 미공개** |
| 계좌번호 | 자릿수(10에서 14) + 문맥 | `Probable` | 은행별 체계가 달라 공통 검증식이 없다. 문맥이 필수다 |
| 생년월일 | 날짜 유효성 + 문맥 | `Probable` | 검증식 없음. 문맥이 필수다 |
| 여권번호 | 형식(영문 1에서 2자 + 숫자 7에서 8자) + 문맥 | `Probable` | 공개된 검증식이 없다. 문맥이 필수다 |

**"최고 등급" 열이 이 표의 핵심이다.** `Certain` 에 도달할 수 없는 엔티티는 구조적으로 오탐을 완전히
없앨 수 없다. 그 사실을 `Candidate::ceiling` 으로 타입에 못박아, 문맥이 아무리 좋아도 천장을 넘지
못하게 했다.

### 검증 실패를 버리는 근거로 쓰지 않는다

주민등록번호에서 이 판단이 중요하다. **2020년 10월 개편으로 뒷자리 여섯 자리가 임의번호가 되면서 그
이후 발급분에는 검증식이 성립하지 않는다.** 번호만 봐서는 발급 시점을 알 수 없다.

그래서 검증 실패는 후보를 버리는 근거가 아니라 **등급을 낮추는 근거로만** 쓴다. 검증에 떨어져도 날짜가
유효하고 문맥이 강하면 `Probable` 로 살아남는다. 반대로 앞 6자리가 실재하지 않는 날짜이면 검증식과
무관하게 후보에서 뺀다. 날짜는 개편과 무관하게 변하지 않는 제약이기 때문이다.

### 한 구간에서 후보가 여럿 나오는 경우

열 자리 숫자열은 사업자등록번호일 수도, 지역 전화번호일 수도, 계좌번호일 수도 있다. 후보를 모두
평가한 뒤 **점수가 가장 높은 하나만** 결과로 내고 나머지는 `Outranked` 사유로 `rejections` 에
남긴다. 왜 다른 판정이 안 나왔는지가 기록된다.

스캐너가 낸 구간끼리 포개지는 경우도 같은 사유로 정리한다. 여권번호 `M12345678` 안에는 여덟 자리
숫자 런이 들어 있어 두 스캐너가 같은 자리를 각각 잡는다. 넓은 쪽이 좁은 쪽을 이미 설명하므로 좁은
쪽을 버린다.

---

## 8. 정규화 동작

패스는 순서대로 적용되고 각 패스는 자기 `SpanMap` 을 낸다.

| 순서 | 패스 | 하는 일 | 규칙 이름 |
| --- | --- | --- | --- |
| 1 | `nfc` | 자모 분리형 한글을 음절로 합친다 | `nfc.hangul_jamo` |
| 2 | `fold` | 전각을 반각으로, 대시와 공백 변종을 통일한다 | `fold.fullwidth`, `fold.dash`, `fold.space` |
| 3 | `hangul` | 한글 수사를 숫자로 되돌린다 | `hangul.digit_reading`, `hangul.unit_reading` |
| 4 | `lookalike` | 숫자 사이에 낀 유사문자를 숫자로 되돌린다 | `lookalike.digit` |
| 5 | `separator` | 숫자 사이의 붙임표, 점, 공백, 괄호를 흡수한다 | `separator.hyphen`, `separator.whitespace` 외 |

순서에는 이유가 있다. 자모를 먼저 합쳐야 수사 음절이 음절 단위로 보이고, 전각을 먼저 접어야 전각
숫자가 숫자로 보이며, 수사를 먼저 숫자로 바꿔야 구분자 흡수의 "양쪽이 숫자" 조건이 성립한다.
유사문자 교정이 구분자 흡수보다 앞에 오는 것도 같은 이유의 뒤집힌 형태다. 이 패스는 양옆이 숫자
자리일 때만 바꾸는데, 구분자를 먼저 없애 버리면 `88-O1-O1` 처럼 구분자와 유사문자가 섞인 실제
표기에서 판단 근거가 사라진다. 그래서 구분자가 아직 남아 있을 때 보고, 볼 때는 건너뛴다.

### 변환 예시

| 입력 | 정규화 결과 |
| --- | --- |
| `팔팔공일공일` | `880101` |
| `구십일년` | `91년` |
| `천구백팔십팔년생` | `1988년생` |
| `８８０１０１－１２３４５６７` | `8801011234567` |
| `880101 - 1234567` | `8801011234567` |
| `88O1O1` | `880101` |
| `POLO` 매장 | `POLO` 매장 (건드리지 않는다) |
| `구공오삼일삼이래요` | `905313이래요` |
| `제품 1234 수량 5678` | `제품 1234 수량 5678` (붙이지 않는다) |
| `이사 갑니다. 만원만 빌려줘` | `이사 갑니다. 만원만 빌려줘` (건드리지 않는다) |

### 두 가지 읽기 문법

한국어 수사 표기는 문법이 둘이라 표 하나로 끝나지 않는다.

- **자릿수 읽기**: 음절 하나가 숫자 한 자리다. `팔팔공일공일` → `880101`, `공일공` → `010`
- **단위 읽기**: 십, 백, 천, 만, 억을 계산한다. `구십일` → `91`, `이천이십사` → `2024`

판별은 단순하다. **런 안에 단위 음절이 하나라도 있으면 단위 읽기, 없으면 자릿수 읽기.**

### 오탐 억제 게이트

`사구`, `이사`, `구이` 처럼 수사 음절과 일상어가 겹치는 경우가 많다. 네 겹으로 막는다.

- **최소 자릿수.** 변환 **결과**의 자릿수가 기본 6 이상이면 문맥 없이 적용한다. 음절 수가 아니라 결과
  자릿수로 재는 이유는 두 문법의 음절 수가 다르기 때문이다. `구십일` 은 3음절이지만 결과는 2자리다
- **숫자 문맥 인접.** 그보다 짧으면 숫자 표지(`년`, `월`, `일`, `번`, `호`, `생`, `세`, `원`, `차`,
  `기`, `시`, `분`, `초`)나 인접 숫자, 붙임표가 앞뒤에 있을 때만 적용한다
- **단위 읽기의 추가 조건.** 숫자 음절이 하나도 없는 런은 변환하지 않는다. `만원` 이 `10000원` 이
  되면 안 된다
- **후단 검증에 위임.** 위 게이트를 통과해도 최종 판정은 검증식과 날짜 유효성이 한다

문법에 맞지 않는 단위 표기(`이삼십` 처럼 단위 사이에 숫자가 둘 이상 오는 경우)는 계산을 포기하고
원문을 그대로 둔다. 자리올림이 64비트를 넘칠 때도 같다.

### 말끝은 값에 삼켜지지 않는다

음성 전사(STT) 입력에서 생기는 문제다. `구공오삼일삼이래요` 의 `이` 는 숫자 2 이고 `구` 는 9 라,
수사 런을 욕심껏 모으면 어미의 첫 음절까지 먹어 `9053132` 가 된다. 한 자리가 더 붙으면 주민등록번호
앞자리로 읽히지 않는다.

그래서 런을 다 모은 뒤 **끝에 말끝이 삼켜져 있는지 보고 그만큼 줄인다.** 짝은 (`이`, `래요`),
(`이`, `에요`), (`이`, `예요`), (`이`, `요`), (`이`, `고`), (`이구`, `요`)다. 떼어 낸 음절은
버리지 않고 원문 그대로 남는다. 숫자로 읽지 않을 뿐이다.

뒤에 말끝이 없으면 줄이지 않는다. `구공오삼일삼이` 의 마지막 `이` 는 그냥 숫자 2 다.

세 임계값은 `NumeralConfig` 로 조정한다.

```rust
pub struct NumeralConfig {
    pub min_digits_without_context: usize,  // 기본 6
    pub min_digits_with_context: usize,     // 기본 2
    pub context_window: usize,              // 기본 2 (건너뛸 공백 수)
}
```

### 구분자 흡수의 범위 제한

공백을 전역으로 지우면 `제품 1234 수량 5678` 이 `제품12345678` 이 되어 **없던 여덟 자리 숫자열이
생긴다.** 그래서 흡수는 **숫자와 숫자 사이에서만** 일어난다. 그래도 위험은 남으므로 흡수한 공백 개수를
`NormalizationCost.absorbed_whitespace` 에 기록해 신뢰도에서 차감한다. 붙임표 흡수는 위험이 훨씬
낮으므로 감점 계수가 다르다.

**줄바꿈은 흡수하지 않는다.** 줄이 다르면 다른 값일 확률이 높다. 표의 마지막 열과 다음 행 첫 열이
이어 붙는 사고를 막기 위해 가로 공백(공백, 탭)만 대상으로 둔다.

### 정규화가 다루지 않는 것

의도적으로 좁힌 범위다. 넓히면 정상 텍스트를 훼손한다.

| 항목 | 하지 않는 것 | 왜 |
| --- | --- | --- |
| 유니코드 조합 | 한글 외 문자의 분해형은 합성하지 않는다 | 전체 합성 표를 의존성으로 들여와야 한다. 한글 조합은 산술식이라 표가 필요 없다 |
| 호환용 자모 | U+3131 이상의 `ㄱ`, `ㄴ` 은 조합하지 않는다 | 표준 정규화 형식 C 도 조합하지 않는다. 낱자 자체를 뜻하는 별개 문자다 |
| 유사문자 교정 | 양옆이 숫자 자리가 아니면 바꾸지 않는다. `S`와 `5`, `B`와 `8` 은 아예 다루지 않는다 | 무조건 바꾸면 `POLO` 가 `P0L0` 이 된다. `S`와 `B` 는 정상 텍스트에서 너무 흔하다 |
| 소수점 | `1.5` 가 `15` 가 된다 | 전화번호 표기의 실익을 택했다. 구분자 흡수로 계수되어 감점되고, 해당 패스만 끌 수 있다 |

### 경계 스냅

탐지 스팬이 비항등 세그먼트를 가로지르는 경우가 있다. `일억` 이 `100000000` 으로 늘어난 뒤 그 안의
`100000` 만 매칭되는 상황이다. 이때 **세그먼트 경계 바깥으로 넓혀 원자성을 지키고, 넓어졌다는 사실을
`snapped: true` 로 판정 근거에 남긴다.** 넓어진 스팬은 검증 단계에서 다시 검증식을 통과해야 하므로
잘못된 확장이 최종 결과로 나가지 않는다.

---

## 9. 판정 규칙과 설정

### 점수

```text
점수 = 패턴 기본점
     + 검증식 통과 가산점 (실패면 감점)
     + 문맥 단서 총점 x 계수
     - 정규화 비용 감점
```

```rust
pub struct Weights {
    pub checksum_passed: f32,       // 기본 0.6
    pub checksum_failed: f32,       // 기본 0.25 (감점)
    pub context: f32,               // 기본 0.5
    pub absorbed_whitespace: f32,   // 기본 0.08. 가장 무겁다
    pub absorbed_separator: f32,    // 기본 0.01
    pub expanded_syllable: f32,     // 기본 0.01
    pub replaced_char: f32,         // 기본 0.002. 값을 바꾸지 않아 가장 가볍다
}
```

감점이 있는 이유는 "정규화는 관대하게, 검증은 엄격하게" 원칙의 대가를 계량화하기 위해서다. 공백을
세 개 흡수해서 만들어진 숫자열은 원래부터 붙어 있던 숫자열보다 덜 믿을 만하다.

### 등급

| 등급 | 조건 |
| --- | --- |
| `Certain` | 검증식 통과. 단 엔티티의 천장이 `Certain` 일 때만 |
| `Probable` | 문맥 단서가 잡혔다. 또는 검증식이 없는 엔티티다 |
| `Possible` | 형식만 맞다. 문맥 단서 없음 |

등급은 `Ord` 를 구현하므로 `certainty >= Certainty::Probable` 로 걸러낼 수 있다.

### 문맥 점수

단서는 스팬 주변의 설정된 창 안에서만 찾고 **거리에 따라 감쇠**시킨다(`1 / (1 + 거리/8)`).
바로 앞에 붙은 `주민등록번호` 와 두 문장 건너의 그것은 같은 무게일 수 없다.

총점에는 **상한 1.5** 가 있다. `주민등록번호`, `주민번호`, `주민등록` 이 한꺼번에 잡히는 문장에서
점수가 세 배로 뛰는 것을 막기 위해서다. 세 단어가 같은 사실을 세 번 말하는 것이지 근거가 세 배인
것은 아니다.

단서 목록은 무게 내림차순, 동률이면 단어 오름차순으로 정렬된다. 같은 입력이면 판정 근거도 글자 단위로
같아야 하기 때문이다.

### 문맥이 필수인 엔티티

계좌번호, 운전면허번호, 생년월일, 여권번호는 숫자열만으로는 아무것도 말할 수 없다. `1234567890` 이
계좌번호인지 주문번호인지는 규칙으로 알 수 없다. 이 넷은 문맥 총점이 `min_context`(기본 0.3) 미만이면
후보 자체가 `NoContext` 로 떨어진다.

### 부정 문맥

지금까지의 단서는 전부 "이건 개인정보다" 쪽으로만 밀었다. 그래서 업무 문서가 통째로 오탐이 된다.
계약서 한 장에는 계약번호·증권번호·접수번호가 스무 개씩 들어 있고, 그중 열 자리짜리는 사업자등록번호
후보가 되며 열한 자리짜리는 전화번호 후보가 된다.

`context::EXCLUDE` 는 **이 숫자는 개인의 것이 아니다**라고 말하는 낱말 사전이다. 문서·거래 식별자
(증권번호, 계약번호, 접수번호, 주문번호, 송장번호, 관리번호), 물건 식별자(상품코드, 모델번호, 품번),
개인의 것이 아닌 전화번호(콜센터, 고객센터, 대표번호, ARS), 문서 구조(우편번호, 별표, 고시)로 되어
있다. 찾는 방식과 거리 감쇠는 긍정 단서와 완전히 같다.

버리는 조건은 셋이 다 서야 한다.

| 조건 | 왜 |
| --- | --- |
| 검증식을 통과하지 **않았다** | `증권번호` 옆이라도 검사 숫자가 맞는 열세 자리는 잘못 붙은 이름표일 뿐 개인정보다 |
| 부정 총점이 `min_veto`(기본 0.5) 이상이다 | 스치듯 걸린 낱말 하나가 판정을 뒤집으면 안 된다 |
| 부정 총점이 긍정 총점보다 크다 | `계약번호 확인 후 연락처 010-1234-5678` 처럼 두 종류가 한 창에 같이 오는 문장이 흔하다 |

떨어진 후보는 `BusinessContext` 사유로 `rejections` 에 남는다.

**이 규칙은 재현율을 정밀도보다 앞에 둔다.** 검증식 통과가 부정 문맥을 이기게 해 두었으므로, 무작위
열 자리가 사업자등록번호 검증식을 우연히 통과하면 `계약번호` 라는 이름표를 달고도 걸린다. 개인정보를
놓치는 쪽과 업무 번호를 한 건 더 가리는 쪽 중 후자를 택한 결과다.

---

## 10. 마스킹

탐지가 낸 **원문 스팬**에 정책을 적용한다. 탐지는 정규화문 위에서 하고 마스킹은 원문 위에서
하는데, 둘을 잇는 것이 `SpanMap` 하나뿐이라 복원 정확성의 책임이 한 곳에 모인다.

### 두 가지 보장

| 보장 | 범위 |
| --- | --- |
| 탐지 구간 **바깥**의 바이트는 원문과 완전히 같다 | 모든 정책 |
| `unmask(mask(text).text, &map)` 이 원문과 완전히 같다 | 토큰화 정책 |

두 번째보다 첫 번째가 더 근본적이다. 이것이 성립하면 마스킹이 문서를 조용히 훼손하는 일이
원천적으로 없다. 구현도 그 모양을 그대로 따른다. 출력은 "직전 구간 끝부터 이번 구간 시작까지를
그대로 복사하고, 구간만 치환한다"의 반복이며 원문을 파싱하거나 재조립하는 경로가 없다.

### 정책 4종

| 정책 | 결과 예시 | 가역성 |
| --- | --- | --- |
| `Redact(Label)` | `[주민등록번호]` | 불가역. 종류가 **한국어**로 남는다 |
| `Redact(Code)` | `[RESIDENT]` | 불가역. 종류가 **영문**으로 남는다 |
| `Redact(Fill('*'))` | `**************` | 불가역. 글자 수는 남는다 |
| `Redact(Fixed(s))` | 지정한 문자열 | 불가역. 아무것도 남기지 않는다 |
| `Partial` | `010******5678` | 불가역. 앞뒤 일부만 남는다 |
| `Hash` | `[PHONE:098844363e2d]` | 불가역. **같은 값은 같은 토큰**이라 연결성 분석이 된다 |
| `Tokenize` | `[[PII:0]]` | **완전 가역.** 복원 맵을 함께 낸다 |

`Redact` 는 자리표시자 모양이 넷이라 표에서 네 줄로 보이지만 정책 자체는 하나다.
`Hash` 는 `hash` 기능 플래그를 켜야 존재한다.

### 자리표시자의 언어를 고른다

이 라이브러리는 한국어 특화지만 **쓰는 사람까지 한국어 사용자인 것은 아니다.** 외국계 기업에서
한국어 문서를 다루는 엔지니어가 실제 사용자층이고, 그들의 영문 보고서에 `[카드번호]` 가 박히면
문서가 오염된다. 그래서 `Redact(Code)` 가 있다.

| 이름 | 값 | 어디에 쓰이나 |
| --- | --- | --- |
| `entity.label()` | `카드번호` | `Redact(Label)` 자리표시자 |
| `entity.code()` | `credit_card` | JSON 직렬화, Python 속성, 명령줄 출력 |
| `entity.code_upper()` | `CREDIT_CARD` | `Redact(Code)` 자리표시자, 해시 토큰 접두어 |

세 이름은 한 곳에서 정해지고 서로 갈라지지 않는지를 단위 테스트가 검사한다.

해시 토큰의 접두어는 정책과 무관하게 항상 영문이다. 사람이 읽는 자리표시자가 아니라 기계가 짝을
맞추는 값이기 때문이다.

```rust
use rust_pii_transformer::detect::{Config, EntityKind};
use rust_pii_transformer::mask::{mask, unmask, Policy, PolicySet, Redaction};

// 엔티티마다 다른 정책을 줄 수 있다.
let policies = PolicySet::new(Policy::Redact(Redaction::Label))
    .with(EntityKind::Phone, Policy::Partial { keep_prefix: 3, keep_suffix: 4, fill: '*' });

let out = mask("카드 4111-1111-1111-1111 연락처 010-1234-5678", &Config::default(), &policies).unwrap();
assert_eq!(out.text, "카드 [카드번호] 연락처 010******5678");

// 되돌릴 수 있게 가리려면 토큰화를 쓴다.
let text = "주민등록번호 팔팔공일공일 - 1234567 입니다";
let out = mask(text, &Config::default(), &PolicySet::new(Policy::Tokenize)).unwrap();
assert_eq!(unmask(&out.text, out.restore.as_ref().unwrap()).unwrap(), text);
```

### 복원 맵은 그 자체가 개인정보다

`RestoreMap` 은 토큰과 원문 조각을 짝지어 들고 있다. 마스킹 텍스트와 같은 곳에 두면 마스킹한
의미가 없다. 토큰 접두어는 **원문에 그 문자열이 없는지 확인한 뒤** 고르므로, 원문에 이미
`[[PII:0]]` 같은 문자열이 있어도 충돌하지 않는다.

### 겹치는 탐지 결과

앞선 구간과 겹치는 탐지 결과는 적용하지 않고 `MaskOutput::skipped` 에 남긴다. 비어 있는 것이
정상이고, 비어 있지 않다면 탐지 층이 겹치는 결과를 냈다는 뜻이다. 조용히 삼키지 않는다.

---

## 11. 합성 검증 코퍼스

실제 주민등록번호를 테스트에 넣을 수 없다. 그래서 검증식이 유효한 합성 데이터 생성기를 함께 둔다.
부산물이 아니라 독립적 가치가 있는 모듈이고, 재현율과 정밀도를 수치로 낼 수 있는 근거다.

### 검증식을 흉내 내지 않는다

검증 자릿수는 계산식을 다시 구현하지 않고 `detect::checksum` 의 **판정기를 그대로 돌려**
마지막 자리를 0부터 9까지 시험해 찾는다. 생성기와 판정기가 어긋날 수 없는 구조다. 계산식을
양쪽에 두 번 적으면 한쪽만 고쳤을 때 코퍼스가 조용히 거짓말을 하게 된다.

### 무엇을 만드는가

- 엔티티 10종의 값. 검증식이 있는 것은 통과하는 값으로 만든다
- 같은 값의 표기 변형 9종: 숫자만, 하이픈, 공백, 전각, 한글 수사, 유사문자, 부분마스킹, 말끝, 원형
- 문맥 단서가 있는 문장과 없는 문장
- 오탐을 유도하는 음성 표본 10종: 주문번호, 송장번호, 운송장번호, 제품 코드, 금액, 회원번호,
  증권번호, 대표번호, 계약번호, 그리고 수사 음절로 이루어진 일상어(`이사 갑니다`, `사구 팔구`)

**음성 표본을 통과하기 쉽게 고르지 않았다.** 13자리 운송장번호가 우연히 Luhn 을 통과하면 그것은
진짜 오탐이고, 그 확률까지 수치에 반영되는 것이 맞다. 남은 오탐 16건은 전부 이 종류다. 13자리
운송장번호가 Luhn 을 통과한 경우와, 10자리 계약번호가 사업자등록번호 검증식을 통과한 경우다.

```rust
use rust_pii_transformer::synth::corpus;

let samples = corpus(20260812, 40);
assert_eq!(samples, corpus(20260812, 40)); // 씨앗이 같으면 항상 같다
```

### 문맥이 필수인 엔티티는 무문맥 양성 표본을 만들지 않는다

계좌번호·운전면허번호·생년월일·여권번호는 문맥 없이는 판정하지 않기로 설계돼 있다. 그런 표본은
정답이 모호하다. 텍스트에는 개인정보가 들어 있지만 라이브러리는 설계상 그것을 내지 않고, 그 판단은
재현율의 손실이 아니라 오탐 억제의 대가이기 때문이다. 그래서 코퍼스에서 제외하고, 대신 그
동작 자체는 별도 단위 테스트가 고정한다.

**표기 변형에도 같은 규칙이 적용된다.** 부분마스킹은 검증식을 쓸 수 없게 만들고, 말끝은 음절을
펼친 비용을 문다. 둘 다 문맥이 없으면 문턱 아래로 떨어지는 것이 설계된 동작이라, 이 두 변형은
무문맥 양성 표본을 만들지 않는다.

---

## 12. 명령줄 도구 rpit

`cli` 기능 플래그로 빌드한다. 입력이 없으면 표준 입력을 읽고 `--output` 이 없으면 표준 출력으로
내므로 파이프에 그대로 붙는다.

```bash
cargo build --features cli --release

# 탐지
rpit detect --text "주민등록번호 팔팔공일공일 - 1234567" 
rpit detect --file report.txt --format json

# 마스킹. 토큰화는 복원 맵 경로를 반드시 준다.
rpit mask --file report.txt --policy tokenize --restore-map map.json --output masked.txt
rpit mask --text "연락처 010-1234-5678" --policy partial --keep-prefix 3 --keep-suffix 4

# 자리표시자 언어를 고른다. label 은 한국어, code 는 영문.
rpit mask --text "카드 4111-1111-1111-1111" --policy label   # 카드 [카드번호]
rpit mask --text "카드 4111-1111-1111-1111" --policy code    # 카드 [CREDIT_CARD]

# 복원
rpit unmask --file masked.txt --restore-map map.json

# 왜 안 걸렸는지까지 본다
rpit explain --text "접수번호 1234567890123 입니다"

# 합성 표본 생성
rpit synth --rounds 1 --seed 3 --format json
```

`explain` 은 통과한 결과와 함께 떨어진 후보를 사유까지 낸다.

```text
탐지 0 건

떨어진 후보 2 건
  카드번호           1234567890123                사유 ChecksumFailed  점수 0.15
  계좌번호           1234567890123                사유 NoContext       점수 0.10
```

토큰화 정책에 `--restore-map` 을 주지 않으면 **실행을 거부한다.** 맵 없이 만든 마스킹 결과는
되돌릴 수 없는데, 가역 정책을 골랐다는 것은 되돌릴 의사가 있다는 뜻이기 때문이다.

---

## 13. 성능

release 빌드, 20회 평균 실측치다.

| 입력 성격 | 크기 | 전체 | 처리량 | 정규화분 | 탐지분 |
| --- | --- | --- | --- | --- | --- |
| 개인정보 혼합 문서 | 86.6 KB | 3.23 ms | 25.6 MB/s | 1.15 ms | 2.08 ms |
| 평범한 산문 | 65.0 KB | 0.46 ms | 135.9 MB/s | 0.46 ms | 0.00 ms |

문서 한 건(866바이트) 기준 지연 시간은 2,000회 측정에서 중앙값 **0.019 ms**, 95 백분위 0.033 ms,
99 백분위 0.057 ms 다.

개인정보가 없는 텍스트가 다섯 배 이상 빠른 것은 각 패스가 변환 대상이 하나도 없으면 항등 매핑으로
빠지고, 숫자 런이 없으면 후보 평가 자체가 일어나지 않기 때문이다.

**이 수치는 처리량이지 정확도가 아니다.** 재현율과 정밀도는 아직 측정하지 못했다.

---

## 14. Python 바인딩

PyO3 로 abi3(파이썬 안정 이진 인터페이스) 휠을 만들어 Python 3.9 이상 전 플랫폼에서 Rust 툴체인 없이
설치되게 한다. 네 층을 모두 낸다.

### 네 층을 모두 낸다

```python
import rust_pii_transformer as rpit

text = "주민등록번호 팔팔공일공일 - 1234567 이고 연락처는 010-1234-5678 입니다"

# 탐지
report = rpit.detect(text)
for f in report.findings:
    print(f.entity, f.certainty, f.score, f.text(text))
# resident probable 0.86 팔팔공일공일 - 1234567
# phone    probable 0.98 010-1234-5678

# 왜 안 걸렸는지
for r in rpit.detect("접수번호 1234567890123").rejections:
    print(r.entity, r.reason)      # bank_account no_context

# 엔티티마다 다른 정책
policies = (rpit.PolicySet(rpit.Policy.redact_label())
            .with_entity("phone", rpit.Policy.partial(3, 4)))
rpit.mask(text, policies).text

# 토큰화는 원문 복원이 보장된다. 맵을 저장했다가 다른 프로세스에서 되돌려도 된다.
out = rpit.mask(text, rpit.PolicySet(rpit.Policy.tokenize()))
blob = out.restore.to_json()
assert rpit.unmask(out.text, rpit.RestoreMap.from_json(blob)) == text
```

### 오프셋 매핑을 직접 쓰기

자기 정규화 패스를 만들고 그 결과를 원문 좌표로 되돌리고 싶을 때 쓴다.

```python
b = rpit.SpanMapBuilder()
b.keep("880101")
b.absorb("-", "separator.hyphen", "separator")
b.keep("1234567")
normalized, smap = b.finish()          # '8801011234567'
smap.validate()                        # 불변식이 깨졌으면 SpanMapError

src = smap.to_source(rpit.Span(0, 13, 0, 13))
src.byte_start, src.byte_end           # (0, 14) 원문에서는 하이픈까지 14바이트
src.rules                              # ['separator.hyphen'] 판정 근거
```

### 문자 좌표가 1급 시민이다

Python 문자열은 문자 인덱스 기반이라 바이트 오프셋만 주면 소비자 쪽에서 다시 변환해야 하고, 그 변환이
새로운 오프셋 버그의 출처가 된다. 그래서 모든 스팬이 두 좌표를 함께 내고, 문자 좌표만 아는 흔한 상황을
위한 진입점을 따로 둔다.

```python
start = normalized.index("880101")     # Python 의 index 는 문자 기준이다
src = smap.to_source_from_chars(normalized, start, start + 6)
src.span.slice(source_text)            # 원문 조각을 그대로 잘라 낸다
```

### 공개 표면

| 이름 | 내용 |
| --- | --- |
| `detect(text, config=None)` | `Report` |
| `normalize(text, config=None)` | `Normalized` |
| `mask(text, policies=None, config=None)` | `MaskOutput` |
| `unmask(masked, restore_map)` | 원문 문자열 |
| `entity_names()` | 이 빌드가 다루는 엔티티 이름 전부 |
| `Config` | `min_score`, `min_context`, `min_veto`, `context_window`, `nfc`, `fold`, `hangul`, `lookalike`, `separator`, `numeral_*`, `weights`, `set_weights(...)` |
| `Report` | `findings`, `rejections`, `normalized_text`, `to_json` |
| `Finding` | `entity`, `entity_label`, `source`, `normalized`, `certainty`, `score`, `evidence`, `text(src)`, `to_json` |
| `Evidence` | `rule`, `checksum`, `checksum_reason`, `context_hits`, `normalizations`, `cost`, `snapped` |
| `Rejection` | `entity`, `source`, `reason`, `score` |
| `Policy` | `redact_label`(한국어), `redact_code`(영문), `redact_fill`, `redact_fixed`, `partial`, `hash`, `tokenize` |
| `PolicySet(default=None)` | `with_entity`, `policy_for` |
| `MaskOutput` | `text`, `applied`, `skipped`, `restore` |
| `RestoreMap` | `prefix`, `entries`, `to_json`, `RestoreMap.from_json` |
| `Span(byte_start, byte_end, char_start, char_end)` | `Span.from_char_range(text, s, e)`, `slice(text)` |
| `SpanMapBuilder(text=None)` | `keep`, `replace`, `numeral`, `absorb`, `finish()` |
| `SpanMap` | `identity`, `compose`, `to_source`, `to_source_from_chars`, `validate`, `segments`, `to_json` |
| `SourceSpan`, `Segment`, `NormalizationCost` | 스팬 복원 결과와 내성 |
| `SpanMapError` | 불변식 위반, 좌표계 불일치, 복원 토큰 오류 |

모듈 수준에 `__version__`, `__status__`, `__has_hash_policy__` 가 있다. 마지막 것은 이 휠에 해시
가명화 정책이 들어 있는지를 런타임에 알려 준다.

### 열거형은 문자열로 낸다

`entity`, `certainty`, `reason`, `checksum` 은 전부 소문자 스네이크 케이스 문자열이다. Python 에서
비교와 직렬화가 그대로 되고 사전 키로 바로 쓸 수 있다. 사람이 읽는 한국어 이름은 `entity_label` 이고,
`to_json()` 이 내는 이름도 속성값과 같다.

`absorb` 의 세 번째 인자는 `"whitespace"`, `"separator"`, `"other"` 중 하나다. 종류마다 감점 계수가
다르므로 정확히 골라야 하고, 다른 값을 주면 `ValueError` 가 난다.

### 경계에서 지키는 두 가지

- **패닉을 Python 으로 넘기지 않는다.** 코어의 `Span::slice` 는 범위 밖에서 패닉하고 `finish` 는
  빌더를 소비한다. 바인딩이 그 앞에서 검사해 `ValueError` 와 `RuntimeError` 로 바꾼다
- **코어를 오염시키지 않는다.** `span` 타입에 `#[pyclass]` 를 직접 달지 않고 `src/python.rs` 안의
  래퍼에만 붙인다. 그래서 기본 빌드에는 PyO3 가 전혀 들어오지 않는다

### 규칙 이름은 상수로 둔다

`SpanMapBuilder` 에 넘기는 규칙 이름은 내부에서 인터닝되어 회수되지 않는다. 코어가 규칙 이름을
`&'static str` 로 들고 다니기 때문이다. 고정된 소수의 이름을 쓰면 풀 크기가 상수에 수렴하지만,
반복문에서 `f"rule.{i}"` 처럼 매번 새 이름을 만들면 그만큼 쌓인다. 가변값은 규칙 이름이 아니라
다른 인자로 넘긴다.

---

## 15. 알려진 한계

### 다루지 않는 항목

| 항목 | 왜 |
| --- | --- |
| 이름, 주소 | 규칙으로 판별할 수 없다. 개체명 인식이 필요한 영역이라 코어에 넣지 않는다 |
| 한국 외 식별번호 | 이 라이브러리는 한국어 텍스트 전용이다. 문서 첫 절 참조 |

### 판정이 흔들리는 자리

**문맥 없는 숫자열은 원리적으로 판별할 수 없다.** `1234567890` 이 계좌번호인지 주문번호인지
규칙만으로는 알 수 없다. 계좌번호와 운전면허번호와 생년월일이 문맥을 요구하는 이유가 이것이다.

**13자리 숫자에서는 카드번호와 계좌번호가 겹친다.** 검증식이 문맥보다 강하게 작동하므로, 계좌
문맥 안에 있어도 Luhn 을 통과하면 카드번호로 판정된다. 합성 코퍼스에서 계좌번호만 재현율과
정밀도가 95퍼센트인 것이 여기서 온다. 문맥 가중치를 올리면 이 경우는 고쳐지지만 다른 곳에서
오탐이 늘어난다. 지금 균형을 기본값으로 두었고 `Config` 로 조정할 수 있다.

**우연한 검증식 통과는 막을 수 없다.** 무작위 13자리의 약 10퍼센트가 Luhn 을 통과하고, 무작위
10자리의 약 10퍼센트가 사업자등록번호 검증식을 통과한다. 그런 운송장번호는 카드번호로, 그런 계약번호는
사업자등록번호로 걸린다. 부정 문맥 사전이 이 자리를 겨냥하지만 검증식 통과가 부정 문맥을 이기도록
설계했기 때문에 여기서는 작동하지 않는다. 개인정보를 놓치는 쪽보다 낫다고 판단한 결과다.

### 정확도 수치의 근거 범위

이 문서의 재현율과 정밀도는 [합성 검증 코퍼스](#11-합성-검증-코퍼스)에서 잰 값이다. 회귀를
막고 변경의 영향을 보는 데는 충분하지만, 실제 한국어 문서를 표본으로 잰 값은 아니다.
자기 데이터에서 재보고 임계값을 맞추는 것을 권한다.

---

## 16. 빌드와 테스트

```bash
cargo build
cargo test
cargo clippy --all-targets -- -D warnings

# 옵트인 기능 포함
cargo test  --features "cli,hash"
cargo clippy --features "cli,hash,python" --all-targets -- -D warnings

# 실행 예제
cargo run --example demo

# 정확도와 복원 왕복을 눈으로 본다
cargo test --test accuracy -- --nocapture
```

### Python 확장 모듈

```bash
pip install maturin
maturin develop                       # pyproject.toml 이 features = ["python"] 을 지정한다
pytest tests/test_python_binding.py
```

maturin 없이 확인하려면 cargo 로 cdylib 를 만든 뒤 확장자만 바꿔 경로에 둔다.

```bash
cargo build --features python --release
# Windows:  target/release/rust_pii_transformer.dll      -> rust_pii_transformer.pyd
# Linux:    target/release/librust_pii_transformer.so    -> rust_pii_transformer.so
# macOS:    target/release/librust_pii_transformer.dylib -> rust_pii_transformer.so
```

### 디렉토리 구조

- `src/`
  - `lib.rs` 공개 API 진입점
  - `error.rs` 단일 에러 열거형
  - `span.rs` 오프셋 매핑
  - `normalize/` `mod.rs`, `nfc.rs`, `fold.rs`, `hangul.rs`, `separator.rs`
  - `detect/` `mod.rs`, `checksum.rs`, `scanner.rs`, `context.rs`, `entity.rs`
  - `mask/` `mod.rs`, `policy.rs`, `restore.rs`
  - `synth/mod.rs` 합성 검증 코퍼스 생성기
  - `bin/rpit.rs` 명령줄 도구 (`--features cli`)
  - `python.rs` PyO3 바인딩 (`--features python`)
- `examples/demo.rs` 탐지 실주행 예제
- `tests/accuracy.rs` 재현율·정밀도와 복원 왕복 실측
- `tests/test_python_binding.py` 바인딩 회귀 테스트

---

## 17. 라이선스

Apache License 2.0
