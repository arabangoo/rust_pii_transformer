//! 정규화문에서 후보 구간을 뽑아내는 스캐너.
//!
//! 정규화 층이 이미 구분자를 흡수했으므로 숫자 계열 엔티티는 **숫자 런을 모으고 길이로
//! 분류하는 것**으로 환원된다. 정규 표현식보다 빠르고 결정적이며, 파국적 역추적이 원리적으로
//! 없다.
//!
//! ## 이메일도 손으로 스캔한다
//!
//! 설계 문서는 이메일처럼 구조가 복잡한 항목에 정규 표현식을 쓴다고 적었지만, 구현은
//! **정규 표현식 크레이트를 들이지 않고 직접 스캔한다.** `regex` 는 순수 Rust 라 외부 함수
//! 인터페이스 0 원칙을 깨지는 않지만 전이 의존성을 넷 이상 끌고 온다. 이메일 스캔은 골뱅이를
//! 찾아 양옆으로 넓히는 40줄짜리 일이라 그 비용을 낼 이유가 없다. 의존성은 `thiserror` 와
//! `serde` 둘로 유지된다.

use crate::span::Span;

/// 골뱅이 왼쪽에 올 수 있는 문자인가.
fn is_local_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '%' | '+' | '-')
}

/// 골뱅이 오른쪽에 올 수 있는 문자인가.
fn is_domain_part(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '.' | '-')
}

/// 연속한 아라비아 숫자 구간을 모두 찾는다.
///
/// 반환 스팬은 **정규화문 좌표계**다. 원문 좌표로 되돌리는 것은 호출자의 일이다.
pub fn digit_runs(text: &str) -> Vec<Span> {
    let mut runs = Vec::new();
    let mut start: Option<(usize, u32)> = None;
    let mut char_index = 0u32;

    for (byte_index, c) in text.char_indices() {
        if c.is_ascii_digit() {
            if start.is_none() {
                start = Some((byte_index, char_index));
            }
        } else if let Some((byte_start, char_start)) = start.take() {
            runs.push(Span::new(
                byte_start as u32..byte_index as u32,
                char_start..char_index,
            ));
        }
        char_index += 1;
    }

    if let Some((byte_start, char_start)) = start {
        runs.push(Span::new(
            byte_start as u32..text.len() as u32,
            char_start..char_index,
        ));
    }
    runs
}

/// 도메인 부분이 그럴듯한가.
///
/// 점이 하나 이상 있고, 마지막 조각이 두 글자 이상의 알파벳이며, 빈 조각이 없어야 한다.
fn plausible_domain(domain: &str) -> bool {
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 || labels.iter().any(|label| label.is_empty()) {
        return false;
    }
    let tld = labels[labels.len() - 1];
    tld.len() >= 2 && tld.chars().all(|c| c.is_ascii_alphabetic())
}

/// 이메일 주소로 보이는 구간을 모두 찾는다.
pub fn emails(text: &str) -> Vec<Span> {
    if !text.contains('@') {
        return Vec::new();
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    let mut found = Vec::new();
    let mut at = 0;

    while at < chars.len() {
        if chars[at].1 != '@' {
            at += 1;
            continue;
        }

        // 골뱅이 왼쪽으로 넓힌다.
        let mut start = at;
        while start > 0 && is_local_part(chars[start - 1].1) {
            start -= 1;
        }
        // 점으로 시작하거나 끝나는 로컬 파트는 주소가 아니다.
        while start < at && chars[start].1 == '.' {
            start += 1;
        }

        // 골뱅이 오른쪽으로 넓힌다.
        let mut end = at + 1;
        while end < chars.len() && is_domain_part(chars[end].1) {
            end += 1;
        }
        // 문장 끝의 마침표나 붙임표는 주소의 일부가 아니다.
        while end > at + 1 && matches!(chars[end - 1].1, '.' | '-') {
            end -= 1;
        }

        let local_ok = start < at && chars[at - 1].1 != '.';
        if local_ok && end > at + 1 {
            let domain: String = chars[at + 1..end].iter().map(|&(_, c)| c).collect();
            if plausible_domain(&domain) {
                let byte_start = chars[start].0;
                let byte_end = chars.get(end).map_or(text.len(), |&(byte, _)| byte);
                found.push(Span::new(
                    byte_start as u32..byte_end as u32,
                    start as u32..end as u32,
                ));
                at = end;
                continue;
            }
        }
        at += 1;
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slices<'a>(text: &'a str, spans: &[Span]) -> Vec<&'a str> {
        spans.iter().map(|s| s.slice(text)).collect()
    }

    #[test]
    fn finds_digit_runs() {
        let text = "주민 8801011234567 카드 4111111111111111 끝";
        let runs = digit_runs(text);
        assert_eq!(
            slices(text, &runs),
            vec!["8801011234567", "4111111111111111"]
        );
    }

    #[test]
    fn digit_run_at_the_very_end_is_closed() {
        let text = "번호 01012345678";
        let runs = digit_runs(text);
        assert_eq!(slices(text, &runs), vec!["01012345678"]);
    }

    #[test]
    fn digit_run_keeps_byte_and_char_offsets_separate() {
        // 한글 두 글자는 6바이트, 2문자다.
        let text = "번호 880101";
        let run = &digit_runs(text)[0];
        assert_eq!(run.byte.start, 7, "한글 2자 6바이트 + 공백 1바이트");
        assert_eq!(run.char.start, 3, "한글 2자 + 공백 1자");
        assert_eq!(run.slice(text), "880101");
    }

    #[test]
    fn no_runs_in_plain_text() {
        assert!(digit_runs("숫자가 하나도 없는 문장").is_empty());
    }

    #[test]
    fn finds_emails() {
        let text = "연락은 minsu.kim@example.com 또는 a_b+c@sub.example.co.kr 로";
        let spans = emails(text);
        assert_eq!(
            slices(text, &spans),
            vec!["minsu.kim@example.com", "a_b+c@sub.example.co.kr"]
        );
    }

    #[test]
    fn trims_sentence_punctuation_from_domain() {
        let text = "메일은 kim@example.com. 입니다";
        assert_eq!(slices(text, &emails(text)), vec!["kim@example.com"]);
    }

    #[test]
    fn rejects_shapes_that_are_not_addresses() {
        for text in [
            "@example.com",     // 로컬 파트 없음
            "kim@",             // 도메인 없음
            "kim@example",      // 점 없음
            "kim@example.c",    // 최상위 도메인이 한 글자
            "kim@example.12",   // 최상위 도메인이 숫자
            "kim@.com",         // 빈 라벨
            "가격은 100@200 원", // 골뱅이를 구분자로 쓴 문장
        ] {
            assert!(emails(text).is_empty(), "주소가 아닌데 잡혔다: {text}");
        }
    }

    #[test]
    fn no_at_sign_takes_the_fast_path() {
        assert!(emails("골뱅이가 없는 문장").is_empty());
    }
}
