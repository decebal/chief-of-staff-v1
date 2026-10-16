use cos::reports::{Brief, Review, brief_schema, review_schema};
use serde_json::{Value, json};

/// Every property the schema requires must be a field the struct reads, and
/// the other way round — otherwise `--json-schema` enforces one shape and
/// `serde` expects another.
fn required(schema: &Value) -> Vec<String> {
    let mut keys: Vec<String> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|k| k.as_str().unwrap().to_owned())
        .collect();
    keys.sort();
    keys
}

fn fields<T: serde::Serialize>(v: &T) -> Vec<String> {
    let mut keys: Vec<String> = serde_json::to_value(v)
        .unwrap()
        .as_object()
        .unwrap()
        .keys()
        .cloned()
        .collect();
    keys.sort();
    keys
}

fn brief_sample() -> Value {
    json!({
        "headline": "Two reviews block others; standup at 10.",
        "calendar": [{ "time": "10:00", "what": "Standup", "prep": "none" }],
        "must_do": [{ "item": "Review #81", "why": "blocks merge", "source": "gh:my-org/my-repo#81" }],
        "reply_to": [{ "from": "A", "subject": "SDK", "ask": "confirm version" }],
        "blockers": [{ "item": "TURN creds", "waiting_on": "infra", "unblock": "ping on ticket" }],
        "can_drop": ["newsletter"],
        "gaps": ["Notion not read: connector pending"]
    })
}

fn review_sample() -> Value {
    json!({
        "summary": "One bet moved.",
        "bets": [{ "bet": "E2E green", "confidence": "sand", "evidence": "3 PRs", "next": "shape scope" }],
        "drift": ["unplanned review"],
        "decisions": [{ "decision": "promote E2E", "options": ["now", "later"], "recommendation": "now" }],
        "stop_doing": ["manual runs"],
        "gaps": []
    })
}

fn brief_sample_v2() -> Value {
    json!({
        "headline": "Release PR waits on you.",
        "calendar": [{ "start": "13:30", "end": "13:45", "what": "Stand-up", "prep": "bring blocker" }],
        "must_do": [{ "item": "Review #81", "why": "blocks merge", "source": "gh:my-org/my-repo#81",
                      "agent_prompt": "cd ~/Projects/my-repo && gh pr checkout 81 ... Stop before anything outward-facing." }],
        "reply_to": [],
        "blockers": [],
        "can_drop": [],
        "gaps": [],
        "celebrations": [{ "win": "Merged #80", "why_it_matters": "unblocked QA", "source": "gh:my-org/my-repo#80" }],
        "upcoming": [{ "date": "2026-10-19", "what": "7.0.0 release", "source": "calendar" }]
    })
}

#[test]
fn brief_schema_matches_struct() {
    let brief: Brief = serde_json::from_value(brief_sample_v2()).unwrap();
    assert_eq!(required(&brief_schema()), fields(&brief));
}

/// Nested objects must match too: a schema field the struct drops is data the
/// model is asked for and the report never sees.
#[test]
fn nested_brief_items_match_schema() {
    let schema = brief_schema();
    let brief =
        serde_json::to_value(serde_json::from_value::<Brief>(brief_sample_v2()).unwrap()).unwrap();
    for key in ["calendar", "must_do", "celebrations", "upcoming"] {
        let mut want = required(&schema["properties"][key]["items"]);
        let mut got: Vec<String> = brief[key][0].as_object().unwrap().keys().cloned().collect();
        want.sort();
        got.sort();
        assert_eq!(want, got, "{key}");
    }
}

#[test]
fn briefs_saved_before_the_new_fields_still_load() {
    let brief: Brief = serde_json::from_value(brief_sample()).unwrap();
    assert!(brief.celebrations.is_empty());
    assert!(brief.must_do[0].agent_prompt.is_empty());
    let review: Review = serde_json::from_value(review_sample()).unwrap();
    assert!(review.bets[0].agent_prompt.is_empty());
}

#[test]
fn review_schema_matches_struct() {
    let review: Review = serde_json::from_value(review_sample()).unwrap();
    assert_eq!(required(&review_schema()), fields(&review));
}

#[test]
fn review_rejects_confidence_outside_the_table() {
    let mut v = review_sample();
    v["bets"][0]["confidence"] = json!("medium");
    assert!(serde_json::from_value::<Review>(v).is_err());
}

#[test]
fn markdown_omits_empty_sections_and_lowercases_tiers() {
    let review: Review = serde_json::from_value(review_sample()).unwrap();
    let md = review.to_markdown("2026-10-16");
    assert!(md.contains("`sand` **E2E green**"));
    assert!(!md.contains("Gaps"));
}
