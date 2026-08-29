use rocola_spotify::auth::TokenSet;

#[test]
fn token_response_deserialises() {
    let t: TokenSet = serde_json::from_str(include_str!("fixtures/token_response.json")).unwrap();
    assert_eq!(t.access_token, "BQabc");
    assert_eq!(t.refresh_token.as_deref(), Some("AQdef"));
    assert_eq!(t.expires_in, 3600);
}
