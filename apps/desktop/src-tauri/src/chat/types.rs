use crate::domain::planet::PlanetAvatar;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
pub struct ChatSeq(i64);
impl ChatSeq {
    pub fn new(value: i64) -> Result<Self, ChatError> {
        if value < 0 {
            Err(ChatError::InvalidInput)
        } else {
            Ok(Self(value))
        }
    }
    pub fn get(self) -> i64 {
        self.0
    }
}
impl Serialize for ChatSeq {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0.to_string())
    }
}
impl<'de> Deserialize<'de> for ChatSeq {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = String::deserialize(deserializer)?;
        if value.is_empty()
            || !value.bytes().all(|b| b.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return Err(serde::de::Error::custom("invalid chat sequence"));
        }
        value
            .parse::<i64>()
            .map(Self)
            .map_err(|_| serde::de::Error::custom("invalid chat sequence"))
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChatError {
    InvalidInput,
    Transport,
    Rejected(u16),
    InvalidResponse,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatScope {
    pub world_id: String,
    pub generation: u64,
}

// Deliberately no Debug implementation: message bodies must not enter diagnostics.
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatMessage {
    pub id: String,
    pub world_id: String,
    pub message_seq: ChatSeq,
    pub change_seq: ChatSeq,
    pub author_key: String,
    pub nickname: String,
    pub avatar: PlanetAvatar,
    pub body: Option<String>,
    pub created_at: String,
    pub deleted_at: Option<String>,
}
pub(crate) fn valid_id(value: &str) -> bool {
    uuid::Uuid::parse_str(value).is_ok()
}
impl ChatMessage {
    pub fn validate(&self, world_id: &str) -> Result<(), ChatError> {
        let created = chrono::DateTime::parse_from_rfc3339(&self.created_at)
            .map_err(|_| ChatError::InvalidResponse)?;
        let body_valid = match (&self.body, &self.deleted_at) {
            (Some(body), None) => !body.trim().is_empty() && body.chars().count() <= 2000,
            (None, Some(deleted)) => {
                chrono::DateTime::parse_from_rfc3339(deleted).is_ok_and(|time| time >= created)
            }
            _ => false,
        };
        if self.world_id != world_id
            || !valid_id(&self.id)
            || !valid_id(&self.world_id)
            || !valid_id(&self.author_key)
            || self.message_seq.get() == 0
            || self.change_seq < self.message_seq
            || !(1..=24).contains(&self.nickname.chars().count())
            || !body_valid
        {
            return Err(ChatError::InvalidResponse);
        }
        Ok(())
    }
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatContext {
    pub world_id: String,
    pub joined_after_seq: ChatSeq,
    pub author_key: String,
    pub last_read_seq: ChatSeq,
    pub last_message_seq: ChatSeq,
    pub last_change_seq: ChatSeq,
    pub unread_count: ChatSeq,
}
impl ChatContext {
    pub fn validate(&self, world_id: &str) -> Result<(), ChatError> {
        if self.world_id != world_id
            || !valid_id(&self.author_key)
            || !valid_id(&self.world_id)
            || self.joined_after_seq > self.last_read_seq
            || self.last_read_seq > self.last_message_seq
            || self.last_message_seq > self.last_change_seq
            || self.unread_count.get() > self.last_message_seq.get() - self.last_read_seq.get()
        {
            return Err(ChatError::InvalidResponse);
        }
        Ok(())
    }
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatPage {
    pub messages: Vec<ChatMessage>,
    pub next_cursor: Option<ChatSeq>,
    pub has_more: bool,
}
#[derive(Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatChangePage {
    pub messages: Vec<ChatMessage>,
    pub next_cursor: ChatSeq,
    pub has_more: bool,
}
#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChatReadState {
    pub last_read_seq: ChatSeq,
    pub unread_count: ChatSeq,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn decimal_sequence_preserves_bigint_and_rejects_noncanonical_values() {
        let seq: ChatSeq = serde_json::from_str("\"9223372036854775807\"").unwrap();
        assert_eq!(seq.get(), i64::MAX);
        assert_eq!(
            serde_json::to_string(&seq).unwrap(),
            "\"9223372036854775807\""
        );
        for value in [
            "1",
            "\"-1\"",
            "\"01\"",
            "\"1.0\"",
            "\"9223372036854775808\"",
        ] {
            assert!(serde_json::from_str::<ChatSeq>(value).is_err());
        }
    }
    #[test]
    fn server_message_requires_valid_profile_and_tombstone() {
        let value = serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","world_id":"22222222-2222-4222-8222-222222222222",
            "author_key":"33333333-3333-4333-8333-333333333333","nickname":"행성 동기화 대기","avatar":"masculine",
            "message_seq":"1","change_seq":"2","body":null,"created_at":"2026-10-10T00:00:00Z","deleted_at":"2026-10-10T01:00:00Z"});
        let message: ChatMessage = serde_json::from_value(value.clone()).unwrap();
        assert!(message
            .validate("22222222-2222-4222-8222-222222222222")
            .is_ok());
        let mut invalid = value.clone();
        invalid["avatar"] = serde_json::json!("photo");
        assert!(serde_json::from_value::<ChatMessage>(invalid).is_err());
        let mut invalid = value;
        invalid["body"] = serde_json::json!("restored secret");
        assert!(serde_json::from_value::<ChatMessage>(invalid)
            .unwrap()
            .validate("22222222-2222-4222-8222-222222222222")
            .is_err());
        assert!(message
            .validate("44444444-4444-4444-8444-444444444444")
            .is_err());
    }
}
