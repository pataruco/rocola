//! The whole flow, in order: config, Spotify, Apple, match, review, write.

use std::io::Write as _;
use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use crossterm::terminal::{EnterAlternateScreen, LeaveAlternateScreen};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use rocola_apple::client::AppleClient;
use rocola_apple::{AppleError, AppleTarget, mint_developer_token, run_user_auth};
use rocola_core::{Candidate, MusicTarget, SourceTrack, TrackMatch, match_tracks};
use rocola_spotify::{Playlist, fetch_playlist, parse_playlist_url, refresh, run_auth_flow};

use crate::app::{App, Decision, Key, Screen};
use crate::config::Config;
use crate::{setup, ui};

const PLAYLIST_DESCRIPTION: &str = "Recreated from Spotify by rocola";

/// Heading for the tracks Spotify has, but Apple Music never can.
const UNMATCHABLE_HEADING: &str = "Local files and removed tracks:";

/// Recreate the playlist at `url` on Apple Music.
///
/// # Errors
///
/// Returns the first failure from setup, either service, or the terminal —
/// every one of them already phrased for the person who has to fix it.
pub async fn run(url: &str) -> anyhow::Result<()> {
    let config_path = Config::default_path();
    let mut config = match Config::load(&config_path)? {
        Some(config) => config,
        None => setup::first_run_setup()?,
    };

    // Pure and instant, so it runs before the sign-in: a mistyped link should
    // say so straight away, not after a trip through the browser.
    let playlist = parse_playlist_url(url)?;
    let access_token = spotify_token(&mut config, &config_path).await?;
    let Playlist {
        name: playlist_name,
        tracks,
        skipped: unmatchable,
    } = fetch_playlist(&access_token, &playlist).await?;
    println!(
        "Read \"{playlist_name}\" — {n} from Spotify.",
        n = ui::songs(tracks.len() + unmatchable.len())
    );
    if !unmatchable.is_empty() {
        println!(
            "Leaving out {n} — local files and tracks Spotify has removed aren't in Apple Music's catalogue. They're named at the end.",
            n = ui::songs(unmatchable.len())
        );
    }

    let developer_token = apple_developer_token(&config, &config_path)?;
    // A stored Music User Token can have expired since the last run. The
    // first Apple call of this run is where that shows up, and it is worth
    // exactly one silent re-sign-in.
    let mut stored_token_untested = config.apple.music_user_token.is_some();
    let user_token = match config.apple.music_user_token.clone() {
        Some(token) => token,
        None => save_user_token(&mut config, &config_path, &developer_token).await?,
    };
    let client = AppleClient::new(developer_token.clone(), user_token);
    let mut target = AppleTarget {
        storefront: String::new(),
        client,
    };
    target.storefront = if let Some(storefront) = config.apple.storefront.clone() {
        storefront
    } else {
        let storefront = match target.client.storefront().await {
            Err(AppleError::Auth(_)) if stored_token_untested => {
                target.client =
                    reconnect_apple(&mut config, &config_path, &developer_token).await?;
                stored_token_untested = false;
                target.client.storefront().await?
            }
            other => other?,
        };
        config.apple.storefront = Some(storefront.clone());
        config.save(&config_path)?;
        storefront
    };

    // Scoped so `Progress` — and with it the borrow of `target` — is dropped
    // before the retry branch can replace the client.
    let first_pass = { match_tracks(&Progress::new(&target, tracks.len()), &tracks).await };
    let matches = match first_pass {
        Err(AppleError::Auth(_)) if stored_token_untested => {
            target.client = reconnect_apple(&mut config, &config_path, &developer_token).await?;
            match_tracks(&Progress::new(&target, tracks.len()), &tracks).await?
        }
        other => other?,
    };

    let app = review(App::from_matches(playlist_name, matches))?;
    if matches!(app.screen, Screen::Aborted) {
        println!("Nothing was created.");
        return Ok(());
    }
    // The write phase gets its own one-shot re-sign-in, independent of the
    // one matching may already have spent.
    let mut session = WriteSession {
        client: target.client,
        developer_token,
        config: &mut config,
        config_path: &config_path,
        retry_available: true,
    };
    write_playlist(&mut session, &app, &unmatchable).await
}

