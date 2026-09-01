use rocola_spotify::api_types::PlaylistPage;

#[test]
fn splits_a_page_into_tracks_and_named_leftovers() {
    let page: PlaylistPage =
        serde_json::from_str(include_str!("fixtures/playlist_page.json")).unwrap();
    let split = page.partition();

    assert_eq!(split.tracks.len(), 1);
    assert_eq!(split.tracks[0].title, "Umbrella");
    assert_eq!(
        split.tracks[0].artists,
        vec!["Rihanna".to_string(), "JAY-Z".to_string()]
    );
    assert_eq!(split.tracks[0].isrc.as_deref(), Some("USUM70701234"));

    // The page also holds one removed (null) track, one local file and one
    // podcast episode. All three must survive as named lines, in playlist
    // order — never dropped, and never a failed parse.
    assert_eq!(
        split.skipped,
        vec![
            "(removed track)".to_string(),
            "Home Recording — Me".to_string(),
            "The Rest Is History: The Fall of Rome (podcast episode)".to_string()
        ]
    );

    assert!(page.next.is_some());
}
