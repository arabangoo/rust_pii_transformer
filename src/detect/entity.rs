//! 엔티티별 인식기.
//!
//! 숫자 런 하나를 받아 **어떤 엔티티일 수 있는지** 후보를 낸다. 한 런이 여러 후보를 낳는 것이
//! 정상이다. 열 자리 숫자열은 사업자등록번호일 수도, 지역 전화번호일 수도, 계좌번호일 수도
//! 있다. 어느 쪽인지는 검증식과 문맥이 정한다.
//!
//! 이 모듈은 판정하지 않는다. 후보와 그 후보의 검증 결과, 그리고 **그 엔티티가 도달할 수 있는
//! 최고 등급**까지만 낸다. 점수 계산과 최종 판정은 [`super`] 의 오케스트레이션이 한다.

use serde::Serialize;

use super::checksum::{self, ChecksumResult};
use super::context::{self, Cue};

/// 이 라이브러리가 다루는 개인정보 종류.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum EntityKind {
    /// 주민등록번호.
    Resident,
    /// 외국인등록번호.
    ForeignerRegistration,
    /// 사업자등록번호.
    BusinessRegistration,
    /// 신용카드 또는 체크카드 번호.
    CreditCard,
    /// 은행 계좌번호.
    BankAccount,
    /// 전화번호.
    Phone,
    /// 이메일 주소.
    Email,
    /// 운전면허번호.
    DriverLicense,
    /// 생년월일.
    BirthDate,
}

impl EntityKind {
    /// 사람이 읽는 이름. 마스킹 자리표시자에 쓴다.
    pub fn label(&self) -> &'static str {
        match self {
            EntityKind::Resident => "주민등록번호",
            EntityKind::ForeignerRegistration => "외국인등록번호",
            EntityKind::BusinessRegistration => "사업자등록번호",
            EntityKind::CreditCard => "카드번호",
            EntityKind::BankAccount => "계좌번호",
            EntityKind::Phone => "전화번호",
            EntityKind::Email => "이메일",
            EntityKind::DriverLicense => "운전면허번호",
            EntityKind::BirthDate => "생년월일",
        }
    }
}

/// 판정 등급.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Certainty {
    /// 형식만 맞다. 문맥 단서가 없다.
    Possible,
    /// 형식이 맞고 문맥 단서가 충분하다. 또는 검증식이 없는 엔티티다.
    Probable,
    /// 검증식을 통과했다.
    Certain,
}

/// 검증 후보 하나.
#[derive(Debug, Clone)]
pub struct Candidate {
    /// 무슨 엔티티로 본 것인가.
    pub entity: EntityKind,
    /// 어떤 패턴 규칙이 걸렸는가. 판정 근거에 그대로 실린다.
    pub rule: &'static str,
    /// 검증식 결과.
    pub checksum: ChecksumResult,
    /// 이 엔티티가 도달할 수 있는 최고 등급.
    ///
    /// 검증식이 없거나 공개돼 있지 않은 엔티티는 아무리 문맥이 좋아도 `Certain` 이 될 수 없다.
    /// 그 사실을 감추지 않기 위해 타입으로 못박는다.
    pub ceiling: Certainty,
    /// 이 엔티티의 문맥 단서 사전.
    pub cues: &'static [Cue],
    /// 패턴 기본점.
    pub base: f32,
    /// 문맥 단서 없이도 성립하는가.
    ///
    /// 카드번호처럼 검증식이 강한 엔티티는 문맥이 없어도 된다. 계좌번호처럼 숫자열만으로는
    /// 아무것도 말할 수 없는 엔티티는 문맥이 필수다.
    pub needs_context: bool,
}

/// 서울 지역번호를 뺀 나머지 지역번호. 앞의 `0` 을 포함한 세 자리다.
const AREA_CODES: &[&str] = &[
    "031", "032", "033", "041", "042", "043", "044", "051", "052", "053", "054", "055", "061",
    "062", "063", "064", "070",
];

