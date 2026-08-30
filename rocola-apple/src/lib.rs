//! Apple Music target: developer token, Music User Token, catalog resolve
//! and playlist write.

pub mod dev_token;

pub use dev_token::mint_developer_token;

#[derive(Debug, thiserror::Error)]
pub enum AppleError {
    #[error("{0}")]
    BadKey(String),
    #[error("Apple Music request failed: {0}. Check your connection and try again.")]
    Http(String),
    #[error("Apple Music sign-in problem: {0}")]
    Auth(String),
    #[error("this song isn't available in your country's Apple Music catalog")]
    NotInStorefront,
}
