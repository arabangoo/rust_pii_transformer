//! 합성 검증 코퍼스.
//!
//! 실제 주민등록번호를 테스트에 넣을 수 없다. 그래서 **검증식이 유효한 합성 데이터 생성기**를
//! 함께 만든다. 이것은 부산물이 아니라 독립적 가치가 있는 모듈이고, 변형 표기 회귀 테스트셋의
//! 기반이다. 재현율과 정밀도를 수치로 낼 수 있는 근거가 여기서 나온다.
//!
//! ## 검증식을 흉내 내지 않는다
//!
//! 검증 자릿수는 계산식을 다시 구현하지 않고 [`crate::detect::checksum`] 의 **판정기를 그대로
//! 돌려** 마지막 자리를 0부터 9까지 시험해 찾는다. 생성기와 판정기가 어긋날 수 없는 구조다.
//! 계산식을 양쪽에 두 번 적으면 한쪽만 고쳤을 때 코퍼스가 조용히 거짓말을 하게 된다.
//!
//! ## 결정적이다
//!
//! 씨앗을 주면 항상 같은 코퍼스가 나온다. 외부 난수 크레이트를 쓰지 않고 선형 합동 생성기를
//! 직접 둔 이유이기도 하다. 실패한 회귀는 씨앗만 있으면 그대로 재현된다.
//!
//! ```
//! use rust_pii_transformer::synth::{corpus, Sample};
//!
//! let a = corpus(42, 1);
//! let b = corpus(42, 1);
//! assert_eq!(a, b, "같은 씨앗은 같은 코퍼스를 낸다");
//! assert!(a.iter().any(|s: &Sample| s.expected.is_some()));
//! assert!(a.iter().any(|s: &Sample| s.expected.is_none()));
//! ```

use std::ops::Range;

use serde::Serialize;

use crate::detect::checksum;
use crate::detect::EntityKind;

// ── 난수 ────────────────────────────────────────────────────

/// 결정적 선형 합동 생성기.
///
/// 외부 크레이트를 쓰지 않는 것은 기본 빌드의 의존성 약속을 코퍼스 생성기에도 그대로 적용하기
/// 때문이다. 통계적 품질이 필요한 용도가 아니라 재현 가능한 표본 추출이 필요한 용도다.
#[derive(Debug, Clone)]
pub struct Rng(u64);

impl Rng {
    /// 씨앗으로 생성기를 만든다.
    pub fn new(seed: u64) -> Self {
        // 씨앗 0 이 고정점이 되지 않도록 한 번 섞는다.
        Self(seed ^ 0x9e37_79b9_7f4a_7c15)
    }

    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        self.0 >> 33
    }

    /// `0..n` 범위의 값 하나.
    pub fn below(&mut self, n: u64) -> usize {
        (self.next() % n.max(1)) as usize
    }

    /// `0..=9` 숫자 하나.
    fn digit(&mut self) -> u8 {
        self.below(10) as u8
    }

    /// 슬라이스에서 하나 고른다.
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len() as u64)]
    }
}

// ── 표본 ────────────────────────────────────────────────────

/// 표기 변형. 이 라이브러리가 존재하는 이유가 이 목록이다.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum Variant {
    /// 숫자만 붙여 쓴다. `8801011234568`
    Plain,
    /// 하이픈으로 끊는다. `880101-1234568`
    Hyphen,
    /// 공백으로 끊는다. `880101 1234568`
    Space,
    /// 전각 숫자. `８８０１０１１２３４５６８`
    Fullwidth,
    /// 앞 여섯 자리를 한글 자릿수 읽기로. `팔팔공일공일-1234568`
    HangulDigits,
    /// 숫자가 아닌 값. 이메일처럼 변형이 없는 엔티티에 쓴다.
    Literal,
}

impl Variant {
    /// 보고서에 쓸 이름.
    pub fn label(&self) -> &'static str {
        match self {
            Variant::Plain => "숫자만",
            Variant::Hyphen => "하이픈",
            Variant::Space => "공백",
            Variant::Fullwidth => "전각",
            Variant::HangulDigits => "한글수사",
            Variant::Literal => "원형",
        }
    }
}