/// 휴대전화 식별번호.
const MOBILE_PREFIXES: &[&str] = &["010", "011", "016", "017", "018", "019"];

/// 대표번호 식별번호.
const REPRESENTATIVE_PREFIXES: &[&str] = &["15", "16", "18"];

/// 운전면허 지역 코드. 최초 발급 지방경찰청을 뜻한다.
const LICENSE_REGIONS: &[&str] = &[
    "11", "12", "13", "14", "15", "16", "17", "18", "19", "20", "21", "22", "23", "24", "25", "26",
    "28",
];

/// 숫자 런을 문자열로 되돌린다. 접두 비교에 쓴다.
fn as_text(digits: &[u8]) -> String {
    digits.iter().map(|d| char::from(b'0' + d)).collect()
}

/// 숫자 런 하나에서 가능한 모든 후보를 낸다.
pub fn candidates(digits: &[u8]) -> Vec<Candidate> {
    let mut out = Vec::new();
    let text = as_text(digits);
    let len = digits.len();

    // 주민등록번호와 외국인등록번호. 성별코드가 둘을 가른다.
    if let Some(analysis) = checksum::analyze_resident(digits) {
        // 앞 6자리가 실재하는 날짜가 아니면 등록번호가 아니다. 이것만은 검증식보다 강한 제약이다.
        if analysis.birth.is_some() {
            let (entity, rule, cues) = if analysis.gender.foreigner {
                (
                    EntityKind::ForeignerRegistration,
                    "resident.foreigner_13",
                    context::FOREIGNER,
                )
            } else {
                (EntityKind::Resident, "resident.korean_13", context::RESIDENT)
            };
            out.push(Candidate {
                entity,
                rule,
                checksum: analysis.checksum,
                ceiling: Certainty::Certain,
                cues,
                base: 0.6,
                needs_context: false,
            });
        }
    }

    // 사업자등록번호.
    if len == 10 {
        out.push(Candidate {
            entity: EntityKind::BusinessRegistration,
            rule: "business.10",
            checksum: checksum::business_registration(digits),
            ceiling: Certainty::Certain,
            cues: context::BUSINESS,
            base: 0.4,
            needs_context: false,
        });
    }

    // 카드번호. 국제 표준 검증식이라 이 라이브러리에서 가장 확실한 엔티티다.
    if (13..=19).contains(&len) {
        out.push(Candidate {
            entity: EntityKind::CreditCard,
            rule: "card.luhn",
            checksum: checksum::luhn(digits),
            ceiling: Certainty::Certain,
            cues: context::CARD,
            base: 0.4,
            needs_context: false,
        });
    }

    // 전화번호. 식별번호로 형태를 가른다.
    if let Some(rule) = phone_rule(&text) {
        out.push(Candidate {
            entity: EntityKind::Phone,
            rule,
            checksum: ChecksumResult::NotApplicable("전화번호에는 검증식이 없다"),
            ceiling: Certainty::Probable,
            cues: context::PHONE,
            base: 0.5,
            needs_context: false,
        });
    }

    // 운전면허번호. 검증 자릿수가 존재하지만 계산식이 공개돼 있지 않아 형식만 본다.
    if len == 12 && LICENSE_REGIONS.contains(&&text[..2]) {
        out.push(Candidate {
            entity: EntityKind::DriverLicense,
            rule: "license.12",
            checksum: ChecksumResult::NotApplicable(
                "검증 자릿수는 존재하나 계산식이 공개돼 있지 않다",
            ),
            ceiling: Certainty::Probable,
            cues: context::DRIVER_LICENSE,
            base: 0.2,
            needs_context: true,
        });
    }

    // 계좌번호. 숫자열만으로는 아무것도 말할 수 없어 문맥이 필수다.
    if (10..=14).contains(&len) {
        out.push(Candidate {
            entity: EntityKind::BankAccount,
            rule: "account.length",
            checksum: ChecksumResult::NotApplicable(
                "은행별 체계가 달라 공통 검증식이 없다",
            ),
            ceiling: Certainty::Probable,
            cues: context::ACCOUNT,
            base: 0.1,
            needs_context: true,
        });
    }

    // 생년월일. 일반 날짜와 구분되지 않으므로 문맥이 필수다.
    if let Some(rule) = birth_date_rule(digits) {
        out.push(Candidate {
            entity: EntityKind::BirthDate,
            rule,
            checksum: ChecksumResult::NotApplicable("생년월일에는 검증식이 없다"),
            ceiling: Certainty::Probable,
            cues: context::BIRTH_DATE,
            base: 0.1,
            needs_context: true,
        });
    }

    out
}

