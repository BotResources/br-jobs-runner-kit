use std::fmt;

use crate::error::KeeperError;

pub const CONFIG_VARIABLE: &str = "BR_SANDBOX_CONFIG";
pub const ACCESS_KEY_VARIABLE: &str = "AWS_ACCESS_KEY_ID";
pub const SECRET_KEY_VARIABLE: &str = "AWS_SECRET_ACCESS_KEY";
pub const POD_NAMESPACE_FILE: &str = "/var/run/secrets/kubernetes.io/serviceaccount/namespace";

#[derive(Clone, PartialEq, Eq)]
pub struct StoreCredential {
    access_key_id: String,
    secret_access_key: String,
}

impl fmt::Debug for StoreCredential {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("StoreCredential(redacted)")
    }
}

fn required(
    lookup: &impl Fn(&str) -> Option<String>,
    name: &'static str,
) -> Result<String, KeeperError> {
    lookup(name)
        .filter(|value| !value.is_empty())
        .ok_or_else(|| KeeperError::Environment {
            name,
            reason: "not set".to_owned(),
        })
}

impl StoreCredential {
    pub fn from_lookup(lookup: impl Fn(&str) -> Option<String>) -> Result<Self, KeeperError> {
        Ok(Self {
            access_key_id: required(&lookup, ACCESS_KEY_VARIABLE)?,
            secret_access_key: required(&lookup, SECRET_KEY_VARIABLE)?,
        })
    }

    pub fn access_key_id(&self) -> &str {
        &self.access_key_id
    }

    pub fn secret_access_key(&self) -> &str {
        &self.secret_access_key
    }
}

pub fn config_path(lookup: impl Fn(&str) -> Option<String>) -> Result<String, KeeperError> {
    required(&lookup, CONFIG_VARIABLE)
}

pub fn own_namespace(file_text: Option<String>) -> Option<String> {
    file_text
        .map(|text| text.trim().to_owned())
        .filter(|text| !text.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn lookup<'a>(pairs: &'a [(&'a str, &'a str)]) -> impl Fn(&str) -> Option<String> + 'a {
        move |name| {
            pairs
                .iter()
                .find(|(key, _)| *key == name)
                .map(|(_, value)| (*value).to_owned())
        }
    }

    #[test]
    fn the_credential_comes_from_the_two_variables() {
        let credential = StoreCredential::from_lookup(lookup(&[
            (ACCESS_KEY_VARIABLE, "id"),
            (SECRET_KEY_VARIABLE, "secret"),
        ]))
        .unwrap();
        assert_eq!(credential.access_key_id(), "id");
        assert_eq!(credential.secret_access_key(), "secret");
    }

    #[test]
    fn a_missing_or_empty_variable_is_named_in_the_error() {
        let only_secret = [(SECRET_KEY_VARIABLE, "secret")];
        let empty_secret = [(ACCESS_KEY_VARIABLE, "id"), (SECRET_KEY_VARIABLE, "")];
        for pairs in [&only_secret[..], &empty_secret[..]] {
            let error = StoreCredential::from_lookup(lookup(pairs)).unwrap_err();
            assert!(matches!(error, KeeperError::Environment { .. }));
        }
        let error =
            StoreCredential::from_lookup(lookup(&[(SECRET_KEY_VARIABLE, "s")])).unwrap_err();
        assert!(error.to_string().contains(ACCESS_KEY_VARIABLE));
    }

    #[test]
    fn the_credential_is_never_shown_by_debug_or_by_an_error() {
        let credential = StoreCredential::from_lookup(lookup(&[
            (ACCESS_KEY_VARIABLE, "AKIDVALUE"),
            (SECRET_KEY_VARIABLE, "SECRETVALUE"),
        ]))
        .unwrap();
        let shown = format!("{credential:?}");
        assert!(!shown.contains("AKIDVALUE") && !shown.contains("SECRETVALUE"));
    }

    #[test]
    fn the_config_path_is_required() {
        assert_eq!(
            config_path(lookup(&[(CONFIG_VARIABLE, "/etc/keeper.json")])).unwrap(),
            "/etc/keeper.json"
        );
        assert!(config_path(lookup(&[])).is_err());
    }

    #[test]
    fn the_own_namespace_is_the_trimmed_text_of_the_file_when_it_has_one() {
        assert_eq!(
            own_namespace(Some("sbx-system\n".to_owned())),
            Some("sbx-system".to_owned())
        );
        assert_eq!(own_namespace(Some("\n".to_owned())), None);
        assert_eq!(own_namespace(None), None);
    }
}