/// 표본 하나.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Sample {
    /// 문장 전체. 탐지기에 이대로 넣는다.
    pub text: String,
    /// 정답. 음성 표본이면 `None`.
    pub expected: Option<EntityKind>,
    /// 문장 안에서 값이 차지하는 바이트 구간. 정답 스팬이다.
    pub span: Range<u32>,
    /// 어떤 표기로 적혔는가.
    pub variant: Variant,
    /// 문맥 단서를 붙였는가.
    pub with_context: bool,
    /// 음성 표본이면 어떤 함정인가.
    pub trap: Option<&'static str>,
}

// ── 값 생성 ─────────────────────────────────────────────────

/// 마지막 자리를 0부터 9까지 시험해 판정기를 통과하는 값을 찾는다.
///
/// 계산식을 다시 구현하지 않는 것이 요점이다. 판정기가 곧 정답의 정의다.
fn complete<F>(prefix: &mut Vec<u8>, passes: F) -> bool
where
    F: Fn(&[u8]) -> bool,
{
    prefix.push(0);
    let last = prefix.len() - 1;
    for candidate in 0..=9u8 {
        prefix[last] = candidate;
        if passes(prefix) {
            return true;
        }
    }
    prefix.pop();
    false
}

fn to_text(digits: &[u8]) -> String {
    digits.iter().map(|d| char::from(b'0' + d)).collect()
}

/// 실재하는 날짜 여섯 자리(yymmdd).
fn birth_six(rng: &mut Rng) -> Vec<u8> {
    let yy = rng.below(100);
    let mm = 1 + rng.below(12);
    // 28일까지만 써서 달 길이와 윤년을 신경 쓰지 않는다.
    let dd = 1 + rng.below(28);
    vec![
        (yy / 10) as u8,
        (yy % 10) as u8,
        (mm / 10) as u8,
        (mm % 10) as u8,
        (dd / 10) as u8,
        (dd % 10) as u8,
    ]
}

/// 검증식을 통과하는 주민등록번호 또는 외국인등록번호 13자리.
fn resident(rng: &mut Rng, foreigner: bool) -> String {
    loop {
        let mut digits = birth_six(rng);
        digits.push(if foreigner {
            (5 + rng.below(4)) as u8
        } else {
            (1 + rng.below(4)) as u8
        });
        for _ in 0..5 {
            digits.push(rng.digit());
        }
        let ok = complete(&mut digits, |d| {
            checksum::analyze_resident(d).is_some_and(|a| a.checksum.passed() && a.birth.is_some())
        });
        if ok {
            return to_text(&digits);
        }
    }
}

/// 검증식을 통과하는 사업자등록번호 10자리.
fn business(rng: &mut Rng) -> String {
    loop {
        let mut digits: Vec<u8> = (0..9).map(|_| rng.digit()).collect();
        if complete(&mut digits, |d| checksum::business_registration(d).passed()) {
            return to_text(&digits);
        }
    }
}

/// Luhn 을 통과하는 카드번호 16자리.
fn card(rng: &mut Rng) -> String {
    loop {
        let mut digits: Vec<u8> = std::iter::once(4).chain((0..14).map(|_| rng.digit())).collect();
        if complete(&mut digits, |d| checksum::luhn(d).passed()) {
            return to_text(&digits);
        }
    }
}

/// 휴대전화번호 11자리.
fn phone(rng: &mut Rng) -> String {
    let mut digits = vec![0, 1, 0];
    for _ in 0..8 {
        digits.push(rng.digit());
    }
    to_text(&digits)
}

/// 계좌번호. 은행마다 체계가 달라 자릿수만 흉내 낸다.
fn account(rng: &mut Rng) -> String {
    let len = 11 + rng.below(4);
    to_text(&(0..len).map(|_| rng.digit()).collect::<Vec<_>>())
}

