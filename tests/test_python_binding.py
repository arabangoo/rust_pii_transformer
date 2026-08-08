# -*- coding: utf-8 -*-
"""PyO3 바인딩 회귀 테스트.

Rust 단위 테스트와 같은 시나리오를 Python 경계에서 다시 주행한다. Rust 쪽이 통과해도
바인딩이 값을 잘못 넘기거나 예외 대신 패닉을 내면 여기서 걸린다.

먼저 확장 모듈을 빌드해야 한다.

    maturin develop --features python
    pytest tests/test_python_binding.py

maturin 없이 확인하려면 cargo 로 cdylib 를 만든 뒤 확장자만 바꿔 경로에 두면 된다.

    cargo build --features python --release
    # Windows:  target/release/rust_pii_transformer.dll  -> rust_pii_transformer.pyd
    # Linux:    target/release/librust_pii_transformer.so -> rust_pii_transformer.so
    # macOS:    target/release/librust_pii_transformer.dylib -> rust_pii_transformer.so
"""
import json

import pytest

rpit = pytest.importorskip(
    "rust_pii_transformer",
    reason="확장 모듈이 빌드되지 않았다. maturin develop --features python 을 먼저 실행한다.",
)


# ── 모듈 표면 ────────────────────────────────────────────────


def test_module_metadata():
    assert rpit.__version__
    # 현재 범위를 감추지 않는다. 탐지·마스킹 층이 들어오면 이 문자열도 함께 바뀐다.
    assert "span layer only" in rpit.__status__


def test_unimplemented_layers_are_absent_not_stubbed():
    # 던지기만 하는 껍데기를 미리 노출하지 않는다. 없으면 없는 것이다.
    for name in ("detect", "mask", "unmask"):
        assert not hasattr(rpit, name), "%s 가 껍데기로 노출됐다" % name


def test_defective_insert_path_is_not_exposed():
    # compose 시 불변식·결합법칙이 깨지는 것이 실측된 경로다. 정리 전까지 노출하지 않는다.
    assert not hasattr(rpit.SpanMapBuilder(), "insert")


# ── 오프셋 복원 ──────────────────────────────────────────────


def _rrn_builder():
    """880101-1234567 에서 하이픈을 흡수하는 빌더."""
    b = rpit.SpanMapBuilder()
    b.keep("880101")
    b.absorb("-", "separator.hyphen", "separator")
    b.keep("1234567")
    return b


def test_absorbed_separator_inside_span_is_included():
    normalized, smap = _rrn_builder().finish()
    assert normalized == "8801011234567"
    smap.validate()

    src = smap.to_source(rpit.Span(0, 13, 0, 13))
    assert (src.byte_start, src.byte_end) == (0, 14)
    assert src.snapped is False
    assert src.rules == ["separator.hyphen"]
    assert src.cost.absorbed_separators == 1


def test_absorbed_separator_at_boundary_is_excluded():
    b = rpit.SpanMapBuilder()
    b.absorb("-", "separator.hyphen", "separator")
    b.keep("880101")
    normalized, smap = b.finish()
    assert normalized == "880101"

    src = smap.to_source(rpit.Span(0, 6, 0, 6))
    assert (src.byte_start, src.byte_end) == (1, 7), "선행 하이픈은 원문 스팬에 들어가면 안 된다"
    assert src.rules == []


def test_partial_match_inside_expansion_snaps_outward():
    b = rpit.SpanMapBuilder()
    b.numeral("일억", "100000000", "hangul.numeral")
    normalized, smap = b.finish()
    assert normalized == "100000000"
    assert smap.segments[0].kind == "expand"

    src = smap.to_source(rpit.Span(0, 6, 0, 6))
    assert (src.byte_start, src.byte_end) == (0, 6), "'일억' 은 UTF-8 로 6바이트다"
    assert (src.char_start, src.char_end) == (0, 2)
    assert src.snapped is True
    assert src.cost.expanded_syllables == 2


def test_korean_byte_and_char_offsets_stay_separate():
    b = rpit.SpanMapBuilder()
    b.keep("생년월일 ")
    b.numeral("팔팔공일공일", "880101", "hangul.numeral")
    normalized, smap = b.finish()
    assert normalized == "생년월일 880101"

    src = smap.to_source(rpit.Span(13, 19, 5, 11))
    assert (src.byte_start, src.byte_end) == (13, 31), "원문의 '팔팔공일공일' 은 18바이트다"
    assert (src.char_start, src.char_end) == (5, 11)
    assert src.cost.expanded_syllables == 6


def test_source_slice_recovers_the_original_fragment():
    """복원 보장의 핵심. 되돌린 스팬이 원문 조각과 정확히 일치해야 한다."""
    source = "계좌 1234-5678 입금"
    b = rpit.SpanMapBuilder(source)
    b.keep("계좌 1234")
    b.absorb("-", "separator.hyphen", "separator")
    b.keep("5678 입금")
    normalized, smap = b.finish()
    assert normalized == "계좌 12345678 입금"

    start = normalized.index("12345678")
    src = smap.to_source_from_chars(normalized, start, start + 8)
    assert src.span.slice(source) == "1234-5678"


# ── 문자 좌표 경로 (Python 소비자의 기본 경로) ────────────────


