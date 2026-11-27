const NANO_EXPONENT: i32 = 9;
const MAX_POWER: u32 = 38;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) struct Nano(pub u128);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Refusal {
    NotAQuantity,
    Negative,
    TooLarge,
}

enum Suffix {
    Binary(u32),
    Decimal(i32),
}

fn split_number(text: &str) -> Option<(&str, &str)> {
    let end = text
        .find(|c: char| !(c.is_ascii_digit() || c == '.'))
        .unwrap_or(text.len());
    let (number, suffix) = text.split_at(end);
    let digits = number.chars().filter(char::is_ascii_digit).count();
    let dots = number.chars().filter(|c| *c == '.').count();
    (digits > 0 && dots <= 1).then_some((number, suffix))
}

fn suffix_of(text: &str) -> Option<Suffix> {
    let binary = ["Ki", "Mi", "Gi", "Ti", "Pi", "Ei"];
    if let Some(index) = binary.iter().position(|name| *name == text) {
        return Some(Suffix::Binary(10 * (index as u32 + 1)));
    }
    let decimal = [
        ("", 0),
        ("n", -9),
        ("u", -6),
        ("m", -3),
        ("k", 3),
        ("M", 6),
        ("G", 9),
        ("T", 12),
        ("P", 15),
        ("E", 18),
    ];
    if let Some((_, exponent)) = decimal.iter().find(|(name, _)| *name == text) {
        return Some(Suffix::Decimal(*exponent));
    }
    let exponent = text.strip_prefix(['e', 'E'])?;
    let digits = exponent.strip_prefix(['+', '-']).unwrap_or(exponent);
    if digits.is_empty() || !digits.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let exponent: i64 = exponent.parse().ok()?;
    Some(Suffix::Decimal(exponent.clamp(-1000, 1000) as i32))
}

fn power_of_ten(exponent: u32) -> Option<u128> {
    if exponent > MAX_POWER {
        return None;
    }
    Some(10u128.pow(exponent))
}

pub(super) fn parse(text: &str) -> Result<Nano, Refusal> {
    let (negative, unsigned) = match text.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, text.strip_prefix('+').unwrap_or(text)),
    };
    let (number, suffix) = split_number(unsigned).ok_or(Refusal::NotAQuantity)?;
    let suffix = suffix_of(suffix).ok_or(Refusal::NotAQuantity)?;
    let (whole, fraction) = number.split_once('.').unwrap_or((number, ""));
    let mantissa: u128 = format!("{whole}{fraction}")
        .parse()
        .map_err(|_| Refusal::TooLarge)?;
    let fraction_digits = i32::try_from(fraction.len()).map_err(|_| Refusal::TooLarge)?;
    if mantissa == 0 {
        return Ok(Nano(0));
    }
    if negative {
        return Err(Refusal::Negative);
    }
    let (power, binary) = match suffix {
        Suffix::Binary(bits) => (NANO_EXPONENT - fraction_digits, bits),
        Suffix::Decimal(exponent) => (
            exponent
                .checked_add(NANO_EXPONENT - fraction_digits)
                .ok_or(Refusal::TooLarge)?,
            0,
        ),
    };
    let scaled = mantissa
        .checked_mul(1u128.checked_shl(binary).ok_or(Refusal::TooLarge)?)
        .ok_or(Refusal::TooLarge)?;
    if power >= 0 {
        let factor = power_of_ten(power.unsigned_abs()).ok_or(Refusal::TooLarge)?;
        return scaled
            .checked_mul(factor)
            .map(Nano)
            .ok_or(Refusal::TooLarge);
    }
    match power_of_ten(power.unsigned_abs()) {
        Some(divisor) => Ok(Nano(scaled.div_ceil(divisor))),
        None => Ok(Nano(1)),
    }
}

#[cfg(test)]
mod tests;
