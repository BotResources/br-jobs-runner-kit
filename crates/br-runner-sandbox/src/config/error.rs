#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ConfigError {
    #[error("the key {key} is blank")]
    Blank { key: &'static str },
    #[error("the key {key} holds a slash")]
    HoldsSlash { key: &'static str },
    #[error("the key {key} is not above zero")]
    NotAboveZero { key: &'static str },
    #[error("the key allowlist is empty in the mode allow")]
    AllowlistRequired,
    #[error("the key allowlist is not empty in the mode {mode}")]
    AllowlistForbidden { mode: &'static str },
    #[error("a site of the key allowlist is blank")]
    BlankSite,
    #[error("the key {key} holds {value:?}, which is not an absolute path")]
    NotAbsolute { key: &'static str, value: String },
    #[error("the key {key} holds {value:?}, which has a parent component or a NUL")]
    PathEscapes { key: &'static str, value: String },
    #[error("the key save_scope_paths holds {outer} and {inner}, and the first holds the second")]
    ScopeNested { outer: String, inner: String },
    #[error("the key save_scope_paths does not hold /work")]
    ScopeWithoutWork,
    #[error(
        "the key seed_root holds {seed_root} and the save scope path {scope_path} holds the other or is the same"
    )]
    SeedRootInScope {
        seed_root: String,
        scope_path: String,
    },
    #[error("the key save_zstd_level is {level}, and zstd accepts 1 to 22")]
    ZstdLevel { level: i32 },
    #[error("the key claim_max_attempts is below 1")]
    ClaimAttempts,
    #[error("the key store_endpoint is not a URL with the scheme http or https")]
    StoreEndpoint,
    #[error("the key store_credentials holds a blank text")]
    BlankCredentials,
}