/// A fresh access token, and a refresh token in the config for next time.
async fn spotify_token(config: &mut Config, config_path: &Path) -> anyhow::Result<String> {
    let tokens = match config.spotify.refresh_token.as_deref() {
        Some(refresh_token) => refresh(&config.spotify.client_id, refresh_token).await?,
        None => run_auth_flow(&config.spotify.client_id).await?,
    };
    // Spotify doesn't rotate the refresh token on use, but it is free to
    // start: persist whatever came back the moment it arrives, so a crash
    // later in the run never costs the sign-in.
    if tokens.refresh_token.is_some() && tokens.refresh_token != config.spotify.refresh_token {
        config
            .spotify
            .refresh_token
            .clone_from(&tokens.refresh_token);
        config.save(config_path)?;
    }
    Ok(tokens.access_token)
}

fn apple_developer_token(config: &Config, config_path: &Path) -> anyhow::Result<String> {
    let p8_path = &config.apple.p8_path;
    let p8_pem = std::fs::read_to_string(p8_path).map_err(|e| {
        anyhow::anyhow!(
            "Couldn't read your Apple key at {p8}: {e}. Fix apple.p8_path in {config_path} and run rocola again.",
            p8 = p8_path.display(),
            config_path = config_path.display()
        )
    })?;
    Ok(mint_developer_token(
        &p8_pem,
        &config.apple.team_id,
        &config.apple.key_id,
    )?)
}

/// Sign in to Apple Music and keep the token for next time.
async fn save_user_token(
    config: &mut Config,
    config_path: &Path,
    developer_token: &str,
) -> anyhow::Result<String> {
    let user_token = run_user_auth(developer_token).await?;
    config.apple.music_user_token = Some(user_token.clone());
    config.save(config_path)?;
    Ok(user_token)
}

/// Throw away a Music User Token Apple has stopped accepting and sign in again.
async fn reconnect_apple(
    config: &mut Config,
    config_path: &Path,
    developer_token: &str,
) -> anyhow::Result<AppleClient> {
    println!("Your Apple Music sign-in has expired. Signing you in again.");
    config.apple.music_user_token = None;
    let user_token = save_user_token(config, config_path, developer_token).await?;
    Ok(AppleClient::new(developer_token.to_owned(), user_token))
}

/// Wraps the real target to count tracks the pipeline has finished with.
///
/// The trait is the only view of the work in flight, so a track counts as
/// done when its ISRC came back from the batch, or when its search returns.
struct Progress<'a> {
    inner: &'a AppleTarget,
    total: usize,
    done: AtomicUsize,
}

impl<'a> Progress<'a> {
    const fn new(inner: &'a AppleTarget, total: usize) -> Self {
        Self {
            inner,
            total,
            done: AtomicUsize::new(0),
        }
    }

    fn advance(&self, by: usize) {
        let done = (self.done.fetch_add(by, Ordering::Relaxed) + by).min(self.total);
        eprint!("\rMatching {done}/{total}…", total = self.total);
        let _ = std::io::stderr().flush();
    }
}

impl Drop for Progress<'_> {
    fn drop(&mut self) {
        eprintln!();
    }
}

impl MusicTarget for Progress<'_> {
    type Error = AppleError;

    async fn resolve_by_isrc(
        &self,
        isrcs: &[String],
    ) -> Result<Vec<(String, Candidate)>, AppleError> {
        let pairs = self.inner.resolve_by_isrc(isrcs).await?;
        let resolved = isrcs
            .iter()
            .filter(|isrc| pairs.iter().any(|(found, _)| found == *isrc))
            .count();
        self.advance(resolved);
        Ok(pairs)
    }

    async fn search(&self, track: &SourceTrack) -> Result<Vec<Candidate>, AppleError> {
        let candidates = self.inner.search(track).await?;
        self.advance(1);
        Ok(candidates)
    }
}

