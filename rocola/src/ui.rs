//! Rendering only. Every rule about what a screen means lives in `app.rs`;
//! this module decides nothing, it just draws whatever state it is handed.

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::{List, Paragraph, Row, Table};
use rocola_core::normalize::{normalize_artist, normalize_title};
use rocola_core::{Candidate, SourceTrack};

use crate::app::{App, Decision, ReviewItem, Screen};

/// Same window the matching engine calls "the same recording": a candidate
/// within three seconds is not a different length, it is the same length.
const DURATION_WINDOW_MS: u32 = 3_000;

/// `"1 song"` / `"4 songs"` — a count in a sentence has to read as English.
#[must_use]
pub fn songs(n: usize) -> String {
    if n == 1 {
        "1 song".to_owned()
    } else {
        format!("{n} songs")
    }
}

/// Draw the current screen into `frame`.
pub fn draw(frame: &mut Frame, app: &App) {
    match &app.screen {
        Screen::Review { items, cursor } => draw_review(frame, items, *cursor),
        Screen::Confirm {
            playlist_name,
            accepted,
            skipped,
            not_found,
        } => {
            let body = format!(
                "Create Apple Music playlist \"{playlist_name}\" with {n}?\n\
                 {skipped} skipped by you · {not_found} not found on Apple Music (all listed after creation)\n\
                 \n\
                 enter create · q abort",
                n = songs(*accepted)
            );
            frame.render_widget(Paragraph::new(body), frame.area());
        }
        Screen::Done => {
            // The real report prints to the normal terminal once this screen
            // closes, so this line says what happens next, not where to look.
            let done = Paragraph::new("Creating your playlist on Apple Music…");
            frame.render_widget(done, frame.area());
        }
        Screen::Aborted => {
            frame.render_widget(Paragraph::new("Nothing was created."), frame.area());
        }
    }
}

/// The source track on top, its candidates below, the keys at the bottom.
fn draw_review(frame: &mut Frame, items: &[ReviewItem], cursor: usize) {
    let [source_area, candidates_area, footer_area] = Layout::vertical([
        Constraint::Length(3),
        Constraint::Min(1),
        Constraint::Length(1),
    ])
    .areas(frame.area());

    let Some(item) = items.get(cursor) else {
        return;
    };
    let source = &item.track.source;

    let table = Table::new(
        [Row::new([
            source.title.clone(),
            source.artists.join(", "),
            source.album.clone(),
            minutes_seconds(source.duration_ms),
        ])],
        [
            Constraint::Percentage(35),
            Constraint::Percentage(25),
            Constraint::Percentage(30),
            Constraint::Length(8),
        ],
    )
    .header(Row::new(["Title", "Artist", "Album", "Duration"]).style(Style::new().bold()));
    frame.render_widget(table, source_area);

    let candidates: Vec<Line> = item
        .track
        .candidates
        .iter()
        .enumerate()
        .map(|(i, candidate)| candidate_line(i + 1, source, candidate))
        .collect();
    frame.render_widget(List::new(candidates), candidates_area);

    let decided = items
        .iter()
        .filter(|i| !matches!(i.decision, Decision::Pending))
        .count();
    let total = items.len();
    frame.render_widget(
        Paragraph::new(format!(
            "↑/↓ move · 1-9 pick · s skip · enter confirm when done · q abort · {decided}/{total} decided"
        )),
        footer_area,
    );
}

/// `"{n}. {title} — {artists} — {album} ({duration})"`, with every field that
/// disagrees with the source in bold — the reviewer's eye goes straight to
/// what makes this candidate a different choice from the ones around it.
fn candidate_line(n: usize, source: &SourceTrack, candidate: &Candidate) -> Line<'static> {
    let title_differs = normalize_title(&source.title) != normalize_title(&candidate.title);
    let album_differs = normalize_title(&source.album) != normalize_title(&candidate.album);
    let duration_differs = source.duration_ms.abs_diff(candidate.duration_ms) > DURATION_WINDOW_MS;
    // Apple returns the whole billing as one string ("A & B") where Spotify
    // gives a list, so equality would call almost every collaboration a
    // difference. Sharing no artist at all is the difference worth showing.
    let source_artists: Vec<String> = source.artists.iter().map(|a| normalize_artist(a)).collect();
    let artists_differ = !candidate
        .artists
        .iter()
        .any(|a| source_artists.contains(&normalize_artist(a)));

    Line::from(vec![
        Span::raw(format!("{n}. ")),
        field(candidate.title.clone(), title_differs),
        Span::raw(" — "),
        field(candidate.artists.join(", "), artists_differ),
        Span::raw(" — "),
        field(candidate.album.clone(), album_differs),
        Span::raw(" ("),
        field(minutes_seconds(candidate.duration_ms), duration_differs),
        Span::raw(")"),
    ])
}