/// 전화번호 형태를 가른다. 아니면 `None`.
fn phone_rule(text: &str) -> Option<&'static str> {
    let len = text.len();

    if len == 11 && MOBILE_PREFIXES.contains(&&text[..3]) {
        return Some("phone.mobile_11");
    }
    if len == 10 && MOBILE_PREFIXES[1..].contains(&&text[..3]) {
        // 010 은 여덟 자리 가입번호가 없다. 011 계열의 옛 번호만 열 자리다.
        return Some("phone.mobile_10");
    }
    if (9..=10).contains(&len) && text.starts_with("02") {
        return Some("phone.seoul");
    }
    if (10..=11).contains(&len) && AREA_CODES.contains(&&text[..3]) {
        return Some("phone.landline");
    }
    if len == 8 && REPRESENTATIVE_PREFIXES.contains(&&text[..2]) {
        return Some("phone.representative");
    }
    None
}

/// 생년월일 형태를 가른다. 여섯 자리와 여덟 자리를 본다.
fn birth_date_rule(digits: &[u8]) -> Option<&'static str> {
    match digits.len() {
        6 => {
            let yy = u16::from(digits[0]) * 10 + u16::from(digits[1]);
            let month = digits[2] * 10 + digits[3];
            let day = digits[4] * 10 + digits[5];
            // 두 자리 연도는 세기를 알 수 없다. 윤년 판정에 영향을 주는 2월 29일만 조심하면
            // 되는데, 1900년대와 2000년대 어느 쪽으로도 읽힐 수 있으므로 관대하게 본다.
            (checksum::is_valid_date(1900 + yy, month, day)
                || checksum::is_valid_date(2000 + yy, month, day))
            .then_some("birth.yymmdd")
        }
        8 => {
            let year = u16::from(digits[0]) * 1000
                + u16::from(digits[1]) * 100
                + u16::from(digits[2]) * 10
                + u16::from(digits[3]);
            let month = digits[4] * 10 + digits[5];
            let day = digits[6] * 10 + digits[7];
            ((1800..=2200).contains(&year) && checksum::is_valid_date(year, month, day))
                .then_some("birth.yyyymmdd")
        }
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn digits(s: &str) -> Vec<u8> {
        checksum::to_digits(s).unwrap()
    }

    fn kinds(s: &str) -> Vec<EntityKind> {
        candidates(&digits(s)).iter().map(|c| c.entity).collect()
    }

    #[test]
    fn thirteen_digits_with_valid_date_is_a_resident_number() {
        let kinds = kinds("8801011234567");
        assert!(kinds.contains(&EntityKind::Resident));
        assert!(!kinds.contains(&EntityKind::ForeignerRegistration));
    }

    #[test]
    fn gender_code_five_makes_it_a_foreigner_number() {
        let kinds = kinds("8801015234567");
        assert!(kinds.contains(&EntityKind::ForeignerRegistration));
        assert!(!kinds.contains(&EntityKind::Resident));
    }

    #[test]
    fn thirteen_digits_with_impossible_date_is_not_a_resident_number() {
        // 13월 32일. 검증식 이전에 날짜가 아니다.
        let kinds = kinds("8813321234567");
        assert!(!kinds.contains(&EntityKind::Resident));
        assert!(!kinds.contains(&EntityKind::ForeignerRegistration));
    }

    #[test]
    fn thirteen_digits_can_still_be_a_card() {
        // 같은 런이 여러 후보를 낳는 것이 정상이다.
        assert!(kinds("8801011234567").contains(&EntityKind::CreditCard));
    }

    #[test]
    fn ten_digits_yield_business_account_and_maybe_phone() {
        let kinds = kinds("1248100998");
        assert!(kinds.contains(&EntityKind::BusinessRegistration));
        assert!(kinds.contains(&EntityKind::BankAccount));
    }

    #[test]
    fn business_checksum_is_carried_on_the_candidate() {
        let cs = candidates(&digits("1248100998"));
        let business = cs
            .iter()
            .find(|c| c.entity == EntityKind::BusinessRegistration)
            .unwrap();
        assert!(business.checksum.passed());
        assert_eq!(business.ceiling, Certainty::Certain);
    }

    #[test]
    fn phone_shapes_are_classified() {
        assert_eq!(phone_rule("01012345678"), Some("phone.mobile_11"));
        assert_eq!(phone_rule("0111234567"), Some("phone.mobile_10"));
        assert_eq!(phone_rule("0212345678"), Some("phone.seoul"));
        assert_eq!(phone_rule("021234567"), Some("phone.seoul"));
        assert_eq!(phone_rule("0311234567"), Some("phone.landline"));
        assert_eq!(phone_rule("07012345678"), Some("phone.landline"));
        assert_eq!(phone_rule("15881234"), Some("phone.representative"));
    }

    #[test]
    fn non_phone_shapes_are_rejected() {
        assert_eq!(phone_rule("0101234567"), None, "010 은 열 자리가 없다");
        assert_eq!(phone_rule("0991234567"), None, "099 는 지역번호가 아니다");
        assert_eq!(phone_rule("12345678"), None);
    }

    #[test]
    fn driver_license_needs_a_known_region_code() {
        assert!(kinds("111234567890").contains(&EntityKind::DriverLicense));
        assert!(!kinds("991234567890").contains(&EntityKind::DriverLicense));
    }

    #[test]
    fn driver_license_can_never_reach_certain() {
        let cs = candidates(&digits("111234567890"));
        let license = cs
            .iter()
            .find(|c| c.entity == EntityKind::DriverLicense)
            .unwrap();
        assert_eq!(license.ceiling, Certainty::Probable);
        assert!(matches!(license.checksum, ChecksumResult::NotApplicable(_)));
    }

    #[test]
    fn account_and_license_need_context() {
        for (number, entity) in [
            ("1234567890123", EntityKind::BankAccount),
            ("111234567890", EntityKind::DriverLicense),
        ] {
            let cs = candidates(&digits(number));
            let found = cs.iter().find(|c| c.entity == entity).unwrap();
            assert!(found.needs_context, "{} 는 문맥이 필수다", entity.label());
        }
    }

    #[test]
    fn birth_dates_are_recognized() {
        assert_eq!(birth_date_rule(&digits("880101")), Some("birth.yymmdd"));
        assert_eq!(birth_date_rule(&digits("19880101")), Some("birth.yyyymmdd"));
        assert_eq!(birth_date_rule(&digits("881301")), None, "13월은 없다");
        assert_eq!(birth_date_rule(&digits("880230")), None, "2월 30일은 없다");
        assert_eq!(birth_date_rule(&digits("000229")), Some("birth.yymmdd"), "2000년은 윤년");
    }

    #[test]
    fn plain_number_yields_only_weak_candidates() {
        // 주문번호처럼 아무 의미 없는 열두 자리.
        let cs = candidates(&digits("987654321098"));
        assert!(
            cs.iter().all(|c| c.needs_context || c.ceiling == Certainty::Probable),
            "약한 후보만 나와야 한다"
        );
    }
}