/// Raw mode and the alternate screen, undone however this scope ends —
/// including a panic, which must never leave the shell unusable.
struct TerminalGuard;

impl TerminalGuard {
    fn enter() -> std::io::Result<Self> {
        crossterm::terminal::enable_raw_mode()?;
        crossterm::execute!(std::io::stdout(), EnterAlternateScreen)?;
        Ok(Self)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let _ = crossterm::terminal::disable_raw_mode();
        let _ = crossterm::execute!(std::io::stdout(), LeaveAlternateScreen);
    }
}

/// Run the review/confirm screens until the app is done or aborted.
fn review(mut app: App) -> anyhow::Result<App> {
    let _guard = TerminalGuard::enter()?;
    let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout()))?;
    loop {
        terminal.draw(|frame| ui::draw(frame, &app))?;
        if matches!(app.screen, Screen::Done | Screen::Aborted) {
            return Ok(app);
        }
        if let Event::Key(pressed) = event::read()?
            && pressed.kind == KeyEventKind::Press
            && let Some(key) = to_key(pressed.code)
        {
            app.on_key(key);
        }
    }
}

const fn to_key(code: KeyCode) -> Option<Key> {
    match code {
        KeyCode::Up => Some(Key::Up),
        KeyCode::Down => Some(Key::Down),
        KeyCode::Enter => Some(Key::Confirm),
        KeyCode::Esc => Some(Key::Abort),
        KeyCode::Char(c) => match c {
            '1'..='9' => Some(Key::Digit(c as u8 - b'0')),
            's' => Some(Key::Skip),
            'A' => Some(Key::AcceptAllHigh),
            'q' => Some(Key::Abort),
            _ => None,
        },
        _ => None,
    }
}

/// The write phase's Apple client, plus everything it needs to sign in again
/// once if Apple stops accepting the stored Music User Token part-way
/// through — which it can do between the last read and the first write.
struct WriteSession<'a> {
    client: AppleClient,
    developer_token: String,
    config: &'a mut Config,
    config_path: &'a Path,
    /// One re-sign-in for the whole write phase, never more.
    retry_available: bool,
}

impl WriteSession<'_> {
    /// Spend the phase's single re-sign-in.
    async fn reconnect(&mut self) -> anyhow::Result<()> {
        self.retry_available = false;
        self.client = reconnect_apple(self.config, self.config_path, &self.developer_token).await?;
        Ok(())
    }

    /// True when this failure is a rejected sign-in and the retry is unspent.
    const fn can_retry<T>(&self, result: &Result<T, AppleError>) -> bool {
        matches!(result, Err(AppleError::Auth(_))) && self.retry_available
    }

    async fn list_playlist_names(&mut self) -> anyhow::Result<Vec<String>> {
        let first = self.client.list_playlist_names().await;
        if !self.can_retry(&first) {
            return Ok(first?);
        }
        self.reconnect().await?;
        Ok(self.client.list_playlist_names().await?)
    }

    async fn create_playlist(&mut self, name: &str) -> anyhow::Result<String> {
        let first = self
            .client
            .create_playlist(name, PLAYLIST_DESCRIPTION)
            .await;
        if !self.can_retry(&first) {
            return Ok(first?);
        }
        // Nothing was created: Apple rejected the request before writing, so
        // the retry cannot leave a second playlist behind.
        self.reconnect().await?;
        Ok(self
            .client
            .create_playlist(name, PLAYLIST_DESCRIPTION)
            .await?)
    }

    /// Add the songs to the playlist that already exists. A retry here reuses
    /// `playlist_id` — re-signing in must never create a second playlist.
    async fn add_tracks(
        &mut self,
        name: &str,
        playlist_id: &str,
        catalog_ids: &[String],
    ) -> anyhow::Result<()> {
        let first = self.client.add_tracks(playlist_id, catalog_ids).await;
        if !self.can_retry(&first) {
            return first.map_err(|e| orphaned(name, &e));
        }
        self.reconnect().await.map_err(|e| orphaned(name, &e))?;
        self.client
            .add_tracks(playlist_id, catalog_ids)
            .await
            .map_err(|e| orphaned(name, &e))
    }
}

