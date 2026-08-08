//! PyO3 바인딩 — README 15절.
//!
//! `feature = "python"` 을 켰을 때만 빌드된다. abi3(안정 이진 인터페이스) 확장 모듈이라
//! Python 3.9 이상에서 휠 하나로 돈다.
//!
//! ## 코어를 오염시키지 않는다
//!
//! [`crate::span`] 의 타입에 `#[pyclass]` 를 직접 달지 않고 이 모듈 안에 래퍼를 둔다.
//! 그래야 기본 빌드가 PyO3 를 전혀 모르는 순수 Rust 로 남는다(독립성 원칙, README 16절).
//!
//! ## 노출 범위는 지금 실제로 도는 것까지다
//!
//! 현재 구현된 층은 오프셋 매핑(`span`) 하나뿐이므로 이 모듈도 거기까지만 낸다.
//! `detect` 와 `mask` 는 **함수 자체를 만들지 않는다.** 던지기만 하는 껍데기를 미리 노출하면
//! 소비자가 그것을 있는 기능으로 오해하기 때문이다. 모듈 수준 `__status__` 로 현재 범위를 밝힌다.
//!
//! ```python
//! import rust_pii_transformer as rpit
//!
//! b = rpit.SpanMapBuilder()
//! b.keep("880101")
//! b.absorb("-", "separator.hyphen", "separator")
//! b.keep("1234567")
//! normalized, smap = b.finish()          # "8801011234567"
//!
//! src = smap.to_source(rpit.Span(0, 13, 0, 13))
//! src.byte_start, src.byte_end           # (0, 14) — 원문에서는 하이픈까지
//! src.cost.absorbed_separators           # 1 — 판정 근거로 실린다
//! ```

use std::collections::HashSet;
use std::sync::{Mutex, OnceLock};

use pyo3::create_exception;
use pyo3::exceptions::{PyException, PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyModule;

use crate::span::{
    Absorbed, NormalizationCost, RuleId, Segment, SegmentKind, SourceSpan, Span, SpanMap,
    SpanMapBuilder,
};

// ── 예외 ────────────────────────────────────────────────────

create_exception!(
    rust_pii_transformer,
    SpanMapError,
    PyException,
    "구간 정렬 테이블의 불변식이 깨졌거나 좌표계가 맞지 않는다."
);

fn map_err(e: crate::Error) -> PyErr {
    SpanMapError::new_err(e.to_string())
}

// ── 규칙 식별자 인터닝 ───────────────────────────────────────

/// Python 이 넘긴 규칙 이름을 `&'static str` 로 바꾼다.
///
/// 코어는 규칙 이름을 `&'static str` 로 들고 다녀 힙 할당을 0 으로 만든다(README 6절).
/// 그 선택이 Python 경계에서는 대가를 만든다. 런타임에 들어온 문자열에는 `'static` 수명이
/// 없으므로 누출(leak)로 승격시켜야 한다.
///
/// 같은 이름은 한 번만 누출되도록 풀에 담는다. 규칙 이름은 설계상 정규화 패스가 가진
/// 고정된 소수 집합이므로 실사용에서 풀 크기는 상수에 수렴한다.
///
/// **한계를 감추지 않는다.** 호출자가 매번 새로운 규칙 이름을 만들어 넣으면
/// (예: `f"rule.{i}"` 를 반복) 그만큼 메모리가 누적되고 회수되지 않는다.
/// 규칙 이름은 상수로 두고 값은 다른 인자로 넘기는 것이 올바른 사용법이다.
fn intern(name: &str) -> RuleId {
    static POOL: OnceLock<Mutex<HashSet<&'static str>>> = OnceLock::new();
    let pool = POOL.get_or_init(|| Mutex::new(HashSet::new()));
    // 잠금이 오염돼도 풀 자체는 여전히 정합하므로 그대로 이어 쓴다.
    let mut guard = pool.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if let Some(found) = guard.get(name) {
        return found;
    }
    let leaked: &'static str = Box::leak(name.to_owned().into_boxed_str());
    guard.insert(leaked);
    leaked
}

// ── 문자열 변환 ─────────────────────────────────────────────

fn kind_name(kind: SegmentKind) -> &'static str {
    match kind {
        SegmentKind::Identity => "identity",
        SegmentKind::Replace => "replace",
        SegmentKind::Delete => "delete",
        SegmentKind::Expand => "expand",
        SegmentKind::Insert => "insert",
    }
}