/// 운전면허번호 12자리. 앞 두 자리는 지방경찰청 코드다.
fn license(rng: &mut Rng) -> String {
    const REGIONS: [&str; 6] = ["11", "12", "13", "21", "26", "28"];
    let region = rng.pick(&REGIONS);
    let rest: String = (0..10).map(|_| char::from(b'0' + rng.digit())).collect();
    format!("{region}{rest}")
}

/// 여덟 자리 생년월일.
fn birth_eight(rng: &mut Rng) -> String {
    let year = 1940 + rng.below(70);
    let month = 1 + rng.below(12);
    let day = 1 + rng.below(28);
    format!("{year:04}{month:02}{day:02}")
}

/// 이메일 주소.
fn email(rng: &mut Rng) -> String {
    const NAMES: [&str; 6] = ["minsu.kim", "jiwon", "hyeon_lee", "sara.park", "dohoon", "yuna99"];
    const HOSTS: [&str; 4] = ["example.com", "mail.example.net", "corp.example.co.kr", "test.example.org"];
    format!("{}@{}", rng.pick(&NAMES), rng.pick(&HOSTS))
}

// ── 표기 변형 ───────────────────────────────────────────────

/// 엔티티별 구분자 위치. 앞에서부터 센 문자 수다.
fn group_positions(entity: EntityKind, len: usize) -> Vec<usize> {
    match entity {
        EntityKind::Resident | EntityKind::ForeignerRegistration => vec![6],
        EntityKind::BusinessRegistration => vec![3, 5],
        EntityKind::CreditCard => vec![4, 8, 12],
        EntityKind::Phone => vec![3, 7],
        EntityKind::DriverLicense => vec![2, 4, 10],
        EntityKind::BirthDate => vec![4, 6],
        EntityKind::BankAccount => {
            if len >= 12 {
                vec![3, 9]
            } else {
                vec![3, 8]
            }
        }
        EntityKind::Email => Vec::new(),
    }
}

fn insert_separator(value: &str, positions: &[usize], sep: char) -> String {
    let mut out = String::with_capacity(value.len() + positions.len());
    for (index, ch) in value.chars().enumerate() {
        if positions.contains(&index) && index > 0 {
            out.push(sep);
        }
        out.push(ch);
    }
    out
}

fn to_fullwidth(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_digit() {
                // U+FF10 이 전각 0 이다.
                char::from_u32(0xff10 + (ch as u32 - '0' as u32)).unwrap_or(ch)
            } else {
                ch
            }
        })
        .collect()
}

/// 앞 여섯 자리를 한글 자릿수 읽기로 바꾼다. `880101` 이 `팔팔공일공일` 이 된다.
fn to_hangul_digits(value: &str, count: usize) -> String {
    const SYLLABLES: [char; 10] = ['공', '일', '이', '삼', '사', '오', '육', '칠', '팔', '구'];
    let mut out = String::new();
    for (index, ch) in value.chars().enumerate() {
        if index < count {
            match ch.to_digit(10) {
                Some(d) => out.push(SYLLABLES[d as usize]),
                None => out.push(ch),
            }
        } else {
            out.push(ch);
        }
    }
    out
}

/// 값을 표기 변형에 맞춰 렌더링한다.
fn render(entity: EntityKind, value: &str, variant: Variant) -> String {
    let positions = group_positions(entity, value.chars().count());
    match variant {
        Variant::Plain | Variant::Literal => value.to_string(),
        Variant::Hyphen => insert_separator(value, &positions, '-'),
        Variant::Space => insert_separator(value, &positions, ' '),
        Variant::Fullwidth => to_fullwidth(value),
        Variant::HangulDigits => {
            let hangul = to_hangul_digits(value, 6);
            // 한글 여섯 음절 뒤에 하이픈을 넣어 숫자 표지에 붙인다.
            let (head, tail) = hangul.split_at(hangul.char_indices().nth(6).map_or(hangul.len(), |(i, _)| i));
            format!("{head}-{tail}")
        }
    }
}

