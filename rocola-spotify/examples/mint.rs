//! Mint the Spotify credentials for the hurl live checks.
//!
//! Usage: cargo run -p rocola-spotify --example mint -- <spotify client id>
//!
//! Opens your browser for the one-time Spotify sign-in, then prints the
//! lines to paste into tests/hurl/vars.env.

fn main() {
    let Some(client_id) = std::env::args().nth(1) else {
        eprintln!("Usage: cargo run -p rocola-spotify --example mint -- <spotify client id>");
        std::process::exit(2);
    };
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    match runtime.block_on(rocola_spotify::run_auth_flow(&client_id)) {
        Ok(tokens) => {
            println!("spotify_token={}", tokens.access_token);
            println!(
                "spotify_refresh_token={}",
                tokens.refresh_token.unwrap_or_default()
            );
            println!("spotify_client_id={client_id}");
        }
        Err(e) => {
            eprintln!("{e}");
            std::process::exit(1);
        }
    }
}
