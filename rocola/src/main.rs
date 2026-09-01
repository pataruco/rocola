mod app;
mod config;
mod run;
mod setup;
mod ui;

use std::process::ExitCode;

#[tokio::main]
async fn main() -> ExitCode {
    let Some(url) = std::env::args().nth(1) else {
        eprintln!("Usage: rocola <spotify playlist url>");
        eprintln!("Example: rocola https://open.spotify.com/playlist/<id>");
        eprintln!(
            "It has to be a playlist you own or collaborate on. In Spotify: Share → Copy link to playlist."
        );
        return ExitCode::from(2);
    };
    match run::run(&url).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{e}");
            ExitCode::FAILURE
        }
    }
}