// ── 문장 조립 ───────────────────────────────────────────────

/// 문맥 단서를 담은 문장 틀. `{}` 자리에 값이 들어간다.
fn with_cue(entity: EntityKind) -> &'static str {
    match entity {
        EntityKind::Resident => "주민등록번호 {} 입니다",
        EntityKind::ForeignerRegistration => "외국인등록번호 {} 확인했습니다",
        EntityKind::BusinessRegistration => "사업자등록번호 {} 로 세금계산서 발행 부탁드립니다",
        EntityKind::CreditCard => "카드번호 {} 로 결제했습니다",
        EntityKind::BankAccount => "입금 계좌번호 {} 로 송금 바랍니다",
        EntityKind::Phone => "연락처는 {} 입니다",
        EntityKind::Email => "메일은 {} 로 보내주세요",
        EntityKind::DriverLicense => "운전면허번호 {} 확인 부탁드립니다",
        EntityKind::BirthDate => "생년월일 {} 로 등록해 주세요",
    }
}

/// 단서가 없는 문장 틀. 어떤 엔티티의 단서 사전에도 없는 낱말만 쓴다.
const WITHOUT_CUE: &str = "첨부 자료 확인 부탁드립니다. {} 참고하시면 됩니다.";

fn assemble(template: &str, rendered: &str) -> (String, Range<u32>) {
    let at = template.find("{}").expect("문장 틀에는 자리가 하나 있다");
    let text = template.replacen("{}", rendered, 1);
    let start = at as u32;
    (text, start..start + rendered.len() as u32)
}

// ── 코퍼스 ──────────────────────────────────────────────────

/// 이 엔티티가 문맥 단서 없이도 성립하는가.
///
/// 문맥이 필수인 엔티티는 단서 없는 양성 표본을 만들지 않는다. 그런 표본은 정답이 모호하다.
/// 텍스트에는 개인정보가 들어 있지만 이 라이브러리는 설계상 그것을 내지 않기로 했고,
/// 그 판단은 재현율의 손실이 아니라 오탐 억제의 대가이기 때문이다.
fn works_without_context(entity: EntityKind) -> bool {
    !matches!(
        entity,
        EntityKind::BankAccount | EntityKind::DriverLicense | EntityKind::BirthDate
    )
}

/// 엔티티별로 시험할 표기 변형.
fn variants_for(entity: EntityKind) -> &'static [Variant] {
    match entity {
        EntityKind::Resident | EntityKind::ForeignerRegistration => &[
            Variant::Plain,
            Variant::Hyphen,
            Variant::Space,
            Variant::Fullwidth,
            Variant::HangulDigits,
        ],
        EntityKind::CreditCard => {
            &[Variant::Plain, Variant::Hyphen, Variant::Space, Variant::Fullwidth]
        }
        EntityKind::BusinessRegistration | EntityKind::Phone => {
            &[Variant::Plain, Variant::Hyphen, Variant::Fullwidth]
        }
        EntityKind::BankAccount | EntityKind::DriverLicense | EntityKind::BirthDate => {
            &[Variant::Plain, Variant::Hyphen]
        }
        EntityKind::Email => &[Variant::Literal],
    }
}

/// 이 코퍼스가 다루는 엔티티.
pub const ENTITIES: &[EntityKind] = &[
    EntityKind::Resident,
    EntityKind::ForeignerRegistration,
    EntityKind::BusinessRegistration,
    EntityKind::CreditCard,
    EntityKind::Phone,
    EntityKind::Email,
    EntityKind::BankAccount,
    EntityKind::DriverLicense,
    EntityKind::BirthDate,
];

fn value_for(rng: &mut Rng, entity: EntityKind) -> String {
    match entity {
        EntityKind::Resident => resident(rng, false),
        EntityKind::ForeignerRegistration => resident(rng, true),
        EntityKind::BusinessRegistration => business(rng),
        EntityKind::CreditCard => card(rng),
        EntityKind::Phone => phone(rng),
        EntityKind::Email => email(rng),
        EntityKind::BankAccount => account(rng),
        EntityKind::DriverLicense => license(rng),
        EntityKind::BirthDate => birth_eight(rng),
    }
}