fn parse_absorbed(class: &str) -> PyResult<Absorbed> {
    match class {
        "whitespace" => Ok(Absorbed::Whitespace),
        "separator" => Ok(Absorbed::Separator),
        "other" => Ok(Absorbed::Other),
        _ => Err(PyValueError::new_err(format!(
            "unknown absorbed class {class:?}; expected 'whitespace', 'separator', or 'other'"
        ))),
    }
}

// ── Span ────────────────────────────────────────────────────

/// 텍스트 한 구간. 바이트 오프셋과 문자 오프셋을 둘 다 담는다.
///
/// Python 문자열은 문자 인덱스 기반이라 문자 좌표를 그대로 노출하는 것이 중요하다.
/// 바이트만 주면 소비자 쪽에서 다시 변환해야 하고 그 변환이 새 오프셋 버그의 출처가 된다.
#[pyclass(name = "Span", module = "rust_pii_transformer", skip_from_py_object)]
#[derive(Clone)]
pub struct PySpan {
    inner: Span,
}

#[pymethods]
impl PySpan {
    #[new]
    fn new(byte_start: u32, byte_end: u32, char_start: u32, char_end: u32) -> Self {
        Self { inner: Span::new(byte_start..byte_end, char_start..char_end) }
    }

    /// 문자 구간만 알 때 텍스트를 훑어 바이트 구간까지 채운 스팬을 만든다.
    ///
    /// Python 소비자가 문자 인덱스만 들고 있는 흔한 상황을 위한 편의 생성자다.
    #[staticmethod]
    fn from_char_range(text: &str, char_start: u32, char_end: u32) -> PyResult<Self> {
        if char_start > char_end {
            return Err(PyValueError::new_err(format!(
                "char_start {char_start} must not exceed char_end {char_end}"
            )));
        }
        // bounds[i] = i 번째 문자의 시작 바이트, 마지막 항목은 텍스트 끝.
        let mut bounds: Vec<u32> = text.char_indices().map(|(b, _)| b as u32).collect();
        bounds.push(text.len() as u32);
        let last = (bounds.len() - 1) as u32;
        if char_end > last {
            return Err(PyValueError::new_err(format!(
                "char_end {char_end} is out of range; text has {last} characters"
            )));
        }
        Ok(Self::new(
            bounds[char_start as usize],
            bounds[char_end as usize],
            char_start,
            char_end,
        ))
    }

    #[getter]
    fn byte_start(&self) -> u32 {
        self.inner.byte.start
    }

    #[getter]
    fn byte_end(&self) -> u32 {
        self.inner.byte.end
    }

    #[getter]
    fn char_start(&self) -> u32 {
        self.inner.char.start
    }

    #[getter]
    fn char_end(&self) -> u32 {
        self.inner.char.end
    }

    #[getter]
    fn byte_len(&self) -> u32 {
        self.inner.byte_len()
    }

    #[getter]
    fn char_len(&self) -> u32 {
        self.inner.char_len()
    }

    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    /// 이 스팬이 가리키는 조각을 잘라 낸다. 범위를 벗어나면 예외를 낸다.
    ///
    /// 코어의 `Span::slice` 는 범위 밖에서 패닉하므로, Python 으로 패닉을 넘기지 않도록
    /// 여기서 먼저 확인한다.
    fn slice(&self, text: &str) -> PyResult<String> {
        let (start, end) = (self.inner.byte.start as usize, self.inner.byte.end as usize);
        if end > text.len() || start > end {
            return Err(PyValueError::new_err(format!(
                "span {start}..{end} is out of range for text of {} bytes",
                text.len()
            )));
        }
        if !text.is_char_boundary(start) || !text.is_char_boundary(end) {
            return Err(PyValueError::new_err(format!(
                "span {start}..{end} does not fall on UTF-8 character boundaries"
            )));
        }
        Ok(text[start..end].to_owned())
    }

    fn __repr__(&self) -> String {
        format!(
            "Span(byte={}..{}, char={}..{})",
            self.inner.byte.start, self.inner.byte.end, self.inner.char.start, self.inner.char.end
        )
    }

    fn __eq__(&self, other: &Self) -> bool {
        self.inner == other.inner
    }
}