def test_char_range_helpers_agree_with_byte_query():
    b = rpit.SpanMapBuilder()
    b.keep("생년월일 ")
    b.numeral("팔팔공일공일", "880101", "hangul.numeral")
    normalized, smap = b.finish()

    start = normalized.index("880101")  # Python 인덱스는 문자 기준이다
    span = rpit.Span.from_char_range(normalized, start, start + 6)
    assert (span.byte_start, span.byte_end) == (13, 19)
    assert smap.to_source(span).byte_end == smap.to_source_from_chars(
        normalized, start, start + 6
    ).byte_end


def test_char_range_accepts_end_of_text():
    span = rpit.Span.from_char_range("가나다", 0, 3)
    assert (span.byte_start, span.byte_end) == (0, 9)


# ── 규칙 목록 ────────────────────────────────────────────────


def test_repeated_rule_is_deduplicated_but_cost_accumulates():
    b = rpit.SpanMapBuilder()
    b.keep("1")
    b.absorb("-", "separator.hyphen", "separator")
    b.keep("2")
    b.absorb("-", "separator.hyphen", "separator")
    b.keep("3")
    _, smap = b.finish()

    src = smap.to_source(rpit.Span(0, 3, 0, 3))
    assert src.rules == ["separator.hyphen"]
    assert src.cost.absorbed_separators == 2


def test_rules_come_out_sorted_by_name_not_application_order():
    b = rpit.SpanMapBuilder()
    b.numeral("팔", "8", "z.numeral")
    b.replace("１", "1", "a.fold")
    b.absorb("-", "m.separator", "separator")
    b.keep("9")
    _, smap = b.finish()

    src = smap.to_source(rpit.Span(0, 4, 0, 4))
    assert src.rules == ["a.fold", "m.separator", "z.numeral"]


# ── 합성 ────────────────────────────────────────────────────


def test_identity_compose_is_a_noop():
    ident = rpit.SpanMap.identity("880101-1234567")
    ident.validate()
    assert ident.is_identity() is True

    _, m2 = _rrn_builder().finish()
    composed = rpit.SpanMap.compose(ident, m2)
    composed.validate()
    assert composed.src_end == ident.src_end
    assert composed.dst_end == m2.dst_end

    src = composed.to_source(rpit.Span(0, 13, 0, 13))
    assert (src.byte_start, src.byte_end) == (0, 14)


def test_compose_rejects_mismatched_coordinate_systems():
    with pytest.raises(rpit.SpanMapError):
        rpit.SpanMap.compose(rpit.SpanMap.identity("abc"), rpit.SpanMap.identity("abcdef"))


# ── 내성과 직렬화 ────────────────────────────────────────────


def test_segment_introspection():
    _, smap = _rrn_builder().finish()
    assert [s.kind for s in smap.segments] == ["identity", "delete", "identity"]
    assert len(smap) == 3

    deleted = smap.segments[1]
    assert deleted.rules == ["separator.hyphen"]
    assert deleted.dst.is_empty() is True
    assert deleted.src.slice("880101-1234567") == "-"
    assert deleted.cost.absorbed_separators == 1


def test_json_export():
    _, smap = _rrn_builder().finish()
    payload = json.loads(smap.to_json())
    assert len(payload["segments"]) == len(smap.segments)

    evidence = json.loads(smap.to_source(rpit.Span(0, 13, 0, 13)).to_json())
    assert evidence["snapped"] is False
    assert evidence["rules"] == ["separator.hyphen"]
    assert evidence["cost"]["absorbed_separators"] == 1


def test_empty_text():
    smap = rpit.SpanMap.identity("")
    smap.validate()
    assert len(smap) == 0
    assert smap.dst_end == (0, 0)
    assert smap.to_source(rpit.Span(0, 0, 0, 0)).span.is_empty() is True


# ── 오류 처리: Rust 패닉이 Python 예외로 바뀌는가 ─────────────


def test_unknown_absorbed_class_raises_value_error():
    with pytest.raises(ValueError, match="unknown absorbed class"):
        rpit.SpanMapBuilder().absorb(" ", "x.rule", "SPACE")


def test_finish_twice_raises_instead_of_panicking():
    b = rpit.SpanMapBuilder()
    b.keep("abc")
    b.finish()
    with pytest.raises(RuntimeError, match="already consumed"):
        b.finish()


def test_use_after_finish_raises_instead_of_panicking():
    b = rpit.SpanMapBuilder()
    b.keep("abc")
    b.finish()
    with pytest.raises(RuntimeError, match="already consumed"):
        b.keep("def")


def test_out_of_range_slice_raises_instead_of_panicking():
    """코어의 Span::slice 는 범위 밖에서 패닉한다. 바인딩이 그것을 막아야 한다."""
    with pytest.raises(ValueError):
        rpit.Span(0, 999, 0, 999).slice("짧다")


def test_non_char_boundary_slice_raises_instead_of_panicking():
    with pytest.raises(ValueError):
        rpit.Span(1, 2, 0, 1).slice("가나")  # '가' 의 중간


def test_char_range_out_of_range_raises():
    with pytest.raises(ValueError):
        rpit.Span.from_char_range("abc", 0, 99)


def test_reversed_char_range_raises():
    with pytest.raises(ValueError):
        rpit.Span.from_char_range("abcdef", 4, 2)
