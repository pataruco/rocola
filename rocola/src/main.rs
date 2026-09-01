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
        eprintln!("Example: rocola https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M");
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
