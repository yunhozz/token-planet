use super::types::{ChatContext, ChatError, ChatMessage, ChatScope, ChatSeq};
use std::collections::BTreeMap;

pub struct ChatState {
    pub scope: ChatScope,
    pub context: Option<ChatContext>,
    pub synced_change_seq: ChatSeq,
    messages: BTreeMap<String, ChatMessage>,
}
impl ChatState {
    pub fn new(scope: ChatScope) -> Self {
        Self {
            scope,
            context: None,
            synced_change_seq: ChatSeq::new(0).unwrap(),
            messages: BTreeMap::new(),
        }
    }
    pub fn context(&mut self, context: ChatContext) -> Result<(), ChatError> {
        context.validate(&self.scope.world_id)?;
        if self
            .context
            .as_ref()
            .is_some_and(|old| old.joined_after_seq != context.joined_after_seq)
        {
            self.messages.clear();
            self.synced_change_seq = ChatSeq::new(0).unwrap();
        }
        self.messages
            .retain(|_, m| m.message_seq > context.joined_after_seq);
        self.context = Some(context);
        Ok(())
    }
    pub fn merge(&mut self, scope: &ChatScope, message: ChatMessage) -> Result<bool, ChatError> {
        if scope != &self.scope {
            return Ok(false);
        }
        message.validate(&self.scope.world_id)?;
        if self
            .context
            .as_ref()
            .is_some_and(|c| message.message_seq <= c.joined_after_seq)
        {
            return Ok(false);
        }
        if let Some(old) = self.messages.get(&message.id) {
            if old.message_seq != message.message_seq || old.author_key != message.author_key {
                return Err(ChatError::InvalidResponse);
            }
            if old.change_seq >= message.change_seq {
                return Ok(false);
            }
            // A deletion is irreversible even if a malformed later event claims a higher sequence.
            if old.deleted_at.is_some() && message.deleted_at.is_none() {
                return Err(ChatError::InvalidResponse);
            }
        }
        self.messages.insert(message.id.clone(), message);
        Ok(true)
    }
    pub fn messages(&self) -> Vec<ChatMessage> {
        let mut messages: Vec<_> = self.messages.values().cloned().collect();
        messages.sort_by_key(|m| m.message_seq);
        messages
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn message(change: i64, deleted: bool) -> ChatMessage {
        serde_json::from_value(serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","world_id":"22222222-2222-4222-8222-222222222222","author_key":"33333333-3333-4333-8333-333333333333","nickname":"n","avatar":"masculine","body":if deleted {None}else{Some("hello")},"message_seq":"1","change_seq":change.to_string(),"created_at":"2026-10-10T00:00:00Z","deleted_at":if deleted {Some("2026-10-10T01:00:00Z")}else{None}})).unwrap()
    }
    #[test]
    fn duplicate_and_late_insert_cannot_restore_tombstone() {
        let scope = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        let mut state = ChatState::new(scope.clone());
        assert!(state.merge(&scope, message(2, true)).unwrap());
        assert!(!state.merge(&scope, message(1, false)).unwrap());
        assert!(!state.merge(&scope, message(2, true)).unwrap());
        assert!(state.messages().first().unwrap().body.is_none());
        let old = ChatScope {
            generation: 0,
            ..scope
        };
        assert!(!state.merge(&old, message(3, false)).unwrap());
    }
}
