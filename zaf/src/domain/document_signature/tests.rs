use std::time::{Duration, UNIX_EPOCH};

use super::*;

const BODY: &str = "# Scope\n\nThe release builds the thing.\n";
const HASH: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";

fn at() -> SystemTime {
    UNIX_EPOCH + Duration::from_secs(1_794_000_000)
}

fn malformed(text: &str) -> usize {
    match DocumentSignature::read(text) {
        SignatureState::Malformed { line, .. } => line,
        other => panic!("expected malformed, got {other:?}"),
    }
}

#[test]
fn signed_document_reads_as_signed() {
    let text = DocumentSignature::sign(BODY, at());
    let state = DocumentSignature::read(&text);
    assert_eq!(
        state,
        SignatureState::Signed {
            at: at(),
            hash: BodyHash::of(&TicketBody::from(BODY)),
        }
    );
    assert_eq!(DocumentSignature::body(&text), BODY);
}

#[test]
fn body_hash_is_the_epic_body_hash() {
    let text = DocumentSignature::sign(BODY, at());
    let expected = BodyHash::of(&TicketBody::from(BODY));
    assert!(text.contains(&format!("hash: {expected}\n---\n")));
}

#[test]
fn edit_below_the_block_voids_the_signature() {
    let text = DocumentSignature::sign(BODY, at()).replace("builds", "breaks");
    assert_eq!(DocumentSignature::read(&text), SignatureState::Unsigned);
}

#[test]
fn signing_again_replaces_the_block() {
    let first = DocumentSignature::sign(BODY, at());
    let later = at() + Duration::from_secs(60);
    let again = DocumentSignature::sign(&first, later);
    assert_eq!(again.matches("---\n").count(), 2);
    assert_eq!(DocumentSignature::body(&again), BODY);
    assert!(matches!(
        DocumentSignature::read(&again),
        SignatureState::Signed { at, .. } if at == later
    ));
}

#[test]
fn extra_key_is_malformed_with_its_line() {
    let text = format!("---\nsigned: 2026-11-17T10:00:00Z\nby: zuri\nhash: {HASH}\n---\n{BODY}");
    assert_eq!(malformed(&text), 3);
}

#[test]
fn repeated_key_is_malformed() {
    let text =
        format!("---\nhash: {HASH}\nsigned: 2026-11-17T10:00:00Z\nhash: {HASH}\n---\n{BODY}");
    assert_eq!(malformed(&text), 4);
}

#[test]
fn unreadable_time_is_malformed() {
    let text = format!("---\nsigned: yesterday\nhash: {HASH}\n---\n{BODY}");
    assert_eq!(malformed(&text), 2);
}

#[test]
fn short_hash_is_malformed() {
    let text = format!(
        "---\nsigned: 2026-11-17T10:00:00Z\nhash: {}\n---\n{BODY}",
        &HASH[..63]
    );
    assert_eq!(malformed(&text), 3);
    let upper = format!(
        "---\nsigned: 2026-11-17T10:00:00Z\nhash: {}\n---\n{BODY}",
        HASH.to_uppercase()
    );
    assert_eq!(malformed(&upper), 3);
}

#[test]
fn missing_closing_line_is_malformed() {
    let text = format!("---\nsigned: 2026-11-17T10:00:00Z\nhash: {HASH}\n");
    assert_eq!(malformed(&text), 1);
    assert_eq!(DocumentSignature::body(&text), text);
}

#[test]
fn missing_key_is_malformed_at_the_closing_line() {
    let text = format!("---\nsigned: 2026-11-17T10:00:00Z\n---\n{BODY}");
    assert_eq!(malformed(&text), 3);
}

#[test]
fn document_without_a_block_is_unsigned() {
    assert_eq!(DocumentSignature::read(BODY), SignatureState::Unsigned);
    assert_eq!(DocumentSignature::body(BODY), BODY);
    assert_eq!(DocumentSignature::read(""), SignatureState::Unsigned);
}
