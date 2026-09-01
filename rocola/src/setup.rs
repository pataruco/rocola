//! First run: ask for the four things rocola can't discover on its own, and
//! write them to the config file.
//!
//! Deliberately plain stdin/stdout rather than ratatui: setup happens once,
//! and pasting a Client ID or a file path into a normal terminal line is
//! easier than into a TUI textbox.

use std::io::Write as _;
use std::path::PathBuf;

use crate::config::{AppleConfig, Config, SpotifyConfig};

const SPOTIFY_PROMPT: &str = "rocola needs a Spotify app of your own (free, ~2 minutes).\n  1. Open https://developer.spotify.com/dashboard and create an app.\n  2. Add this exact Redirect URI: http://127.0.0.1:8888/callback\n  3. Paste the app's Client ID here: ";
const TEAM_ID_PROMPT: &str = "Now your Apple pieces (needs a paid Apple Developer membership):\n  Team ID (developer.apple.com → Membership): ";
const KEY_ID_PROMPT: &str = "Key ID of your MusicKit key: ";
const P8_PROMPT: &str =
    "Path to your AuthKey_….p8 file (Apple lets you download it only once — keep it safe): ";
const GIT_WARNING: &str = "Warning: your .p8 key is inside a git repository. Move it somewhere private — anyone who can read that repo can impersonate your Apple developer account.";

/// Ask for the Spotify and Apple details, save them, and return the config.
///
/// # Errors
///
/// Returns an error if stdin closes before all four answers arrive, or if the
/// config file can't be written.
pub fn first_run_setup() -> anyhow::Result<Config> {
    let client_id = ask(SPOTIFY_PROMPT)?;
    let team_id = ask(TEAM_ID_PROMPT)?;
    let key_id = ask(KEY_ID_PROMPT)?;
    let p8_path = expand_home(&ask(P8_PROMPT)?);

    let config = Config {
        spotify: SpotifyConfig {
            client_id,
            refresh_token: None,
        },
        apple: AppleConfig {
            team_id,
            key_id,
            p8_path,
            music_user_token: None,
            storefront: None,
        },
    };

    let path = Config::default_path();
    config.save(&path)?;
    println!("Saved your settings to {}.", path.display());
    if config.p8_inside_git_worktree() {
        println!("{GIT_WARNING}");
    }
    Ok(config)
}

/// Print a prompt and read one non-blank line.
fn ask(prompt: &str) -> anyhow::Result<String> {
    loop {
        print!("{prompt}");
        std::io::stdout().flush()?;
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 {
            anyhow::bail!("Setup didn't finish. Run rocola again when you have those details.");
        }
        let answer = line.trim();
        if !answer.is_empty() {
            return Ok(answer.to_owned());
        }
        println!("That can't be blank — paste the value and press enter.");
    }
}

/// Expand a leading `~/` to the home directory.
///
/// The shell only expands `~` when the user types it as a bare word, not when
/// it arrives inside a prompt answer — and a literal `~` in the stored path
/// would silently defeat both the .p8 read and the git-worktree warning.
fn expand_home(input: &str) -> PathBuf {
    let Some(home) = dirs::home_dir() else {
        return PathBuf::from(input);
    };
    match input {
        "~" => home,
        _ => input
            .strip_prefix("~/")
            .map_or_else(|| PathBuf::from(input), |rest| home.join(rest)),
    }
}

#[cfg(test)]
mod tests {
    use super::expand_home;

    #[test]
    fn expands_a_leading_tilde_slash() {
        let home = dirs::home_dir().expect("a home directory");
        assert_eq!(
            expand_home("~/keys/AuthKey_X.p8"),
            home.join("keys/AuthKey_X.p8")
        );
        assert_eq!(expand_home("~"), home);
    }

    #[test]
    fn leaves_other_paths_alone() {
        assert_eq!(
            expand_home("/tmp/AuthKey_X.p8"),
            std::path::Path::new("/tmp/AuthKey_X.p8")
        );
        // Only a leading `~/` is a home reference; `~` mid-path is a real
        // directory name.
        assert_eq!(
            expand_home("keys/~/AuthKey_X.p8"),
            std::path::Path::new("keys/~/AuthKey_X.p8")
        );
    }
}
