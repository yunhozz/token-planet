use super::types::*;
use crate::sync::{auth::AuthConfig, client::shared_http_client};
use reqwest::Client;
use serde::de::DeserializeOwned;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct ChatClient {
    http: Client,
    base_url: String,
    publishable_key: String,
}
fn send_body(world: &str, request: &str, body: &str) -> Value {
    json!({"p_world_id":world,"p_request_id":request,"p_body":body})
}
impl ChatClient {
    pub fn new(config: &AuthConfig) -> Self {
        Self {
            http: shared_http_client(),
            base_url: config.base_url.trim_end_matches('/').into(),
            publishable_key: config.publishable_key.clone(),
        }
    }
    async fn rpc<T: DeserializeOwned>(
        &self,
        token: &str,
        function: &str,
        body: Value,
    ) -> Result<T, ChatError> {
        let response = self
            .http
            .post(format!("{}/rest/v1/rpc/{function}", self.base_url))
            .header("apikey", &self.publishable_key)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .map_err(|_| ChatError::Transport)?;
        if !response.status().is_success() {
            return Err(ChatError::Rejected(response.status().as_u16()));
        }
        response
            .json()
            .await
            .map_err(|_| ChatError::InvalidResponse)
    }
    pub async fn context(&self, token: &str, world: &str) -> Result<ChatContext, ChatError> {
        if !valid_id(world) {
            return Err(ChatError::InvalidInput);
        }
        let result: ChatContext = self
            .rpc(token, "get_group_chat_context", json!({"p_world_id":world}))
            .await?;
        result.validate(world)?;
        Ok(result)
    }
    pub async fn send(
        &self,
        token: &str,
        world: &str,
        request: &str,
        body: &str,
    ) -> Result<ChatMessage, ChatError> {
        if !valid_id(world)
            || !valid_id(request)
            || body.trim().is_empty()
            || body.chars().count() > 2000
        {
            return Err(ChatError::InvalidInput);
        }
        let result: ChatMessage = self
            .rpc(
                token,
                "send_group_chat_message",
                send_body(world, request, body),
            )
            .await?;
        result.validate(world)?;
        // A retry may return a tombstone, but an active response must match the submitted body.
        if result.body.as_deref().is_some_and(|text| text != body) {
            return Err(ChatError::InvalidResponse);
        }
        Ok(result)
    }
    pub async fn delete(
        &self,
        token: &str,
        world: &str,
        message: &str,
    ) -> Result<ChatMessage, ChatError> {
        if !valid_id(world) || !valid_id(message) {
            return Err(ChatError::InvalidInput);
        }
        let result: ChatMessage = self
            .rpc(
                token,
                "delete_group_chat_message",
                json!({"p_world_id":world,"p_message_id":message}),
            )
            .await?;
        result.validate(world)?;
        if result.id != message || result.deleted_at.is_none() {
            return Err(ChatError::InvalidResponse);
        }
        Ok(result)
    }
    pub async fn mark_read(
        &self,
        token: &str,
        world: &str,
        sequence: ChatSeq,
    ) -> Result<ChatReadState, ChatError> {
        if !valid_id(world) {
            return Err(ChatError::InvalidInput);
        }
        let result: ChatReadState = self
            .rpc(
                token,
                "mark_group_chat_read",
                json!({"p_world_id":world,"p_message_seq":sequence}),
            )
            .await?;
        if result.last_read_seq != sequence {
            return Err(ChatError::InvalidResponse);
        }
        Ok(result)
    }
    pub async fn list(
        &self,
        token: &str,
        world: &str,
        before: Option<ChatSeq>,
        limit: u8,
    ) -> Result<ChatPage, ChatError> {
        if !valid_id(world) || !(1..=50).contains(&limit) {
            return Err(ChatError::InvalidInput);
        }
        let result: ChatPage = self
            .rpc(
                token,
                "list_group_chat_messages",
                json!({"p_world_id":world,"p_before_seq":before,"p_limit":limit}),
            )
            .await?;
        if result.messages.len() > usize::from(limit)
            || result.has_more != result.next_cursor.is_some()
        {
            return Err(ChatError::InvalidResponse);
        }
        for (i, message) in result.messages.iter().enumerate() {
            message.validate(world)?;
            if before.is_some_and(|seq| message.message_seq >= seq)
                || (i > 0 && result.messages[i - 1].message_seq >= message.message_seq)
            {
                return Err(ChatError::InvalidResponse);
            }
        }
        if result.has_more && result.messages.first().map(|m| m.message_seq) != result.next_cursor {
            return Err(ChatError::InvalidResponse);
        }
        Ok(result)
    }
    pub async fn sync(
        &self,
        token: &str,
        world: &str,
        after: ChatSeq,
        until: ChatSeq,
        limit: u8,
    ) -> Result<ChatChangePage, ChatError> {
        if !valid_id(world) || !(1..=50).contains(&limit) || after > until {
            return Err(ChatError::InvalidInput);
        }
        let result: ChatChangePage=self.rpc(token,"sync_group_chat_changes",json!({"p_world_id":world,"p_after_change_seq":after,"p_until_change_seq":until,"p_limit":limit})).await?;
        if result.messages.len() > usize::from(limit)
            || result.next_cursor < after
            || result.next_cursor > until
        {
            return Err(ChatError::InvalidResponse);
        }
        for (i, message) in result.messages.iter().enumerate() {
            message.validate(world)?;
            if message.change_seq <= after
                || message.change_seq > until
                || (i > 0 && result.messages[i - 1].change_seq >= message.change_seq)
            {
                return Err(ChatError::InvalidResponse);
            }
        }
        if (result.has_more
            && (result.messages.last().map(|m| m.change_seq) != Some(result.next_cursor)
                || result.next_cursor <= after))
            || (!result.has_more && result.next_cursor != until)
        {
            return Err(ChatError::InvalidResponse);
        }
        Ok(result)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn send_payload_has_only_authorized_arguments() {
        let body = send_body("world", "request", "hello");
        assert_eq!(
            body,
            serde_json::json!({"p_world_id":"world","p_request_id":"request","p_body":"hello"})
        );
    }
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread::{self, JoinHandle},
    };
    #[derive(Debug)]
    struct CapturedRequest {
        method: String,
        path: String,
        headers: std::collections::HashMap<String, String>,
        body: serde_json::Value,
    }