// ── NormalizationCost ───────────────────────────────────────

/// 이 스팬이 원문에서 얼마나 멀어졌는지의 정량 지표. 탐지 층이 신뢰도를 깎는 데 쓴다.
#[pyclass(name = "NormalizationCost", module = "rust_pii_transformer", skip_from_py_object)]
#[derive(Clone)]
pub struct PyNormalizationCost {
    inner: NormalizationCost,
}

#[pymethods]
impl PyNormalizationCost {
    #[getter]
    fn absorbed_whitespace(&self) -> u16 {
        self.inner.absorbed_whitespace
    }

    #[getter]
    fn absorbed_separators(&self) -> u16 {
        self.inner.absorbed_separators
    }

    #[getter]
    fn expanded_syllables(&self) -> u16 {
        self.inner.expanded_syllables
    }

    #[getter]
    fn replaced_chars(&self) -> u16 {
        self.inner.replaced_chars
    }

    fn is_zero(&self) -> bool {
        self.inner.is_zero()
    }

    fn __repr__(&self) -> String {
        format!(
            "NormalizationCost(absorbed_whitespace={}, absorbed_separators={}, expanded_syllables={}, replaced_chars={})",
            self.inner.absorbed_whitespace,
            self.inner.absorbed_separators,
            self.inner.expanded_syllables,
            self.inner.replaced_chars
        )
    }
}

// ── Segment ─────────────────────────────────────────────────

/// 정규화 한 조각. 연속한 항등 구간은 하나로 접혀 있다.
#[pyclass(name = "Segment", module = "rust_pii_transformer", skip_from_py_object)]
#[derive(Clone)]
pub struct PySegment {
    inner: Segment,
}

#[pymethods]
impl PySegment {
    /// 입력(원문) 쪽 구간.
    #[getter]
    fn src(&self) -> PySpan {
        PySpan { inner: self.inner.src.clone() }
    }

    /// 출력(정규화문) 쪽 구간.
    #[getter]
    fn dst(&self) -> PySpan {
        PySpan { inner: self.inner.dst.clone() }
    }

    /// `"identity"`, `"replace"`, `"delete"`, `"expand"`, `"insert"` 중 하나.
    #[getter]
    fn kind(&self) -> &'static str {
        kind_name(self.inner.kind)
    }

    /// 이 구간을 만든 규칙들. 이름 오름차순이고 항등 구간은 비어 있다.
    #[getter]
    fn rules(&self) -> Vec<String> {
        self.inner.rules.iter().map(|r| (*r).to_owned()).collect()
    }

    #[getter]
    fn cost(&self) -> PyNormalizationCost {
        PyNormalizationCost { inner: self.inner.cost }
    }

    fn __repr__(&self) -> String {
        format!(
            "Segment(kind={:?}, src={}..{}, dst={}..{})",
            kind_name(self.inner.kind),
            self.inner.src.byte.start,
            self.inner.src.byte.end,
            self.inner.dst.byte.start,
            self.inner.dst.byte.end
        )
    }
}

// ── SourceSpan ──────────────────────────────────────────────

/// [`PySpanMap`] 의 `to_source` 결과. 원문 스팬과 그 판정 근거.
#[pyclass(name = "SourceSpan", module = "rust_pii_transformer", skip_from_py_object)]
#[derive(Clone)]
pub struct PySourceSpan {
    inner: SourceSpan,
}

#[pymethods]
impl PySourceSpan {
    /// 원문 기준 스팬.
    #[getter]
    fn span(&self) -> PySpan {
        PySpan { inner: self.inner.span.clone() }
    }

    #[getter]
    fn byte_start(&self) -> u32 {
        self.inner.span.byte.start
    }

    #[getter]
    fn byte_end(&self) -> u32 {
        self.inner.span.byte.end
    }

    #[getter]
    fn char_start(&self) -> u32 {
        self.inner.span.char.start
    }

    #[getter]
    fn char_end(&self) -> u32 {
        self.inner.span.char.end
    }

    /// 세그먼트 경계 바깥으로 넓혀졌는가.
    #[getter]
    fn snapped(&self) -> bool {
        self.inner.snapped
    }

    /// 이 구간에 적용된 정규화 규칙 목록. 중복이 제거되고 이름 오름차순으로 정렬돼 있다.
    #[getter]
    fn rules(&self) -> Vec<String> {
        self.inner.rules.iter().map(|r| (*r).to_owned()).collect()
    }

