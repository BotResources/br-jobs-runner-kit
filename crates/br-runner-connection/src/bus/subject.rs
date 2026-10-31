use crate::error::Error;

fn refuse(reason: &str) -> Error {
    Error::InvalidInput(format!("the bus subject {reason}"))
}

pub(super) fn check(subject: &str) -> Result<(), Error> {
    if subject.is_empty() {
        return Err(refuse("is empty"));
    }
    if subject.chars().any(char::is_whitespace) {
        return Err(refuse("holds a whitespace"));
    }
    for token in subject.split('.') {
        match token {
            "" => return Err(refuse("holds an empty token")),
            "*" | ">" => return Err(refuse("holds a wildcard")),
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::check;

    #[test]
    fn a_subject_with_a_wildcard_a_whitespace_or_an_empty_token_is_refused() {
        let refused = [
            "",
            "a b",
            "a\tb",
            "a\u{a0}b",
            "a\u{2003}b",
            "a\nb",
            "*",
            ">",
            "a.*",
            "a.>",
            "*.a",
            "a.*.b",
            ".a",
            "a.",
            "a..b",
            ".",
        ];
        for subject in refused {
            assert!(check(subject).is_err(), "{subject:?}");
        }
    }

    #[test]
    fn a_literal_subject_is_accepted() {
        for subject in ["a", "a.b.c", "a*b", "a>b", "a.b*", "Ünï.cöde", "a-b_c.1"] {
            assert!(check(subject).is_ok(), "{subject:?}");
        }
    }
}
