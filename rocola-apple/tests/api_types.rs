use rocola_apple::api_types::{SearchResponse, SongsResponse};
use rocola_core::MatchedBy;

#[test]
fn isrc_response_maps_to_candidates_keyed_by_isrc() {
    let r: SongsResponse =
        serde_json::from_str(include_str!("fixtures/isrc_response.json")).unwrap();
    let pairs = r.into_isrc_candidates();
    assert_eq!(pairs.len(), 1);
    let (isrc, c) = &pairs[0];
    assert_eq!(isrc, "USUM70701234");
    assert_eq!(c.catalog_id, "900032829");
    assert_eq!(c.artists, vec!["Rihanna".to_string()]);
    assert_eq!(c.duration_ms, 275_986);
    assert_eq!(c.matched_by, MatchedBy::Isrc);
}

#[test]
fn search_response_maps_to_search_candidates() {
    let r: SearchResponse =
        serde_json::from_str(include_str!("fixtures/search_response.json")).unwrap();
    let cs = r.into_candidates();
    assert_eq!(cs.len(), 1);
    assert_eq!(cs[0].catalog_id, "1440783625");
    assert_eq!(cs[0].matched_by, MatchedBy::Search);
}
