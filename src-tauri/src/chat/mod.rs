use std::collections::{HashMap, VecDeque};

use thiserror::Error;
use uuid::Uuid;

pub const MAX_CHAT_BODY_BYTES: usize = 2_000;
pub const CHAT_OVERLAY_LIFETIME_MS: u64 = 5_000;
pub const REACTION_WINDOW_US: u64 = 3_000_000;
pub const MAX_REACTIONS_PER_WINDOW: usize = 5;

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
}

pub fn validate_chat_message(message: &ChatMessage) -> Result<(), ChatError> {
    if message.body.trim().is_empty() {
        return Err(ChatError::EmptyBody);
    }

    if message.body.len() > MAX_CHAT_BODY_BYTES {
        return Err(ChatError::BodyTooLarge);
    }

    Ok(())
}

pub fn validate_reaction(message: &ReactionMessage) -> Result<(), ChatError> {
    if allowed_reactions().contains(&message.reaction.as_str()) {
        Ok(())
    } else {
        Err(ChatError::UnknownReaction)
    }
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
}