fn field(text: String, differs: bool) -> Span<'static> {
    if differs {
        Span::styled(text, Style::new().bold())
    } else {
        Span::raw(text)
    }
}

fn minutes_seconds(duration_ms: u32) -> String {
    let seconds = duration_ms / 1_000;
    format!("{}:{:02}", seconds / 60, seconds % 60)
}

#[cfg(test)]
mod tests {
    use super::{candidate_line, draw, minutes_seconds};
    use crate::app::App;
    use ratatui::Terminal;
    use ratatui::backend::TestBackend;
    use rocola_core::{Candidate, Confidence, MatchedBy, SourceTrack, TrackMatch};

    fn source() -> SourceTrack {
        SourceTrack {
            title: "Umbrella".into(),
            artists: vec!["Rihanna".into()],
            album: "Good Girl Gone Bad".into(),
            duration_ms: 275_000,
            isrc: None,
        }
    }

    fn candidate(title: &str, artist: &str, album: &str, duration_ms: u32) -> Candidate {
        Candidate {
            catalog_id: "1".into(),
            title: title.into(),
            artists: vec![artist.into()],
            album: album.into(),
            duration_ms,
            matched_by: MatchedBy::Search,
        }
    }

    #[test]
    fn renders_minutes_and_zero_padded_seconds() {
        assert_eq!(minutes_seconds(275_000), "4:35");
        assert_eq!(minutes_seconds(61_000), "1:01");
    }

    #[test]
    fn only_the_fields_that_differ_are_bold() {
        let line = candidate_line(
            1,
            &source(),
            &candidate(
                "Umbrella",
                "Rihanna",
                "Now That's What I Call Music",
                275_500,
            ),
        );
        let bold: Vec<&str> = line
            .spans
            .iter()
            .filter(|s| {
                s.style
                    .add_modifier
                    .contains(ratatui::style::Modifier::BOLD)
            })
            .map(|s| s.content.as_ref())
            .collect();
        assert_eq!(bold, vec!["Now That's What I Call Music"]);
    }

    #[test]
    fn a_shared_artist_and_a_remaster_suffix_are_not_differences() {
        let line = candidate_line(
            2,
            &source(),
            &candidate(
                "Umbrella (feat. JAY-Z)",
                "Rihanna",
                "Good Girl Gone Bad",
                275_000,
            ),
        );
        assert!(
            !line.spans.iter().any(|s| s
                .style
                .add_modifier
                .contains(ratatui::style::Modifier::BOLD)),
            "nothing meaningfully differs: {line:?}"
        );
    }

    fn track(title: &str, confidence: Confidence) -> TrackMatch {
        TrackMatch {
            source: source(),
            candidates: vec![candidate(title, "Rihanna", "Good Girl Gone Bad", 275_000)],
            confidence,
        }
    }

    /// Draw one app state into an 80x24 test terminal and read it back.
    fn render(matches: Vec<TrackMatch>) -> String {
        let app = App::from_matches("Mix".into(), matches);
        let mut terminal = Terminal::new(TestBackend::new(80, 24)).expect("test terminal");
        terminal
            .draw(|frame| draw(frame, &app))
            .expect("draw must not fail");
        terminal
            .backend()
            .buffer()
            .content()
            .iter()
            .map(ratatui::buffer::Cell::symbol)
            .collect()
    }

    #[test]
    fn review_shows_the_candidates_and_the_footer_tally() {
        let screen = render(vec![
            track("Umbrella", Confidence::Ambiguous),
            track("Diamonds", Confidence::Exact),
        ]);
        assert!(screen.contains("1. Umbrella"), "{screen}");
        assert!(screen.contains("0/1 decided"), "{screen}");
    }

    #[test]
    fn no_ambiguous_tracks_renders_the_confirm_question() {
        let screen = render(vec![track("Umbrella", Confidence::Exact)]);
        assert!(
            screen.contains("Create Apple Music playlist \"Mix\" with 1 song?"),
            "{screen}"
        );
        assert!(screen.contains("enter create · q abort"), "{screen}");
    }
}
