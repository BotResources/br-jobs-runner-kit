use super::*;

fn nano(text: &str) -> u128 {
    parse(text).unwrap().0
}

#[test]
fn the_decimal_and_binary_suffixes_of_kubernetes_give_the_value() {
    assert_eq!(nano("1"), 1_000_000_000);
    assert_eq!(nano("500m"), 500_000_000);
    assert_eq!(nano("1500m"), nano("1.5"));
    assert_eq!(nano("2k"), 2_000 * 1_000_000_000);
    assert_eq!(nano("1Ki"), 1024 * 1_000_000_000);
    assert_eq!(nano("64Gi"), 64 * (1u128 << 30) * 1_000_000_000);
    assert_eq!(nano("1Ei"), (1u128 << 60) * 1_000_000_000);
    assert_eq!(nano("1G"), nano("1000M"));
    assert_eq!(nano("1E"), nano("1000P"));
    assert_eq!(nano("100n"), 100);
    assert_eq!(nano("100u"), 100_000);
}

#[test]
fn the_exponent_form_and_the_exa_suffix_are_two_different_things() {
    assert_eq!(nano("1e3"), nano("1k"));
    assert_eq!(nano("1E3"), nano("1k"));
    assert_eq!(nano("1E"), 10u128.pow(18) * 1_000_000_000);
    assert_eq!(nano("12e-1"), nano("1200m"));
    assert_eq!(nano("1e+2"), nano("100"));
}

#[test]
fn a_fraction_is_kept_and_a_value_under_a_nano_unit_rounds_up() {
    assert_eq!(nano("0.5"), 500_000_000);
    assert_eq!(nano(".5"), 500_000_000);
    assert_eq!(nano("1.25Gi"), 5 * (1u128 << 28) * 1_000_000_000);
    assert_eq!(nano("0.0000000001"), 1);
    assert_eq!(nano("1e-30"), 1);
}

#[test]
fn zero_in_any_form_is_zero() {
    for text in ["0", "0m", "0.0", "0Gi", "-0", "+0", "0e5"] {
        assert_eq!(nano(text), 0, "{text}");
    }
}

#[test]
fn a_negative_value_is_refused_and_a_plus_sign_is_allowed() {
    assert_eq!(parse("-1"), Err(Refusal::Negative));
    assert_eq!(parse("-500m"), Err(Refusal::Negative));
    assert_eq!(nano("+1"), nano("1"));
}

#[test]
fn a_text_that_is_not_a_quantity_is_refused() {
    for text in [
        "", " ", "abc", "1x", "1 Gi", "Gi", "1.2.3", "1..", ".", "1ki", "1gi", "1e", "1e+", "1ee3",
        "0x10", "1,5", "m", "1Mi ",
    ] {
        assert_eq!(parse(text), Err(Refusal::NotAQuantity), "{text:?}");
    }
}

#[test]
fn a_value_that_does_not_fit_is_refused_and_never_wraps() {
    assert_eq!(parse("1e100"), Err(Refusal::TooLarge));
    assert_eq!(parse("340282366920938463463374607431768211455"), Err(Refusal::TooLarge));
    assert_eq!(parse("1e99999999999"), Err(Refusal::TooLarge));
}

#[test]
fn values_compare_in_the_order_of_their_size() {
    assert!(parse("500m").unwrap() < parse("1").unwrap());
    assert!(parse("1Gi").unwrap() > parse("1G").unwrap());
    assert!(parse("1024Mi").unwrap() == parse("1Gi").unwrap());
}
