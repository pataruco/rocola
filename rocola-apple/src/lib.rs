//! Apple Music target: developer token, Music User Token, catalog resolve
//! and playlist write.

pub mod api_types;
pub mod client;
pub mod dev_token;
pub mod target;
pub mod user_token;

pub use client::{AddFailure, AppleClient};
pub use dev_token::mint_developer_token;
pub use target::AppleTarget;
pub use user_token::run_user_auth;

#[derive(Debug, thiserror::Error)]
pub enum AppleError {
    #[error("{0}")]
    BadKey(String),
    #[error("Apple Music request failed: {0}. Check your connection and try again.")]
    Http(String),
    #[error("Apple Music is asking rocola to slow down. Wait a minute, then run rocola again.")]
    RateLimited,
    #[error("Apple Music sign-in problem: {0}")]
    Auth(String),
    #[error(
        "Apple Music didn't say which country's catalogue to search. Check your Apple Music \
         subscription is active, then run rocola again."
    )]
    NotInStorefront,
}