/// The playlist exists but is empty. Say so, and say what to do about it —
/// silence here leaves an unexplained playlist and a baffling duplicate-run
/// prompt on the next run.
fn orphaned(name: &str, e: &impl std::fmt::Display) -> anyhow::Error {
    anyhow::anyhow!(
        "Created \"{name}\" but couldn't add the songs: {e}. Delete it in Music.app and run rocola again."
    )
}

/// Create the playlist, add the accepted songs, and account for every track
/// that didn't make it.
async fn write_playlist(
    session: &mut WriteSession<'_>,
    app: &App,
    unmatchable: &[String],
) -> anyhow::Result<()> {
    let Some(name) = playlist_name(session, &app.playlist_name).await? else {
        println!("Nothing was created.");
        return Ok(());
    };

    let catalog_ids = app.accepted_catalog_ids();
    let playlist_id = session.create_playlist(&name).await?;
    if !catalog_ids.is_empty() {
        session
            .add_tracks(&name, &playlist_id, &catalog_ids)
            .await?;
    }
    println!(
        "Created \"{name}\" — {n} added.",
        n = ui::songs(catalog_ids.len())
    );

    report(
        "Skipped by you:",
        app.decided
            .iter()
            .filter(|item| matches!(item.decision, Decision::Skipped))
            .map(|item| track_line(&item.track)),
    );
    report(
        "Not found on Apple Music:",
        app.not_found.iter().map(track_line),
    );
    report(UNMATCHABLE_HEADING, unmatchable.iter().cloned());
    Ok(())
}

/// The name to create under, or `None` when the duplicate-run guard says to
/// write nothing.
async fn playlist_name(
    session: &mut WriteSession<'_>,
    wanted: &str,
) -> anyhow::Result<Option<String>> {
    if !session
        .list_playlist_names()
        .await?
        .iter()
        .any(|name| name == wanted)
    {
        return Ok(Some(wanted.to_owned()));
    }
    print!(
        "You already have an Apple Music playlist called \"{wanted}\". rocola can't add songs to a playlist that already exists. Create \"{wanted} (rocola)\" instead? [y/N] "
    );
    std::io::stdout().flush()?;
    let mut answer = String::new();
    std::io::stdin().read_line(&mut answer)?;
    Ok(answer
        .trim()
        .eq_ignore_ascii_case("y")
        .then(|| format!("{wanted} (rocola)")))
}

/// One heading and one line per entry — the spec's rule that nothing is ever
/// dropped silently. A heading with nothing under it prints nothing at all.
fn report(heading: &str, lines: impl Iterator<Item = String>) {
    let mut lines = lines.peekable();
    if lines.peek().is_none() {
        return;
    }
    println!("{heading}");
    for line in lines {
        println!("  {line}");
    }
}

/// `"Title — Artist, Artist"`, the same shape rocola-spotify gives the items
/// it couldn't turn into tracks, so every report section reads alike.
fn track_line(track: &TrackMatch) -> String {
    format!(
        "{title} — {artists}",
        title = track.source.title,
        artists = track.source.artists.join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::{orphaned, track_line};
    use rocola_core::{Confidence, SourceTrack, TrackMatch};

    #[test]
    fn a_report_line_names_the_track_and_every_artist() {
        let track = TrackMatch {
            source: SourceTrack {
                title: "Under Pressure".into(),
                artists: vec!["Queen".into(), "David Bowie".into()],
                album: "Hot Space".into(),
                duration_ms: 248_000,
                isrc: None,
            },
            candidates: Vec::new(),
            confidence: Confidence::NotFound,
        };
        assert_eq!(track_line(&track), "Under Pressure — Queen, David Bowie");
    }

    #[test]
    fn an_empty_playlist_says_so_and_says_what_to_do() {
        let error = orphaned("Roadtrip", &"Apple Music answered 500");
        assert_eq!(
            error.to_string(),
            "Created \"Roadtrip\" but couldn't add the songs: Apple Music answered 500. \
             Delete it in Music.app and run rocola again."
        );
    }
}