/// 양성 표본 하나.
pub fn positive(rng: &mut Rng, entity: EntityKind, variant: Variant, with_context: bool) -> Sample {
    let value = value_for(rng, entity);
    let rendered = render(entity, &value, variant);
    let template = if with_context { with_cue(entity) } else { WITHOUT_CUE };
    let (text, span) = assemble(template, &rendered);
    Sample { text, expected: Some(entity), span, variant, with_context, trap: None }
}

/// 오탐을 유도하는 음성 표본. 개인정보가 들어 있지 않다.
///
/// **통과하기 쉽게 고르지 않았다.** 자릿수가 겹치는 업무 번호를 그대로 넣는다. 13자리 운송장
/// 번호가 우연히 Luhn 을 통과하면 그것은 이 라이브러리의 진짜 오탐이고, 그 확률까지 수치에
/// 반영되는 것이 맞다.
pub fn negative(rng: &mut Rng) -> Sample {
    let choice = rng.below(7);
    let (text, trap) = match choice {
        0 => (
            format!("주문번호 {}{} 로 접수되었습니다", 2 + rng.below(8), to_text(&(0..10).map(|_| rng.digit()).collect::<Vec<_>>())),
            "11자리 주문번호",
        ),
        1 => (
            format!("송장번호 {} 입니다", to_text(&(0..12).map(|_| rng.digit()).collect::<Vec<_>>())),
            "12자리 송장번호",
        ),
        2 => (
            format!("운송장번호 {} 조회 결과입니다", to_text(&(0..13).map(|_| rng.digit()).collect::<Vec<_>>())),
            "13자리 운송장번호 (Luhn 우연 통과 가능)",
        ),
        3 => (
            format!(
                "제품 코드 {} 수량 {} 개",
                to_text(&(0..4).map(|_| rng.digit()).collect::<Vec<_>>()),
                to_text(&(0..4).map(|_| rng.digit()).collect::<Vec<_>>())
            ),
            "짧은 숫자 두 개",
        ),
        4 => (
            format!("총 금액은 {} 원입니다", to_text(&(0..7).map(|_| rng.digit()).collect::<Vec<_>>())),
            "금액",
        ),
        5 => (
            String::from("이사 갑니다. 사구 팔구 방식으로 진행하고 구이 정식을 주문했습니다."),
            "수사 음절로 이루어진 일상어",
        ),
        _ => (
            format!("회원번호 {} 등급 조회 결과", to_text(&(0..9).map(|_| rng.digit()).collect::<Vec<_>>())),
            "9자리 회원번호",
        ),
    };
    let len = text.len() as u32;
    Sample { text, expected: None, span: 0..len, variant: Variant::Plain, with_context: false, trap: Some(trap) }
}

