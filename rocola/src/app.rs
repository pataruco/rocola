use rocola_core::{Confidence, TrackMatch};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Key {
    Up,
    Down,
    Digit(u8),
    Skip,
    Confirm,
    Abort,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Decision {
    Accepted(usize),
    Skipped,
    Pending,
}

#[derive(Debug, Clone)]
pub struct ReviewItem {
    pub track: TrackMatch,
    pub decision: Decision,
}

#[derive(Debug, Clone)]
pub enum Screen {
    Review {
        items: Vec<ReviewItem>,
        cursor: usize,
    },
    Confirm {
        playlist_name: String,
        accepted: usize,
        skipped: usize,
        not_found: usize,
    },
    Done,
    Aborted,
}

#[derive(Debug)]
pub struct App {
    pub screen: Screen,
    pub playlist_name: String,
    pub auto_accepted: Vec<TrackMatch>,
    pub not_found: Vec<TrackMatch>,
    /// Review rows, kept after review completes so Confirm/Result can report
    /// every decision and `accepted_catalog_ids` can read reviewer choices.
    pub decided: Vec<ReviewItem>,
}

impl App {
    pub fn from_matches(playlist_name: String, matches: Vec<TrackMatch>) -> Self {
        let mut auto_accepted = Vec::new();
        let mut not_found = Vec::new();
        let mut review = Vec::new();
        for m in matches {
            match m.confidence {
                Confidence::Exact | Confidence::High => auto_accepted.push(m),
                Confidence::NotFound => not_found.push(m),
                Confidence::Ambiguous => review.push(ReviewItem {
                    track: m,
                    decision: Decision::Pending,
                }),
            }
        }
        let mut app = Self {
            screen: Screen::Review {
                items: review,
                cursor: 0,
            },
            playlist_name,
            auto_accepted,
            not_found,
            decided: Vec::new(),
        };
        app.advance_if_review_done();
        app
    }

    pub fn on_key(&mut self, key: Key) {
        if key == Key::Abort {
            self.screen = Screen::Aborted;
            return;
        }
        match &mut self.screen {
            Screen::Review { items, cursor } => {
                match key {
                    Key::Up => *cursor = cursor.saturating_sub(1),
                    Key::Down => *cursor = (*cursor + 1).min(items.len().saturating_sub(1)),
                    Key::Digit(n) => {
                        let idx = usize::from(n).wrapping_sub(1);
                        if let Some(item) = items.get_mut(*cursor)
                            && idx < item.track.candidates.len()
                        {
                            item.decision = Decision::Accepted(idx);
                            *cursor = (*cursor + 1).min(items.len().saturating_sub(1));
                        }
                    }
                    Key::Skip => {
                        if let Some(item) = items.get_mut(*cursor) {
                            item.decision = Decision::Skipped;
                            *cursor = (*cursor + 1).min(items.len().saturating_sub(1));
                        }
                    }
                    // Promotion to Confirm happens on an explicit Confirm
                    // key press with every row decided — not automatically
                    // after the Digit/Skip that decided the last row, so a
                    // Confirm press always lands on (or stays on) Review
                    // before Confirm can advance it in the same call.
                    Key::Confirm => self.advance_if_review_done(),
                    Key::Abort => unreachable!("handled above"),
                }
            }
            Screen::Confirm { .. } => {
                if key == Key::Confirm {
                    self.screen = Screen::Done;
                }
            }
            Screen::Done | Screen::Aborted => {}
        }
    }

    /// Everything Confirm will write: best candidate of each auto-accepted
    /// match, then each reviewer-chosen candidate.
    pub fn accepted_catalog_ids(&self) -> Vec<String> {
        let review_rows: &[ReviewItem] = match &self.screen {
            Screen::Review { items, .. } => items,
            _ => &self.decided,
        };
        self.auto_accepted
            .iter()
            .filter_map(|m| m.candidates.first().map(|c| c.catalog_id.clone()))
            .chain(review_rows.iter().filter_map(|item| match item.decision {
                Decision::Accepted(i) => item.track.candidates.get(i).map(|c| c.catalog_id.clone()),
                _ => None,
            }))
            .collect()
    }

    fn advance_if_review_done(&mut self) {
        let Screen::Review { items, .. } = &self.screen else {
            return;
        };
        if items
            .iter()
            .any(|i| matches!(i.decision, Decision::Pending))
        {
            return;
        }
        let accepted = self.auto_accepted.len()
            + items
                .iter()
                .filter(|i| matches!(i.decision, Decision::Accepted(_)))
                .count();
        let skipped = items
            .iter()
            .filter(|i| matches!(i.decision, Decision::Skipped))
            .count();
        self.decided = items.clone();
        self.screen = Screen::Confirm {
            playlist_name: self.playlist_name.clone(),
            accepted,
            skipped,
            not_found: self.not_found.len(),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rocola_core::{Candidate, Confidence, MatchedBy, SourceTrack, TrackMatch};

    fn tm(title: &str, confidence: Confidence, n_candidates: usize) -> TrackMatch {
        let source = SourceTrack {
            title: title.into(),
            artists: vec!["A".into()],
            album: "L".into(),
            duration_ms: 200_000,
            isrc: None,
        };
        let candidates = (0..n_candidates)
            .map(|i| Candidate {
                catalog_id: format!("cat{i}"),
                title: title.into(),
                artists: vec!["A".into()],
                album: "L".into(),
                duration_ms: 200_000,
                matched_by: MatchedBy::Search,
            })
            .collect();
        TrackMatch {
            source,
            candidates,
            confidence,
        }
    }

    #[test]
    fn triage_splits_by_confidence_and_counts_everything() {
        let app = App::from_matches(
            "Mix".into(),
            vec![
                tm("a", Confidence::Exact, 1),
                tm("b", Confidence::High, 1),
                tm("c", Confidence::Ambiguous, 3),
                tm("d", Confidence::NotFound, 0),
            ],
        );
        assert_eq!(app.auto_accepted.len(), 2);
        assert_eq!(app.not_found.len(), 1);
        let Screen::Review { items, cursor } = &app.screen else {
            panic!("ambiguous tracks must open the review screen")
        };
        assert_eq!(items.len(), 1);
        assert_eq!(*cursor, 0);
    }

    #[test]
    fn no_ambiguous_tracks_skips_review_entirely() {
        let app = App::from_matches("Mix".into(), vec![tm("a", Confidence::Exact, 1)]);
        assert!(matches!(
            app.screen,
            Screen::Confirm {
                accepted: 1,
                skipped: 0,
                not_found: 0,
                ..
            }
        ));
    }

    #[test]
    fn digit_accepts_that_candidate_and_review_completes_to_confirm() {
        let mut app = App::from_matches(
            "Mix".into(),
            vec![
                tm("c", Confidence::Ambiguous, 3),
                tm("e", Confidence::Ambiguous, 2),
            ],
        );
        app.on_key(Key::Digit(2)); // pick candidate index 1 for row 0, advance
        app.on_key(Key::Skip); // skip row 1
        app.on_key(Key::Confirm);
        let Screen::Confirm {
            accepted, skipped, ..
        } = &app.screen
        else {
            panic!("decided review must confirm")
        };
        assert_eq!((*accepted, *skipped), (1, 1));
        assert_eq!(app.accepted_catalog_ids(), vec!["cat1".to_string()]);
    }

    #[test]
    fn confirm_is_refused_while_any_row_is_pending() {
        let mut app = App::from_matches("Mix".into(), vec![tm("c", Confidence::Ambiguous, 2)]);
        app.on_key(Key::Confirm);
        assert!(
            matches!(app.screen, Screen::Review { .. }),
            "cannot confirm with pending rows"
        );
    }

    #[test]
    fn digit_out_of_range_is_ignored() {
        let mut app = App::from_matches("Mix".into(), vec![tm("c", Confidence::Ambiguous, 2)]);
        app.on_key(Key::Digit(9));
        let Screen::Review { items, .. } = &app.screen else {
            panic!()
        };
        assert!(matches!(items[0].decision, Decision::Pending));
    }

    #[test]
    fn abort_works_from_any_screen_and_confirm_reaches_done() {
        let mut reviewing =
            App::from_matches("Mix".into(), vec![tm("c", Confidence::Ambiguous, 1)]);
        reviewing.on_key(Key::Abort);
        assert!(matches!(reviewing.screen, Screen::Aborted));

        let mut confirming = App::from_matches("Mix".into(), vec![tm("a", Confidence::Exact, 1)]);
        confirming.on_key(Key::Confirm);
        assert!(matches!(confirming.screen, Screen::Done));
    }
}