    enum MockResponse {
        Json(u16, String),
        Disconnect,
    }

    fn spawn_rpc_server(response: MockResponse) -> (String, JoinHandle<CapturedRequest>) {
        let (url, server) = spawn_rpc_sequence(vec![response]);
        let thread = thread::spawn(move || server.join().unwrap().remove(0));
        (url, thread)
    }

    fn spawn_rpc_sequence(
        responses: Vec<MockResponse>,
    ) -> (String, JoinHandle<Vec<CapturedRequest>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        let thread = thread::spawn(move || {
            responses
                .into_iter()
                .map(|response| {
                    let (mut stream, _) = listener.accept().unwrap();
                    stream
                        .set_read_timeout(Some(std::time::Duration::from_secs(5)))
                        .unwrap();
                    let captured = capture_request(&mut stream);
                    respond(&mut stream, response);
                    captured
                })
                .collect()
        });
        (format!("http://{address}"), thread)
    }

    fn respond(stream: &mut TcpStream, response: MockResponse) {
        if let MockResponse::Json(status, body) = response {
            let reason = if status == 200 { "OK" } else { "Rejected" };
            write!(stream, "HTTP/1.1 {status} {reason}\r\n").unwrap();
            write!(stream, "Content-Type: application/json\r\n").unwrap();
            write!(stream, "Content-Length: {}\r\n", body.len()).unwrap();
            write!(stream, "Connection: close\r\n\r\n{body}").unwrap();
            stream.flush().unwrap();
        }
    }