    #[getter]
    fn cost(&self) -> PyNormalizationCost {
        PyNormalizationCost { inner: self.inner.cost }
    }

    /// 판정 근거를 JSON 문자열로 낸다.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    fn __repr__(&self) -> String {
        format!(
            "SourceSpan(byte={}..{}, char={}..{}, snapped={}, rules={:?})",
            self.inner.span.byte.start,
            self.inner.span.byte.end,
            self.inner.span.char.start,
            self.inner.span.char.end,
            self.inner.snapped,
            self.inner.rules
        )
    }
}

// ── SpanMap ─────────────────────────────────────────────────

/// 원문과 정규화문을 잇는 단조 구간 정렬 테이블.
#[pyclass(name = "SpanMap", module = "rust_pii_transformer", skip_from_py_object)]
#[derive(Clone)]
pub struct PySpanMap {
    inner: SpanMap,
}

#[pymethods]
impl PySpanMap {
    /// 변환이 전혀 없는 항등 매핑.
    #[staticmethod]
    fn identity(text: &str) -> Self {
        Self { inner: SpanMap::identity(text) }
    }

    /// 두 매핑을 하나로 합친다. `inner` 가 A→B, `outer` 가 B→C 면 결과는 A→C 다.
    ///
    /// `inner` 의 출력 좌표계와 `outer` 의 입력 좌표계가 다르면 `SpanMapError` 를 낸다.
    #[staticmethod]
    fn compose(inner: &PySpanMap, outer: &PySpanMap) -> PyResult<Self> {
        SpanMap::compose(&inner.inner, &outer.inner)
            .map(|m| Self { inner: m })
            .map_err(map_err)
    }

    /// 정규화문 구간을 원문 구간으로 되돌린다.
    fn to_source(&self, dst: &PySpan) -> PySourceSpan {
        PySourceSpan { inner: self.inner.to_source(&dst.inner) }
    }

    /// 문자 좌표만으로 되돌린다. 정규화문을 함께 넘겨 바이트 좌표를 채운다.
    ///
    /// `to_source(Span.from_char_range(normalized, start, end))` 의 축약이다.
    fn to_source_from_chars(
        &self,
        normalized: &str,
        char_start: u32,
        char_end: u32,
    ) -> PyResult<PySourceSpan> {
        let dst = PySpan::from_char_range(normalized, char_start, char_end)?;
        Ok(self.to_source(&dst))
    }

    /// 세그먼트 목록.
    #[getter]
    fn segments(&self) -> Vec<PySegment> {
        self.inner.segments().iter().map(|s| PySegment { inner: s.clone() }).collect()
    }

    /// 변환이 하나도 없는가.
    fn is_identity(&self) -> bool {
        self.inner.is_identity()
    }

    /// 원문 끝 좌표 `(byte, char)`.
    #[getter]
    fn src_end(&self) -> (u32, u32) {
        self.inner.src_end()
    }

    /// 정규화문 끝 좌표 `(byte, char)`.
    #[getter]
    fn dst_end(&self) -> (u32, u32) {
        self.inner.dst_end()
    }

    /// 불변식 검사. 깨졌으면 `SpanMapError` 를 낸다.
    fn validate(&self) -> PyResult<()> {
        self.inner.validate().map_err(map_err)
    }

