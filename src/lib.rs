//! # rust_pii_transformer
//!
//! 한국어 텍스트에서 개인정보를 **결정적으로** 탐지하고, **원문 복원이 보장되는** 마스킹을
//! 수행하는 **모델 없는 순수 Rust** 라이브러리.
//!
//! 모델 파일 0개, GPU 0, 외부 API 호출 0, 네트워크 접근 0. 폐쇄망에서 그대로 돈다.
//!
//! ## 핵심 차별점은 변형 표기 인식이다
//!
//! 순수 숫자열뿐 아니라 한글 수사 표기(`팔팔공일공일`), 띄어쓰기와 구분자 변형
//! (`880101 - 1234567`), 한글과 숫자 혼합 표기(`88년 1월 1일생`)를 잡는다.
//!
//! ## 3층 파이프라인
//!
//! ```text
//! 원문 → [정규화] → (정규화문, SpanMap) → [탐지] → Finding(정규화 좌표)
//!                          └────────────────────────→ Finding(원문 좌표) → [마스킹]
//! ```
//!
//! 탐지는 깨끗한 정규화문 위에서 하고 마스킹은 원문 위에서 한다. 둘을 잇는 것이
//! [`SpanMap`] 하나뿐이라 복원 정확성의 책임이 한 곳에 모인다.
//!
//! ## 현재 구현 상태
//!
//! **1층(정규화)의 오프셋 매핑 자료구조만 구현돼 있다.** 정규화 패스, 탐지, 마스킹은 아직 없다.
//! 진행 상황과 남은 범위는 저장소 `README.md` 의 개발 로드맵 절에 3단계
//! (동작 확인 / 코드만 존재 / 미구현)로 기록한다.
//!
//! | 모듈 | 상태 |
//! | --- | --- |
//! | [`span`] | 동작 확인. `SpanMap`, `Segment`, 합성, 역방향 매핑, 불변식 검사 |
//! | `normalize` | 미구현 |
//! | `detect` | 미구현 |
//! | `mask` | 미구현 |
//! | `synth` | 미구현 |
//!
//! ## 지금 쓸 수 있는 것
//!
//! ```
//! use rust_pii_transformer::{Absorbed, Span, SpanMapBuilder};
//!
//! // 정규화 패스 하나를 흉내 낸다. "880101-1234567" 에서 하이픈을 흡수한다.
//! let mut builder = SpanMapBuilder::new();
//! builder.keep("880101");
//! builder.absorb("-", "separator.hyphen", Absorbed::Separator);
//! builder.keep("1234567");
//! let (normalized, map) = builder.finish();
//! assert_eq!(normalized, "8801011234567");
//!
//! // 정규화문에서 찾은 스팬을 원문 좌표로 되돌린다.
//! let found = Span::new(0..13, 0..13);
//! let source = map.to_source(&found);
//! assert_eq!(source.span.byte, 0..14);          // 원문에서는 하이픈까지 14바이트
//! assert_eq!(source.cost.absorbed_separators, 1); // 판정 근거로 실린다
//! ```

pub mod error;
pub mod span;

pub use error::{Error, Result};
pub use span::{
    Absorbed, NormalizationCost, RuleId, Segment, SegmentKind, SourceSpan, Span, SpanMap,
    SpanMapBuilder,
};
