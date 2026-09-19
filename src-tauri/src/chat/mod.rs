use std::collections::{HashMap, VecDeque};

use thiserror::Error;
use uuid::Uuid;

pub const MAX_CHAT_BODY_BYTES: usize = 2_000;
pub const CHAT_OVERLAY_LIFETIME_MS: u64 = 5_000;
pub const REACTION_WINDOW_US: u64 = 3_000_000;
pub const MAX_REACTIONS_PER_WINDOW: usize = 5;

/// MP-14: how many chat messages a room retains.
///
/// The bound is applied identically to locally generated and received
/// messages, so a peer cannot grow the room's chat history — and therefore
/// the snapshot that is cloned on every emit — without limit. At the
/// [`MAX_CHAT_BODY_BYTES`] ceiling this caps retained chat at ~400 KiB, which
/// is a deliberate choice: comfortably under a second of video bitrate, and
/// far below what an unbounded `Vec` would accumulate over a long party.
pub const MAX_CHAT_HISTORY: usize = 200;

/// MP-14: how many reactions a room retains. Reactions carry no body, but the
/// same unbounded-growth argument applies.
pub const MAX_REACTION_HISTORY: usize = 200;

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ChatMessage {
    pub message_id: Uuid,
    pub body: String,
    pub created_host_time_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct ReactionMessage {
    pub reaction_id: Uuid,
    pub reaction: String,
}

#[derive(Debug, Error, PartialEq, Eq)]
pub enum ChatError {
    #[error("MP-CHAT-001 empty chat body")]
    EmptyBody,
    #[error("MP-CHAT-002 chat body exceeds 2000 UTF-8 bytes")]
    BodyTooLarge,
    #[error("MP-CHAT-003 unknown reaction")]
    UnknownReaction,
    #[error("MP-CHAT-004 reaction rate limit exceeded")]
    ReactionRateLimited,
    #[error("MP-CHAT-005 chat message id is not a UUID")]
    InvalidMessageId,
    #[error("MP-CHAT-006 reaction id is not a UUID")]
    InvalidReactionId,
}

/// The body rules shared by locally generated and received chat messages.
fn validate_chat_body(body: &str) -> Result<(), ChatError> {
    if body.trim().is_empty() {
        return Err(ChatError::EmptyBody);
    }

    if body.len() > MAX_CHAT_BODY_BYTES {
        return Err(ChatError::BodyTooLarge);
    }

    Ok(())
}

/// The reaction rules shared by locally generated and received reactions.
fn validate_reaction_token(reaction: &str) -> Result<(), ChatError> {
    if allowed_reactions().contains(&reaction) {
        Ok(())
    } else {
        Err(ChatError::UnknownReaction)
    }
}

pub fn validate_chat_message(message: &ChatMessage) -> Result<(), ChatError> {
    validate_chat_body(&message.body)
}

pub fn validate_reaction(message: &ReactionMessage) -> Result<(), ChatError> {
    validate_reaction_token(&message.reaction)
}

/// MP-14: validate a chat message that arrived from a peer over the wire.
///
/// The receive path must apply the *same* rules as the local send path. A
/// locally generated message is built from a `Uuid` and validated before it is
/// appended; a received one arrives as an arbitrary string, so the id shape
/// has to be checked here too rather than trusted.
pub fn validate_received_chat_message(message_id: &str, body: &str) -> Result<(), ChatError> {
    if Uuid::parse_str(message_id).is_err() {
        return Err(ChatError::InvalidMessageId);
    }

    validate_chat_body(body)
}

/// MP-14: validate a reaction that arrived from a peer over the wire.
///
/// Reaction *rate* is enforced separately by [`ReactionRateLimiter`]; this is
/// the value check, which the local send path already applied.
pub fn validate_received_reaction(reaction_id: &str, reaction: &str) -> Result<(), ChatError> {
    if Uuid::parse_str(reaction_id).is_err() {
        return Err(ChatError::InvalidReactionId);
    }

    validate_reaction_token(reaction)
}

/// MP-14: append to a bounded history, evicting the oldest entry.
///
/// `remove(0)` is O(n) but the bound is small and the shift is a memcpy of at
/// most a few hundred `String`-holding structs, so a `VecDeque` would buy
/// nothing worth the churn in the snapshot type.
pub fn push_bounded<T>(history: &mut Vec<T>, item: T, max: usize) {
    if max == 0 {
        // A zero bound means "retain nothing". Clearing rather than no-opping
        // is deliberate: if a bound were ever mis-set to 0, the failure should
        // be a visibly empty history rather than a silently unbounded one.
        history.clear();
        return;
    }

    while history.len() >= max {
        history.remove(0);
    }

    history.push(item);
}

pub fn allowed_reactions() -> [&'static str; 6] {
    ["😂", "❤️", "😮", "🔥", "😭", "👏"]
}

#[derive(Debug, Default)]
pub struct ReactionRateLimiter {
    events_by_participant: HashMap<String, VecDeque<u64>>,
}

impl ReactionRateLimiter {
    pub fn accept(&mut self, participant_id: &str, host_time_us: u64) -> Result<(), ChatError> {
        let events = self
            .events_by_participant
            .entry(participant_id.to_owned())
            .or_default();
        let window_start = host_time_us.saturating_sub(REACTION_WINDOW_US);

        while events
            .front()
            .is_some_and(|event_time_us| *event_time_us < window_start)
        {
            events.pop_front();
        }

        if events.len() >= MAX_REACTIONS_PER_WINDOW {
            return Err(ChatError::ReactionRateLimited);
        }

        events.push_back(host_time_us);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn chat_message(body: String) -> ChatMessage {
        ChatMessage {
            message_id: Uuid::now_v7(),
            body,
            created_host_time_us: 42,
        }
    }

    #[test]
    fn chat_body_accepts_maximum_utf8_bytes() {
        let message = chat_message("a".repeat(MAX_CHAT_BODY_BYTES));

        assert_eq!(validate_chat_message(&message), Ok(()));
    }

    #[test]
    fn chat_body_rejects_empty_and_oversized_input() {
        assert_eq!(
            validate_chat_message(&chat_message("   ".to_owned())),
            Err(ChatError::EmptyBody)
        );
        assert_eq!(
            validate_chat_message(&chat_message("a".repeat(MAX_CHAT_BODY_BYTES + 1))),
            Err(ChatError::BodyTooLarge)
        );
    }

    #[test]
    fn reactions_allow_only_v1_set() {
        for reaction in allowed_reactions() {
            let message = ReactionMessage {
                reaction_id: Uuid::now_v7(),
                reaction: reaction.to_owned(),
            };

            assert_eq!(validate_reaction(&message), Ok(()));
        }

        let unknown = ReactionMessage {
            reaction_id: Uuid::now_v7(),
            reaction: "⭐".to_owned(),
        };

        assert_eq!(validate_reaction(&unknown), Err(ChatError::UnknownReaction));
    }

    #[test]
    fn reaction_limiter_allows_five_per_three_seconds() {
        let mut limiter = ReactionRateLimiter::default();

        for index in 0..MAX_REACTIONS_PER_WINDOW {
            assert_eq!(limiter.accept("rahul", index as u64), Ok(()));
        }

        assert_eq!(
            limiter.accept("rahul", 2_999_999),
            Err(ChatError::ReactionRateLimited)
        );
        assert_eq!(limiter.accept("rahul", 3_000_001), Ok(()));
    }

    // ── MP-14: the receive path must apply the same rules as the send path ──

    #[test]
    fn received_chat_message_accepts_a_well_formed_message() {
        let id = Uuid::now_v7().to_string();

        assert_eq!(
            validate_received_chat_message(&id, "hello"),
            Ok(()),
            "a well-formed received message must still be accepted"
        );
    }

    #[test]
    fn received_chat_message_requires_a_uuid_id() {
        assert_eq!(
            validate_received_chat_message("not-a-uuid", "hello"),
            Err(ChatError::InvalidMessageId)
        );
        // A UUID-shaped string that is not actually parseable must also fail:
        // the check is a parse, not a length or character-class test.
        assert_eq!(
            validate_received_chat_message("0198c3d0-7c55-7f82-9af2-36c9946b297", "hello"),
            Err(ChatError::InvalidMessageId)
        );
    }

    #[test]
    fn received_chat_message_applies_the_local_body_rules() {
        let id = Uuid::now_v7().to_string();

        assert_eq!(
            validate_received_chat_message(&id, "   "),
            Err(ChatError::EmptyBody)
        );
        assert_eq!(
            validate_received_chat_message(&id, &"a".repeat(MAX_CHAT_BODY_BYTES + 1)),
            Err(ChatError::BodyTooLarge)
        );
        // The boundary itself is still accepted, so the cap is not off by one.
        assert_eq!(
            validate_received_chat_message(&id, &"a".repeat(MAX_CHAT_BODY_BYTES)),
            Ok(())
        );
    }

    #[test]
    fn received_reaction_requires_a_known_token_and_a_uuid_id() {
        let id = Uuid::now_v7().to_string();

        assert_eq!(validate_received_reaction(&id, "🔥"), Ok(()));
        assert_eq!(
            validate_received_reaction(&id, "⭐"),
            Err(ChatError::UnknownReaction)
        );
        assert_eq!(
            validate_received_reaction("not-a-uuid", "🔥"),
            Err(ChatError::InvalidReactionId)
        );
    }

    #[test]
    fn push_bounded_evicts_the_oldest_entry_and_keeps_order() {
        let mut history: Vec<u32> = Vec::new();

        for value in 0..(MAX_CHAT_HISTORY as u32 + 5) {
            push_bounded(&mut history, value, MAX_CHAT_HISTORY);
        }

        assert_eq!(history.len(), MAX_CHAT_HISTORY);
        assert_eq!(
            history.first().copied(),
            Some(5),
            "the oldest entries must be the ones evicted"
        );
        assert_eq!(history.last().copied(), Some(MAX_CHAT_HISTORY as u32 + 4));
        assert!(
            history.windows(2).all(|pair| pair[0] < pair[1]),
            "eviction must not reorder the retained history"
        );
    }

    #[test]
    fn push_bounded_with_zero_capacity_stores_nothing() {
        let mut history: Vec<u32> = vec![1, 2, 3];
        push_bounded(&mut history, 4, 0);

        assert!(history.is_empty());
    }
}
