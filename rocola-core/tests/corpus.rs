use rocola_core::{Candidate, Confidence, SourceTrack, classify};
use serde::Deserialize;

#[derive(Deserialize)]
struct Case {
    name: String,
    source: SourceTrack,
    candidates: Vec<Candidate>,
    expected: Confidence,
}

#[test]
fn golden_corpus() {
    let raw = include_str!("fixtures/corpus.json");
    let cases: Vec<Case> = serde_json::from_str(raw).expect("corpus.json parses");
    let mut failures = Vec::new();
    for case in cases {
        let got = classify(&case.source, case.candidates.clone()).confidence;
        if got != case.expected {
            failures.push(format!(
                "{}: expected {:?}, got {got:?}",
                case.name, case.expected
            ));
        }
    }
    assert!(failures.is_empty(), "\n{}", failures.join("\n"));
}
