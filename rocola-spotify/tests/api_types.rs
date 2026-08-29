use rocola_spotify::api_types::PlaylistPage;

#[test]
fn maps_page_to_source_tracks_skipping_null_and_local() {
    let page: PlaylistPage =
        serde_json::from_str(include_str!("fixtures/playlist_page.json")).unwrap();
    let tracks = page.source_tracks();
    assert_eq!(tracks.len(), 1);
    assert_eq!(tracks[0].title, "Umbrella");
    assert_eq!(
        tracks[0].artists,
        vec!["Rihanna".to_string(), "JAY-Z".to_string()]
    );
    assert_eq!(tracks[0].isrc.as_deref(), Some("USUM70701234"));
    assert!(page.next.is_some());
}