/// 회귀 검증용 코퍼스를 만든다.
///
/// `rounds` 는 엔티티별 반복 횟수다. 한 라운드마다 엔티티별로 모든 표기 변형을 한 번씩,
/// 그리고 음성 표본을 여러 건 만든다.
pub fn corpus(seed: u64, rounds: usize) -> Vec<Sample> {
    let mut rng = Rng::new(seed);
    let mut out = Vec::new();

    for _ in 0..rounds {
        for &entity in ENTITIES {
            for &variant in variants_for(entity) {
                out.push(positive(&mut rng, entity, variant, true));
            }
            if works_without_context(entity) {
                for &variant in variants_for(entity) {
                    out.push(positive(&mut rng, entity, variant, false));
                }
            }
        }
        for _ in 0..12 {
            out.push(negative(&mut rng));
        }
    }

    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_resident_numbers_pass_the_real_validator() {
        let mut rng = Rng::new(1);
        for _ in 0..200 {
            let value = resident(&mut rng, false);
            let digits = checksum::to_digits(&value).unwrap();
            let analysis = checksum::analyze_resident(&digits).expect("13자리여야 한다");
            assert!(analysis.checksum.passed(), "{value} 가 검증식을 통과하지 못했다");
            assert!(analysis.birth.is_some(), "{value} 의 앞자리가 실재 날짜가 아니다");
            assert!(!analysis.gender.foreigner);
        }
    }

    #[test]
    fn generated_foreigner_numbers_use_the_foreigner_gender_range() {
        let mut rng = Rng::new(2);
        for _ in 0..200 {
            let value = resident(&mut rng, true);
            let digits = checksum::to_digits(&value).unwrap();
            let analysis = checksum::analyze_resident(&digits).unwrap();
            assert!(analysis.checksum.passed());
            assert!(analysis.gender.foreigner);
        }
    }

    #[test]
    fn generated_business_numbers_pass_the_real_validator() {
        let mut rng = Rng::new(3);
        for _ in 0..200 {
            let value = business(&mut rng);
            let digits = checksum::to_digits(&value).unwrap();
            assert!(checksum::business_registration(&digits).passed(), "{value}");
        }
    }

    #[test]
    fn generated_cards_pass_luhn() {
        let mut rng = Rng::new(4);
        for _ in 0..200 {
            let value = card(&mut rng);
            assert_eq!(value.len(), 16);
            let digits = checksum::to_digits(&value).unwrap();
            assert!(checksum::luhn(&digits).passed(), "{value}");
        }
    }

    #[test]
    fn variants_render_the_same_value_differently() {
        let value = "8801011234568";
        assert_eq!(render(EntityKind::Resident, value, Variant::Plain), "8801011234568");
        assert_eq!(render(EntityKind::Resident, value, Variant::Hyphen), "880101-1234568");
        assert_eq!(render(EntityKind::Resident, value, Variant::Space), "880101 1234568");
        assert_eq!(
            render(EntityKind::Resident, value, Variant::Fullwidth),
            "８８０１０１１２３４５６８"
        );
        assert_eq!(
            render(EntityKind::Resident, value, Variant::HangulDigits),
            "팔팔공일공일-1234568"
        );
    }

    #[test]
    fn the_gold_span_points_at_the_value() {
        let mut rng = Rng::new(5);
        for &entity in ENTITIES {
            for &variant in variants_for(entity) {
                let sample = positive(&mut rng, entity, variant, true);
                let slice = &sample.text[sample.span.start as usize..sample.span.end as usize];
                assert!(
                    sample.text.contains(slice),
                    "{entity:?} {variant:?} 의 정답 스팬이 문장을 벗어났다"
                );
                assert!(!slice.is_empty());
                assert!(
                    !slice.starts_with(' ') && !slice.ends_with(' '),
                    "{entity:?} {variant:?}: 정답 스팬에 앞뒤 공백이 붙었다"
                );
            }
        }
    }

    #[test]
    fn the_corpus_is_deterministic() {
        assert_eq!(corpus(7, 2), corpus(7, 2));
        assert_ne!(corpus(7, 2), corpus(8, 2));
    }

    #[test]
    fn the_corpus_has_both_polarities() {
        let samples = corpus(9, 1);
        let positives = samples.iter().filter(|s| s.expected.is_some()).count();
        let negatives = samples.iter().filter(|s| s.expected.is_none()).count();
        assert!(positives > 20, "양성이 {positives} 건뿐이다");
        assert!(negatives >= 12, "음성이 {negatives} 건뿐이다");
    }

    #[test]
    fn negatives_contain_no_planted_value() {
        // 음성 표본은 정말로 개인정보가 없어야 한다. 정답 스팬은 문장 전체를 가리킨다.
        let mut rng = Rng::new(11);
        for _ in 0..50 {
            let sample = negative(&mut rng);
            assert!(sample.expected.is_none());
            assert!(sample.trap.is_some());
            assert_eq!(sample.span, 0..sample.text.len() as u32);
        }
    }
}
