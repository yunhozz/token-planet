use super::types::{ChatError, ChatMessage};
use serde_json::{json, Value};

pub fn topic(world: &str) -> String {
    format!("realtime:group-chat:{world}")
}
pub fn websocket_url(base: &str, key: &str) -> Result<String, ChatError> {
    let mut url = reqwest::Url::parse(base).map_err(|_| ChatError::InvalidInput)?;
    if !url.username().is_empty()
        || url.password().is_some()
        || url.path() != "/"
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err(ChatError::InvalidInput);
    }
    let scheme = match url.scheme() {
        "https" => "wss",
        "http" if url.host_str() == Some("127.0.0.1") && url.port() == Some(54321) => "ws",
        _ => return Err(ChatError::InvalidInput),
    };
    url.set_scheme(scheme)
        .map_err(|_| ChatError::InvalidInput)?;
    url.set_path("/realtime/v1/websocket");
    url.query_pairs_mut()
        .append_pair("apikey", key)
        .append_pair("vsn", "1.0.0");
    Ok(url.into())
}
pub fn join_frame(world: &str, token: &str, reference: u64) -> Value {
    json!({"topic":topic(world),"event":"phx_join","ref":reference.to_string(),"payload":{"access_token":token,"config":{"broadcast":{"ack":false,"self":false},"presence":{"enabled":false},"postgres_changes":[
        {"event":"INSERT","schema":"public","table":"group_chat_messages","filter":format!("world_id=eq.{world}")},
        {"event":"UPDATE","schema":"public","table":"group_chat_messages","filter":format!("world_id=eq.{world}")}]}}})
}
pub fn heartbeat_frame(reference: u64) -> Value {
    json!({"topic":"phoenix","event":"heartbeat","payload":{},"ref":reference.to_string()})
}
pub fn token_frame(world: &str, token: &str, reference: u64) -> Value {
    json!({"topic":topic(world),"event":"access_token","payload":{"access_token":token},"ref":reference.to_string()})
}
pub enum Frame {
    Reply { reference: String, ok: bool },
    PostgresReady,
    Message(ChatMessage),
    Ignore,
    Closed,
}
pub fn parse_frame(text: &str, world: &str) -> Result<Frame, ChatError> {
    let frame: Value = serde_json::from_str(text).map_err(|_| ChatError::InvalidResponse)?;
    if frame.get("topic").and_then(Value::as_str) != Some(topic(world).as_str())
        && frame.get("topic").and_then(Value::as_str) != Some("phoenix")
    {
        return Ok(Frame::Ignore);
    }
    match frame.get("event").and_then(Value::as_str) {
        Some("phx_reply") => Ok(Frame::Reply {
            reference: frame
                .get("ref")
                .and_then(Value::as_str)
                .unwrap_or_default()
                .into(),
            ok: frame["payload"]["status"] == "ok",
        }),
        Some("phx_error" | "phx_close") => Ok(Frame::Closed),
        Some("system")
            if matches!(
                frame["payload"]["status"].as_str(),
                Some("error" | "timeout")
            ) =>
        {
            Ok(Frame::Closed)
        }
        Some("system")
            if frame["topic"] == topic(world)
                && frame["payload"]["extension"] == "postgres_changes"
                && frame["payload"]["status"] == "ok" =>
        {
            Ok(Frame::PostgresReady)
        }
        Some("postgres_changes") => {
            let data = &frame["payload"]["data"];
            if data["schema"] != "public"
                || data["table"] != "group_chat_messages"
                || !matches!(data["type"].as_str(), Some("INSERT" | "UPDATE"))
            {
                return Ok(Frame::Ignore);
            }
            let mut row = data["record"].clone();
            if row["world_id"] != world {
                return Ok(Frame::Ignore);
            }
            for field in ["message_seq", "change_seq"] {
                if let Some(number) = row[field].as_i64() {
                    if number < 0 {
                        return Err(ChatError::InvalidResponse);
                    }
                    row[field] = json!(number.to_string());
                }
            }
            let message: ChatMessage =
                serde_json::from_value(row).map_err(|_| ChatError::InvalidResponse)?;
            message.validate(world)?;
            Ok(Frame::Message(message))
        }
        _ => Ok(Frame::Ignore),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn join_and_heartbeat_and_refresh_use_phoenix_frames() {
        let join = join_frame("world", "secret", 1);
        assert_eq!(join["event"], "phx_join");
        assert_eq!(
            join["payload"]["config"]["postgres_changes"]
                .as_array()
                .unwrap()
                .len(),
            2
        );
        assert_eq!(heartbeat_frame(2)["topic"], "phoenix");
        assert_eq!(token_frame("world", "fresh", 3)["event"], "access_token");
    }
    #[test]
    fn postgres_readiness_is_channel_scoped_and_timeout_closes() {
        let frame = json!({"topic":"realtime:group-chat:world","event":"system","payload":{"extension":"postgres_changes","status":"ok","message":"Subscribed to PostgreSQL","channel":"main"}});
        assert!(matches!(
            parse_frame(&frame.to_string(), "world").unwrap(),
            Frame::PostgresReady
        ));
        let mut unrelated = frame.clone();
        unrelated["payload"]["extension"] = json!("broadcast");
        assert!(matches!(
            parse_frame(&unrelated.to_string(), "world").unwrap(),
            Frame::Ignore
        ));
        unrelated = frame.clone();
        unrelated["topic"] = json!("phoenix");
        assert!(matches!(
            parse_frame(&unrelated.to_string(), "world").unwrap(),
            Frame::Ignore
        ));
        unrelated = frame.clone();
        unrelated["topic"] = json!("realtime:group-chat:other");
        assert!(matches!(
            parse_frame(&unrelated.to_string(), "world").unwrap(),
            Frame::Ignore
        ));
        let mut expired = frame;
        expired["payload"]["status"] = json!("timeout");
        assert!(matches!(
            parse_frame(&expired.to_string(), "world").unwrap(),
            Frame::Closed
        ));
    }
    #[test]
    fn url_rejects_remote_plaintext_and_credentials() {
        assert!(websocket_url("https://example.supabase.co", "key")
            .unwrap()
            .starts_with("wss:"));
        assert!(websocket_url("http://127.0.0.1:54321", "key")
            .unwrap()
            .starts_with("ws:"));
        for url in [
            "http://example.com",
            "https://user:password@example.com",
            "http://127.0.0.1:55555",
            "https://example.com/?secret=x",
        ] {
            assert!(websocket_url(url, "key").is_err());
        }
    }
    #[test]
    fn realtime_bigints_are_lossless_and_foreign_or_delete_events_ignored() {
        let record = serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","world_id":"22222222-2222-4222-8222-222222222222","author_key":"33333333-3333-4333-8333-333333333333","nickname":"n","avatar":"feminine","body":"hello","message_seq":9007199254740993i64,"change_seq":"9007199254740994","created_at":"2026-10-10T00:00:00Z","deleted_at":null});
        let frame = serde_json::json!({"topic":"realtime:group-chat:22222222-2222-4222-8222-222222222222","event":"postgres_changes","payload":{"data":{"schema":"public","table":"group_chat_messages","type":"INSERT","record":record}}});
        let message =
            parse_frame(&frame.to_string(), "22222222-2222-4222-8222-222222222222").unwrap();
        match message {
            Frame::Message(m) => assert_eq!(m.message_seq.get(), 9007199254740993),
            _ => panic!("missing message"),
        }
        let mut delete = frame.clone();
        delete["payload"]["data"]["type"] = serde_json::json!("DELETE");
        assert!(matches!(
            parse_frame(&delete.to_string(), "22222222-2222-4222-8222-222222222222").unwrap(),
            Frame::Ignore
        ));
        assert!(matches!(
            parse_frame(&frame.to_string(), "44444444-4444-4444-8444-444444444444").unwrap(),
            Frame::Ignore
        ));
        assert!(parse_frame("malformed secret", "world").is_err());
    }
}
