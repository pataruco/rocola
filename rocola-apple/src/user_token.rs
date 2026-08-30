use std::sync::Arc;

use axum::extract::State;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use tokio::sync::mpsc;

use crate::AppleError;

fn render_page(developer_token: &str) -> String {
    include_str!("musickit.html").replace("__DEV_TOKEN__", developer_token)
}

#[derive(Deserialize)]
struct TokenPost {
    #[serde(rename = "userToken")]
    user_token: String,
}

/// Serve the `MusicKit` page on 127.0.0.1:8889, open the browser, and wait
/// (max 5 minutes) for the page to post back the Music User Token.
///
/// # Errors
///
/// Returns [`AppleError::Auth`] when port 8889 is already in use, when the
/// browser doesn't post a token within 5 minutes, or when the sign-in page
/// closes before posting one.
pub async fn run_user_auth(developer_token: &str) -> Result<String, AppleError> {
    let (tx, mut rx) = mpsc::channel::<String>(1);
    let page = Arc::new(render_page(developer_token));

    let app = Router::new()
        .route(
            "/",
            get({
                let page = Arc::clone(&page);
                move || async move { axum::response::Html(page.as_ref().clone()) }
            }),
        )
        .route(
            "/token",
            post(
                |State(tx): State<mpsc::Sender<String>>, Json(body): Json<TokenPost>| async move {
                    let _ = tx.send(body.user_token).await;
                    "ok"
                },
            ),
        )
        .with_state(tx);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:8889")
        .await
        .map_err(|_| {
            AppleError::Auth(
                "port 8889 is in use. Close the other program using it and try again.".into(),
            )
        })?;
    let server = tokio::spawn(async move {
        let _ = axum::serve(listener, app).await;
    });

    if open::that("http://127.0.0.1:8889/").is_err() {
        eprintln!("Open this link in your browser to connect Apple Music:\nhttp://127.0.0.1:8889/");
    }

    let received = tokio::time::timeout(std::time::Duration::from_secs(300), rx.recv()).await;
    server.abort();

    let token = received
        .map_err(|_| {
            AppleError::Auth(
                "the Apple Music sign-in didn't finish within 5 minutes. Run rocola again to retry."
                    .into(),
            )
        })?
        .ok_or_else(|| {
            AppleError::Auth("the sign-in page closed early. Run rocola again to retry.".into())
        })?;
    Ok(token)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn page_embeds_the_developer_token_and_posts_back() {
        let page = render_page("DEVTOKEN123");
        assert!(page.contains("developerToken: 'DEVTOKEN123'"));
        assert!(!page.contains("__DEV_TOKEN__"));
        assert!(page.contains("fetch('/token'"));
    }
}
