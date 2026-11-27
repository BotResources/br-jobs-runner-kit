const DAY_SECONDS: u32 = 86_400;
const FORM: &str = "a time of day is HH:MM or HH:MM:SS, with two digits for each part";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum End {
    Start,
    End,
}

pub(super) fn parse(text: &str, position: End) -> Result<u32, &'static str> {
    let parts: Vec<&str> = text.split(':').collect();
    if !(2..=3).contains(&parts.len()) {
        return Err(FORM);
    }
    let mut numbers = [0u32; 3];
    for (slot, part) in numbers.iter_mut().zip(&parts) {
        if part.len() != 2 || !part.bytes().all(|byte| byte.is_ascii_digit()) {
            return Err(FORM);
        }
        *slot = part.parse().map_err(|_| FORM)?;
    }
    let [hours, minutes, seconds] = numbers;
    if minutes > 59 || seconds > 59 {
        return Err("minutes and seconds are below 60");
    }
    let total = hours * 3600 + minutes * 60 + seconds;
    if total == DAY_SECONDS && position == End::End {
        return Ok(total);
    }
    if hours > 23 {
        return Err("hours are below 24; 24:00 may end a window and cannot begin one");
    }
    Ok(total)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_forms_of_a_time_of_day_give_the_second_of_the_day() {
        assert_eq!(parse("00:00", End::Start), Ok(0));
        assert_eq!(parse("09:30", End::Start), Ok(9 * 3600 + 1800));
        assert_eq!(parse("23:59:59", End::Start), Ok(86_399));
    }

    #[test]
    fn only_the_end_of_a_window_may_be_24_00() {
        assert_eq!(parse("24:00", End::End), Ok(86_400));
        assert_eq!(parse("24:00:00", End::End), Ok(86_400));
        assert!(parse("24:00", End::Start).is_err());
        assert!(parse("24:00:01", End::End).is_err());
        assert!(parse("25:00", End::End).is_err());
    }

    #[test]
    fn a_text_that_is_not_two_digits_for_each_part_is_refused() {
        for text in ["9:00", "10:61", "10:00:60", "10", "ten", "10:5", "+1:00", "1:2:3:4", ""] {
            assert!(parse(text, End::End).is_err(), "{text}");
        }
    }
}
