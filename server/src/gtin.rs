// SPDX-FileCopyrightText: 2026 Joerg Bernau <Joerg@bernau.family>
// SPDX-License-Identifier: AGPL-3.0-or-later
//! Barcodes of albums (EAN-8, UPC-A, EAN-13, GTIN-14): cleaning, the check digit, and the common form
//! (GTIN-14) in which two spellings of one product compare equal.

/// The digits of a barcode as typed or tagged: spaces and hyphens are dropped. `None` if anything else is in it
/// (letters, other punctuation) or nothing is left.
pub fn clean(text: &str) -> Option<String> {
    let mut out = String::new();
    for c in text.chars() {
        match c {
            ' ' | '-' | '\t' | '\u{a0}' => {}
            '0'..='9' => out.push(c),
            _ => return None,
        }
    }
    (!out.is_empty()).then_some(out)
}

/// Is this (already cleaned) digit string a valid GTIN: 8, 12, 13 or 14 digits with a correct check digit?
pub fn valid(digits: &str) -> bool {
    if !matches!(digits.len(), 8 | 12 | 13 | 14) || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return false;
    }
    let nums: Vec<u32> = digits.bytes().map(|b| u32::from(b - b'0')).collect();
    let (body, check) = nums.split_at(nums.len() - 1);
    // From the right, the digit next to the check digit weighs 3, then 1, 3, ...
    let sum: u32 = body.iter().rev().enumerate().map(|(i, d)| if i % 2 == 0 { d * 3 } else { *d }).sum();
    (10 - sum % 10) % 10 == check[0]
}

/// Cleans and checks in one step: the digits of a valid barcode, else `None`.
pub fn parse(text: &str) -> Option<String> {
    clean(text).filter(|d| valid(d))
}

/// The 14-digit form (padded with zeros on the left), the same for an EAN-13 with a leading 0 and the UPC-A it comes from.
pub fn to_gtin14(digits: &str) -> String {
    format!("{digits:0>14}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn known_good_codes_are_valid() {
        for code in ["4006381333931", "036000291452", "96385074", "00036000291452", "9780201379624"] {
            assert!(valid(code), "{code}");
        }
    }

    #[test]
    fn bad_check_digit_or_length_is_refused() {
        for code in ["4006381333932", "036000291453", "96385075", "12345", "400638133393", "40063813339311", "", "40063813339a1"] {
            assert!(!valid(code), "{code}");
        }
    }

    #[test]
    fn clean_drops_spaces_and_hyphens_only() {
        assert_eq!(clean("4 006381-333931").as_deref(), Some("4006381333931"));
        assert_eq!(clean("4006381333931x"), None);
        assert_eq!(clean(" - "), None);
        assert_eq!(parse("400-6381 333931").as_deref(), Some("4006381333931"));
        assert_eq!(parse("4006381333932"), None);
    }

    #[test]
    fn spellings_of_one_product_share_the_gtin14() {
        assert_eq!(to_gtin14("036000291452"), "00036000291452");
        assert_eq!(to_gtin14("0036000291452"), "00036000291452");
        assert_eq!(to_gtin14("96385074"), "00000096385074");
        assert!(valid(&to_gtin14("96385074")));
    }
}