    fn capture_request(stream: &mut TcpStream) -> CapturedRequest {
        let mut bytes = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let count = stream.read(&mut buffer).unwrap();
            assert_ne!(count, 0, "client closed before sending its request");
            bytes.extend_from_slice(&buffer[..count]);
            if let Some(header_end) = bytes.windows(4).position(|window| window == b"\r\n\r\n") {
                let header_text = std::str::from_utf8(&bytes[..header_end]).unwrap();
                let content_length = header_text
                    .lines()
                    .skip(1)
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().unwrap())
                    })
                    .unwrap_or(0);
                if bytes.len() >= header_end + 4 + content_length {
                    let mut lines = header_text.lines();
                    let mut request_line = lines.next().unwrap().split_whitespace();
                    let method = request_line.next().unwrap().to_owned();
                    let path = request_line.next().unwrap().to_owned();
                    let headers = lines
                        .filter_map(|line| {
                            let (name, value) = line.split_once(':')?;
                            Some((name.to_ascii_lowercase(), value.trim().to_owned()))
                        })
                        .collect();
                    let body_start = header_end + 4;
                    let body_bytes = &bytes[body_start..body_start + content_length];
                    let body = if body_bytes.is_empty() {
                        serde_json::Value::Null
                    } else {
                        serde_json::from_slice(body_bytes).unwrap()
                    };
                    return CapturedRequest {
                        method,
                        path,
                        headers,
                        body,
                    };
                }
            }
        }
    }

    fn run_async<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(future)
    }

    fn message() -> serde_json::Value {
        serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","world_id":"22222222-2222-4222-8222-222222222222",
            "author_key":"33333333-3333-4333-8333-333333333333","nickname":"행성 동기화 대기","avatar":"masculine",
            "message_seq":"1","change_seq":"1","body":"hello","created_at":"2026-10-10T00:00:00Z","deleted_at":null})
    }
    #[test]
    fn send_rpc_uses_existing_auth_headers_and_server_profile() {
        let (url, server) = spawn_rpc_server(MockResponse::Json(200, message().to_string()));
        let client = ChatClient::new(&AuthConfig {
            base_url: url,
            publishable_key: "test-key".into(),
        });
        let result = run_async(client.send(
            "test-token",
            "22222222-2222-4222-8222-222222222222",
            "44444444-4444-4444-8444-444444444444",
            "hello",
        ))
        .unwrap();
        assert_eq!(result.nickname, "행성 동기화 대기");
        let request = server.join().unwrap();
        assert_eq!(request.method, "POST");
        assert_eq!(request.path, "/rest/v1/rpc/send_group_chat_message");
        assert_eq!(
            request.headers.get("authorization").unwrap(),
            "Bearer test-token"
        );
        assert_eq!(request.headers.get("apikey").unwrap(), "test-key");
        assert_eq!(request.body.as_object().unwrap().len(), 3);
    }
    #[test]
    fn rejected_and_malformed_responses_do_not_expose_server_body() {
        for (status, body, expected) in [
            (403, "secret message and token", ChatError::Rejected(403)),
            (200, "secret message and token", ChatError::InvalidResponse),
        ] {
            let (url, server) = spawn_rpc_server(MockResponse::Json(status, body.into()));
            let client = ChatClient::new(&AuthConfig {
                base_url: url,
                publishable_key: "test-key".into(),
            });
            assert_eq!(
                run_async(client.context("test-token", "22222222-2222-4222-8222-222222222222")),
                Err(expected)
            );
            server.join().unwrap();
        }
    }
    #[test]
    fn transport_disconnect_is_a_static_error() {
        let (url, server) = spawn_rpc_server(MockResponse::Disconnect);
        let client = ChatClient::new(&AuthConfig {
            base_url: url,
            publishable_key: "test-key".into(),
        });
        assert_eq!(
            run_async(client.context("test-token", "22222222-2222-4222-8222-222222222222")),
            Err(ChatError::Transport)
        );
        server.join().unwrap();
    }
    #[test]
    fn all_read_rpc_names_and_cursor_arguments_match_database_contract() {
        let world = "22222222-2222-4222-8222-222222222222";
        let context = serde_json::json!({"world_id":world,"joined_after_seq":"0","author_key":"33333333-3333-4333-8333-333333333333","last_read_seq":"0","last_message_seq":"1","last_change_seq":"1","unread_count":"0"});
        let (url, server) = spawn_rpc_sequence(vec![
            MockResponse::Json(200, context.to_string()),
            MockResponse::Json(
                200,
                serde_json::json!({"messages":[message()],"next_cursor":null,"has_more":false})
                    .to_string(),
            ),
            MockResponse::Json(
                200,
                serde_json::json!({"messages":[message()],"next_cursor":"1","has_more":false})
                    .to_string(),
            ),
            MockResponse::Json(
                200,
                serde_json::json!({"last_read_seq":"1","unread_count":"0"}).to_string(),
            ),
        ]);
        let client = ChatClient::new(&AuthConfig {
            base_url: url,
            publishable_key: "test-key".into(),
        });
        run_async(async {
            client.context("token", world).await.unwrap();
            client.list("token", world, None, 50).await.unwrap();
            client
                .sync(
                    "token",
                    world,
                    ChatSeq::new(0).unwrap(),
                    ChatSeq::new(1).unwrap(),
                    50,
                )
                .await
                .unwrap();
            client
                .mark_read("token", world, ChatSeq::new(1).unwrap())
                .await
                .unwrap();
        });
        let requests = server.join().unwrap();
        assert_eq!(
            requests.iter().map(|r| r.path.as_str()).collect::<Vec<_>>(),
            vec![
                "/rest/v1/rpc/get_group_chat_context",
                "/rest/v1/rpc/list_group_chat_messages",
                "/rest/v1/rpc/sync_group_chat_changes",
                "/rest/v1/rpc/mark_group_chat_read"
            ]
        );
        assert_eq!(
            requests[1].body,
            serde_json::json!({"p_world_id":world,"p_before_seq":null,"p_limit":50})
        );
        assert_eq!(
            requests[2].body,
            serde_json::json!({"p_world_id":world,"p_after_change_seq":"0","p_until_change_seq":"1","p_limit":50})
        );
        assert_eq!(
            requests[3].body,
            serde_json::json!({"p_world_id":world,"p_message_seq":"1"})
        );
    }
    #[test]
    fn delete_and_retry_accept_tombstones_without_restoring_body() {
        let world = "22222222-2222-4222-8222-222222222222";
        let id = "11111111-1111-4111-8111-111111111111";
        let mut tombstone = message();
        tombstone["body"] = serde_json::Value::Null;
        tombstone["deleted_at"] = serde_json::json!("2026-10-10T01:00:00Z");
        tombstone["change_seq"] = serde_json::json!("2");
        let (url, server) = spawn_rpc_sequence(vec![
            MockResponse::Json(200, tombstone.to_string()),
            MockResponse::Json(200, tombstone.to_string()),
        ]);
        let client = ChatClient::new(&AuthConfig {
            base_url: url,
            publishable_key: "test-key".into(),
        });
        assert!(run_async(client.delete("token", world, id))
            .unwrap()
            .body
            .is_none());
        assert!(run_async(client.send(
            "token",
            world,
            "44444444-4444-4444-8444-444444444444",
            "hello"
        ))
        .unwrap()
        .body
        .is_none());
        let requests = server.join().unwrap();
        assert_eq!(requests[0].path, "/rest/v1/rpc/delete_group_chat_message");
        assert_eq!(
            requests[0].body,
            serde_json::json!({"p_world_id":world,"p_message_id":id})
        );
    }
    #[test]
    fn cross_world_and_out_of_bounds_page_are_rejected() {
        let world = "22222222-2222-4222-8222-222222222222";
        let mut foreign = message();
        foreign["world_id"] = serde_json::json!("44444444-4444-4444-8444-444444444444");
        for payload in [
            serde_json::json!({"messages":[foreign],"next_cursor":null,"has_more":false}),
            serde_json::json!({"messages":[message()],"next_cursor":"1","has_more":true}),
        ] {
            let (url, server) = spawn_rpc_server(MockResponse::Json(200, payload.to_string()));
            let client = ChatClient::new(&AuthConfig {
                base_url: url,
                publishable_key: "test-key".into(),
            });
            assert!(matches!(
                run_async(client.list("token", world, Some(ChatSeq::new(1).unwrap()), 50)),
                Err(ChatError::InvalidResponse)
            ));
            server.join().unwrap();
        }
    }
}