    /// 테이블 전체를 JSON 문자열로 낸다.
    fn to_json(&self) -> PyResult<String> {
        serde_json::to_string(&self.inner).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    fn __len__(&self) -> usize {
        self.inner.segments().len()
    }

    fn __repr__(&self) -> String {
        let (sb, _) = self.inner.src_end();
        let (db, _) = self.inner.dst_end();
        format!(
            "SpanMap(segments={}, src_bytes={sb}, dst_bytes={db})",
            self.inner.segments().len()
        )
    }
}

// ── SpanMapBuilder ──────────────────────────────────────────

/// 정규화 패스가 출력 문자열과 [`PySpanMap`] 을 동시에 만드는 빌더.
///
/// 원문 조각을 앞에서부터 빠짐없이 한 번씩 넘겨야 피복 불변식이 성립한다.
///
/// 코어의 `insert` 는 여기에 노출하지 않는다. 현재 정규화 파이프라인이 쓰지 않는 경로이고,
/// 합성 시 불변식과 결합법칙이 깨지는 것이 실측으로 확인됐기 때문이다. 그 경로가 정리되면
/// 여기에 함께 추가한다.
#[pyclass(name = "SpanMapBuilder", module = "rust_pii_transformer")]
pub struct PySpanMapBuilder {
    /// `finish` 가 빌더를 소비하므로 Option 으로 들고 있다가 꺼낸다.
    inner: Option<SpanMapBuilder>,
}

impl PySpanMapBuilder {
    fn get(&mut self) -> PyResult<&mut SpanMapBuilder> {
        self.inner.as_mut().ok_or_else(|| {
            PyRuntimeError::new_err("this builder was already consumed by finish()")
        })
    }
}

#[pymethods]
impl PySpanMapBuilder {
    /// 빈 빌더. `text` 를 주면 출력 버퍼를 그 길이만큼 미리 잡는다.
    #[new]
    #[pyo3(signature = (text=None))]
    fn new(text: Option<&str>) -> Self {
        let builder = match text {
            Some(t) => SpanMapBuilder::with_capacity(t),
            None => SpanMapBuilder::new(),
        };
        Self { inner: Some(builder) }
    }

    /// 원문 조각을 그대로 통과시킨다. 연속 호출은 하나의 항등 세그먼트로 접힌다.
    fn keep(&mut self, text: &str) -> PyResult<()> {
        self.get()?.keep(text);
        Ok(())
    }

    /// 문자 폴딩. 전각을 반각으로 바꾸거나 유사문자를 교정할 때 쓴다.
    fn replace(&mut self, src_text: &str, dst_text: &str, rule: &str) -> PyResult<()> {
        let rule = intern(rule);
        self.get()?.replace(src_text, dst_text, rule);
        Ok(())
    }

    /// 한글 수사 역변환. `팔팔공일공일` 을 `880101` 로 옮길 때 쓴다.
    fn numeral(&mut self, src_text: &str, dst_text: &str, rule: &str) -> PyResult<()> {
        let rule = intern(rule);
        self.get()?.numeral(src_text, dst_text, rule);
        Ok(())
    }

    /// 원문 조각을 흡수(삭제)한다.
    ///
    /// `class_` 는 `"whitespace"`, `"separator"`, `"other"` 중 하나다. 종류마다 신뢰도
    /// 감점 계수가 다르므로 정확히 골라야 한다.
    #[pyo3(signature = (src_text, rule, class_="separator"))]
    fn absorb(&mut self, src_text: &str, rule: &str, class_: &str) -> PyResult<()> {
        let class = parse_absorbed(class_)?;
        let rule = intern(rule);
        self.get()?.absorb(src_text, rule, class);
        Ok(())
    }

    /// 정규화 결과 문자열과 매핑을 `(str, SpanMap)` 으로 낸다. 빌더는 여기서 소비된다.
    fn finish(&mut self) -> PyResult<(String, PySpanMap)> {
        let builder = self.inner.take().ok_or_else(|| {
            PyRuntimeError::new_err("this builder was already consumed by finish()")
        })?;
        let (text, map) = builder.finish();
        Ok((text, PySpanMap { inner: map }))
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            Some(_) => "SpanMapBuilder(active)".to_owned(),
            None => "SpanMapBuilder(consumed)".to_owned(),
        }
    }
}

// ── 모듈 ────────────────────────────────────────────────────

/// Python 모듈 정의. 모듈명은 cdylib 이름(`rust_pii_transformer`)과 일치해야 한다.
#[pymodule]
fn rust_pii_transformer(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("__version__", env!("CARGO_PKG_VERSION"))?;
    // 현재 범위를 소비자가 런타임에 확인할 수 있게 남긴다. 없는 기능을 껍데기로 만들지 않는다.
    m.add(
        "__status__",
        "span layer only — normalize, detect, and mask are not implemented yet",
    )?;

    m.add("SpanMapError", m.py().get_type::<SpanMapError>())?;
    m.add_class::<PySpan>()?;
    m.add_class::<PyNormalizationCost>()?;
    m.add_class::<PySegment>()?;
    m.add_class::<PySourceSpan>()?;
    m.add_class::<PySpanMap>()?;
    m.add_class::<PySpanMapBuilder>()?;
    Ok(())
}
