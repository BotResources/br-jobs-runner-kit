use std::future::Future;
use std::pin::Pin;

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

pub struct Token(String);

impl Token {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    pub(crate) fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(redacted)")
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum CredentialKind {
    NoUsable,
    FetchFailed,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
#[error("{}: {text}", match .kind {
    CredentialKind::NoUsable => "no usable credential",
    CredentialKind::FetchFailed => "credential fetch failed",
})]
pub struct CredentialFailure {
    pub kind: CredentialKind,
    pub text: String,
}

impl CredentialFailure {
    pub fn no_usable(text: impl Into<String>) -> Self {
        Self {
            kind: CredentialKind::NoUsable,
            text: text.into(),
        }
    }

    pub fn fetch_failed(text: impl Into<String>) -> Self {
        Self {
            kind: CredentialKind::FetchFailed,
            text: text.into(),
        }
    }
}

pub trait Credential: Send + Sync {
    fn token(&self) -> BoxFuture<'_, Result<Token, CredentialFailure>>;
}

pub struct FixedToken {
    value: String,
}

impl FixedToken {
    pub fn new(token: impl Into<String>) -> Self {
        Self {
            value: token.into(),
        }
    }
}

impl std::fmt::Debug for FixedToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("FixedToken(redacted)")
    }
}

impl Credential for FixedToken {
    fn token(&self) -> BoxFuture<'_, Result<Token, CredentialFailure>> {
        Box::pin(async move { Ok(Token::new(self.value.clone())) })
    }
}
