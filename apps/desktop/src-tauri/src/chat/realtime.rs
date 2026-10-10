use super::{
    client::ChatClient,
    protocol::{self, Frame},
    state::ChatState,
    types::*,
};
use crate::sync::auth::AuthConfig;
use futures_util::{SinkExt, StreamExt};
use serde::Serialize;
use std::{future::Future, pin::Pin, sync::Arc, time::Duration};
use tokio::{
    sync::{watch, Mutex},
    task::JoinHandle,
    time::{interval, timeout, Instant},
};
use tokio_tungstenite::{connect_async, tungstenite::Message};

pub type SessionSource =
    Arc<dyn Fn() -> Pin<Box<dyn Future<Output = Result<String, ChatError>> + Send>> + Send + Sync>;
pub type EventSink = Arc<dyn Fn(ChatEvent) + Send + Sync>;
#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ConnectionStatus {
    Connecting,
    Connected,
    Reconnecting,
    Unavailable,
    Stopped,
}
#[derive(Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChatEvent {
    Connection {
        scope: ChatScope,
        status: ConnectionStatus,
    },
    Snapshot {
        scope: ChatScope,
        context: ChatContext,
        messages: Vec<ChatMessage>,
    },
    Message {
        scope: ChatScope,
        message: ChatMessage,
    },
}
struct Active {
    scope: ChatScope,
    cancel: watch::Sender<bool>,
    task: JoinHandle<()>,
}
pub struct ChatRuntime {
    config: AuthConfig,
    session: SessionSource,
    events: EventSink,
    active: Mutex<Option<Active>>,
}
impl ChatRuntime {
    pub fn new(config: AuthConfig, session: SessionSource, events: EventSink) -> Self {
        Self {
            config,
            session,
            events,
            active: Mutex::new(None),
        }
    }
    pub async fn start(&self, scope: ChatScope) -> Result<(), ChatError> {
        if !super::types::valid_id(&scope.world_id) {
            return Err(ChatError::InvalidInput);
        }
        let url = protocol::websocket_url(&self.config.base_url, &self.config.publishable_key)?;
        self.start_connection(scope, url).await
    }
    async fn start_connection(&self, scope: ChatScope, url: String) -> Result<(), ChatError> {
        let mut active = self.active.lock().await;
        if active
            .as_ref()
            .is_some_and(|a| a.scope == scope && !a.task.is_finished())
        {
            return Ok(());
        }
        if let Some(old) = active.take() {
            let _ = old.cancel.send(true);
            let _ = old.task.await;
        }
        let (cancel, mut cancelled) = watch::channel(false);
        let client = ChatClient::new(&self.config);
        let session = self.session.clone();
        let events = self.events.clone();
        let worker_scope = scope.clone();
        let task = tokio::spawn(async move {
            tokio::select! {
                biased;
                _=cancelled.changed()=>{
                    events(ChatEvent::Connection {
                        scope: worker_scope,
                        status: ConnectionStatus::Stopped,
                    });
                },
                _=run_loop(&url,worker_scope.clone(),client,session,events.clone())=>{},
            }
        });
        *active = Some(Active {
            scope,
            cancel,
            task,
        });
        Ok(())
    }
    pub async fn stop(&self, scope: &ChatScope) {
        let old = {
            let mut active = self.active.lock().await;
            if active.as_ref().is_some_and(|a| &a.scope == scope) {
                active.take()
            } else {
                None
            }
        };
        if let Some(old) = old {
            let _ = old.cancel.send(true);
            let _ = old.task.await;
        }
    }
    pub async fn stop_all(&self) {
        let old = self.active.lock().await.take();
        if let Some(old) = old {
            let _ = old.cancel.send(true);
            let _ = old.task.await;
        }
    }
}
impl Drop for ChatRuntime {
    fn drop(&mut self) {
        if let Some(old) = self.active.get_mut().take() {
            let _ = old.cancel.send(true);
            old.task.abort();
        }
    }
}
pub fn retry_delay(attempt: u32, jitter: u16) -> Duration {
    let base = [1000u64, 2000, 4000, 8000, 16000, 30000][attempt.min(5) as usize];
    Duration::from_millis((base + u64::from(jitter % 500)).min(30000))
}
async fn run_loop(
    url: &str,
    scope: ChatScope,
    client: ChatClient,
    session: SessionSource,
    events: EventSink,
) {
    let mut state = ChatState::new(scope.clone());
    let mut attempt = 0;
    loop {
        events(ChatEvent::Connection {
            scope: scope.clone(),
            status: if attempt == 0 {
                ConnectionStatus::Connecting
            } else {
                ConnectionStatus::Reconnecting
            },
        });
        let result = connection(
            url,
            &client,
            &session,
            &events,
            &mut state,
            Duration::from_secs(20),
        )
        .await;
        if matches!(
            result,
            Err(ChatError::Rejected(401 | 403) | ChatError::InvalidInput)
        ) {
            events(ChatEvent::Connection {
                scope: scope.clone(),
                status: ConnectionStatus::Unavailable,
            });
            return;
        }
        let jitter = u16::from_le_bytes([
            uuid::Uuid::new_v4().as_bytes()[0],
            uuid::Uuid::new_v4().as_bytes()[1],
        ]);
        tokio::time::sleep(retry_delay(attempt, jitter)).await;
        attempt = attempt.saturating_add(1);
    }
}
struct History {
    context: ChatContext,
    messages: Vec<ChatMessage>,
}
async fn history(
    client: &ChatClient,
    token: &str,
    state: &ChatState,
) -> Result<History, ChatError> {
    let world = &state.scope.world_id;
    let context = client.context(token, world).await?;
    let mut messages = client.list(token, world, None, 50).await?.messages;
    // Initial connection needs only the latest page; Realtime is already buffering.
    if state
        .context
        .as_ref()
        .is_some_and(|old| old.joined_after_seq == context.joined_after_seq)
    {
        let mut after = state.synced_change_seq;
        loop {
            let page = client
                .sync(token, world, after, context.last_change_seq, 50)
                .await?;
            messages.extend(page.messages);
            after = page.next_cursor;
            if !page.has_more {
                break;
            }
        }
    }
    Ok(History { context, messages })
}
async fn connection(
    url: &str,
    client: &ChatClient,
    session: &SessionSource,
    events: &EventSink,
    state: &mut ChatState,
    heartbeat: Duration,
) -> Result<(), ChatError> {
    let mut token = timeout(Duration::from_secs(5), session())
        .await
        .map_err(|_| ChatError::Transport)??;
    let (socket, _) = timeout(Duration::from_secs(10), connect_async(url))
        .await
        .map_err(|_| ChatError::Transport)?
        .map_err(|_| ChatError::Transport)?;
    let (mut sink, mut stream) = socket.split();
    let world = state.scope.world_id.clone();
    sink.send(Message::Text(
        protocol::join_frame(&world, &token, 1).to_string().into(),
    ))
    .await
    .map_err(|_| ChatError::Transport)?;
    let join = async {
        let mut buffered = Vec::new();
        let mut joined = false;
        let mut ready = false;
        // Phoenix joining precedes replication readiness. Querying history before
        // both signals can miss writes made before the listener starts streaming.
        loop {
            let next = stream
                .next()
                .await
                .ok_or(ChatError::Transport)?
                .map_err(|_| ChatError::Transport)?;
            if let Message::Text(text) = next {
                match protocol::parse_frame(text.as_str(), &world)? {
                    Frame::Reply { reference, ok } if reference == "1" => {
                        if !ok {
                            return Err(ChatError::Rejected(403));
                        }
                        joined = true;
                    }
                    Frame::PostgresReady => ready = true,
                    Frame::Closed => return Err(ChatError::Transport),
                    Frame::Message(message) => {
                        buffered.push(message);
                        if buffered.len() > 2000 {
                            return Err(ChatError::Transport);
                        }
                    }
                    _ => {}
                }
            } else if matches!(next, Message::Close(_)) {
                return Err(ChatError::Transport);
            } else if let Message::Ping(data) = next {
                sink.send(Message::Pong(data))
                    .await
                    .map_err(|_| ChatError::Transport)?;
            }
            if joined && ready {
                return Ok(buffered);
            }
        }
    };
    let mut buffered = timeout(Duration::from_secs(10), join)
        .await
        .map_err(|_| ChatError::Transport)??;
    let mut ticks = interval(heartbeat);
    ticks.tick().await;
    let mut reference = 1u64;
    let mut pending_heartbeat: Option<(String, Instant)> = None;
    let history = {
        let sync = history(client, &token, state);
        tokio::pin!(sync);
        loop {
            tokio::select! {
                result=&mut sync=>break result?,
                next=stream.next()=>{
                    let next=next.ok_or(ChatError::Transport)?.map_err(|_|ChatError::Transport)?;
                    if let Message::Text(text)=next {
                        match protocol::parse_frame(text.as_str(),&world)? {
                            Frame::Message(message)=>{buffered.push(message);if buffered.len()>2000{return Err(ChatError::Transport);}},
                            Frame::Closed=>return Err(ChatError::Transport),
                            Frame::Reply{reference:r,ok}=>{
                                if pending_heartbeat.as_ref().is_some_and(|(p,_)|p==&r){if !ok{return Err(ChatError::Transport);}pending_heartbeat=None;}
                            },
                            _=>{},
                        }
                    }else if matches!(next,Message::Close(_)){return Err(ChatError::Transport);}
                },
                _=ticks.tick()=>{
                    if pending_heartbeat.is_some(){return Err(ChatError::Transport);}
                    reference+=1;
                    sink.send(Message::Text(protocol::heartbeat_frame(reference).to_string().into())).await.map_err(|_|ChatError::Transport)?;
                    pending_heartbeat=Some((reference.to_string(),Instant::now()));
                },
            }
        }
    };
    let mut synchronized = ChatState::new(state.scope.clone());
    synchronized.context(history.context.clone())?;
    for message in state
        .messages()
        .into_iter()
        .chain(history.messages)
        .chain(buffered)
    {
        synchronized.merge(&state.scope, message)?;
    }
    synchronized.synced_change_seq = history.context.last_change_seq;
    *state = synchronized;
    events(ChatEvent::Snapshot {
        scope: state.scope.clone(),
        context: history.context,
        messages: state.messages(),
    });
    events(ChatEvent::Connection {
        scope: state.scope.clone(),
        status: ConnectionStatus::Connected,
    });
    loop {
        tokio::select! {
            next=stream.next()=> {
                let next=next.ok_or(ChatError::Transport)?.map_err(|_|ChatError::Transport)?;
                match next {
                    Message::Text(text)=>match protocol::parse_frame(text.as_str(),&world)? {
                        Frame::Message(message)=>{if state.merge(&state.scope.clone(),message.clone())? {events(ChatEvent::Message{scope:state.scope.clone(),message});}},
                        Frame::Reply{reference:r,ok}=>{
                            if !ok{return Err(ChatError::Rejected(403));}
                            if pending_heartbeat.as_ref().is_some_and(|(p,_)|p==&r){pending_heartbeat=None;}
                        },
                        Frame::Closed=>return Err(ChatError::Transport),
                        _=>{},
                    },
                    Message::Ping(data)=>sink.send(Message::Pong(data)).await.map_err(|_|ChatError::Transport)?,
                    Message::Close(_)=>return Err(ChatError::Transport),
                    _=>{},
                }
            },
            _=ticks.tick()=> {
                if pending_heartbeat.is_some(){return Err(ChatError::Transport);}
                let fresh=timeout(Duration::from_secs(5),session()).await.map_err(|_|ChatError::Transport)??;
                if token!=fresh {
                    token=fresh;reference+=1;
                    sink.send(Message::Text(protocol::token_frame(&world,&token,reference).to_string().into())).await.map_err(|_|ChatError::Transport)?;
                }
                reference+=1;
                sink.send(Message::Text(protocol::heartbeat_frame(reference).to_string().into())).await.map_err(|_|ChatError::Transport)?;
                pending_heartbeat=Some((reference.to_string(),Instant::now()));
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reconnect_delay_is_bounded_and_has_jitter() {
        assert!(retry_delay(0, 0).as_millis() >= 1000);
        assert!(retry_delay(1, 123).as_millis() > 2000);
        assert!(retry_delay(100, 999).as_secs() <= 30);
    }
    use std::{
        io::{Read, Write},
        net::{TcpListener, TcpStream},
        thread::{self, JoinHandle as ThreadHandle},
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
    }

    fn spawn_rpc_sequence(
        responses: Vec<MockResponse>,
    ) -> (String, ThreadHandle<Vec<CapturedRequest>>) {
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
        let MockResponse::Json(status, body) = response;
        {
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

    fn row(deleted: bool, change: i64) -> serde_json::Value {
        serde_json::json!({"id":"11111111-1111-4111-8111-111111111111","world_id":"22222222-2222-4222-8222-222222222222","author_key":"33333333-3333-4333-8333-333333333333","nickname":"n","avatar":"masculine","message_seq":"1","change_seq":change.to_string(),"body":if deleted{None}else{Some("hello")},"created_at":"2026-10-10T00:00:00Z","deleted_at":if deleted{Some("2026-10-10T01:00:00Z")}else{None}})
    }
    fn context(change: i64) -> serde_json::Value {
        serde_json::json!({"world_id":"22222222-2222-4222-8222-222222222222","author_key":"33333333-3333-4333-8333-333333333333","joined_after_seq":"0","last_read_seq":"0","last_message_seq":"1","last_change_seq":change.to_string(),"unread_count":"0"})
    }
    fn change_frame(deleted: bool, change: i64) -> String {
        serde_json::json!({"topic":protocol::topic("22222222-2222-4222-8222-222222222222"),"event":"postgres_changes","payload":{"data":{"schema":"public","table":"group_chat_messages","type":if deleted{"UPDATE"}else{"INSERT"},"record":row(deleted,change)}}}).to_string()
    }
    fn readiness_frame(status: &str) -> String {
        serde_json::json!({"topic":"realtime:group-chat:22222222-2222-4222-8222-222222222222","event":"system","payload":{"extension":"postgres_changes","status":status,"message":"Subscribed to PostgreSQL","channel":"main"}}).to_string()
    }

    #[tokio::test]
    async fn delayed_readiness_blocks_history_and_merges_buffered_tombstone() {
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = ChatClient::new(&AuthConfig {
            base_url: format!("http://{}", http.local_addr().unwrap()),
            publishable_key: "mock".into(),
        });
        let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", ws.local_addr().unwrap());
        let (joined_tx, joined_rx) = tokio::sync::oneshot::channel();
        let (ready_tx, ready_rx) = tokio::sync::oneshot::channel();
        let (close_tx, close_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (tcp, _) = ws.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            socket.next().await.unwrap().unwrap();
            socket.send(Message::Text(serde_json::json!({"topic":"realtime:group-chat:22222222-2222-4222-8222-222222222222","event":"phx_reply","ref":"1","payload":{"status":"ok"}}).to_string().into())).await.unwrap();
            socket
                .send(Message::Text(change_frame(true, 2).into()))
                .await
                .unwrap();
            joined_tx.send(()).unwrap();
            ready_rx.await.unwrap();
            socket
                .send(Message::Text(readiness_frame("ok").into()))
                .await
                .unwrap();
            close_rx.await.unwrap();
            socket.close(None).await.unwrap();
        });
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = seen.clone();
        let events: EventSink = Arc::new(move |event| captured.lock().unwrap().push(event));
        let session: SessionSource = Arc::new(|| Box::pin(async { Ok("mock".into()) }));
        let worker = tokio::spawn(async move {
            let mut state = ChatState::new(ChatScope {
                world_id: "22222222-2222-4222-8222-222222222222".into(),
                generation: 1,
            });
            let result = connection(
                &url,
                &client,
                &session,
                &events,
                &mut state,
                Duration::from_secs(20),
            )
            .await;
            (result, state)
        });
        joined_rx.await.unwrap();
        assert!(
            timeout(Duration::from_millis(100), http.accept())
                .await
                .is_err(),
            "history started before replication readiness"
        );
        assert!(
            seen.lock().unwrap().is_empty(),
            "no snapshot or Connected before readiness"
        );
        ready_tx.send(()).unwrap();
        for body in [
            context(1),
            serde_json::json!({"messages":[row(false,1)],"next_cursor":null,"has_more":false}),
        ] {
            let (mut stream, _) = timeout(Duration::from_secs(1), http.accept())
                .await
                .unwrap()
                .unwrap();
            use tokio::io::{AsyncReadExt, AsyncWriteExt};
            let mut request = vec![0; 4096];
            stream.read(&mut request).await.unwrap();
            let body = body.to_string();
            stream.write_all(format!("HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).as_bytes()).await.unwrap();
        }
        timeout(Duration::from_secs(1), async {
            loop {
                if seen.lock().unwrap().iter().any(|event| {
                    matches!(
                        event,
                        ChatEvent::Connection {
                            status: ConnectionStatus::Connected,
                            ..
                        }
                    )
                }) {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        close_tx.send(()).unwrap();
        let (result, state) = timeout(Duration::from_secs(1), worker)
            .await
            .unwrap()
            .unwrap();
        server.await.unwrap();
        assert!(matches!(result, Err(ChatError::Transport)));
        assert!(state.messages()[0].body.is_none());
        assert!(
            matches!(&seen.lock().unwrap()[0], ChatEvent::Snapshot { messages, .. } if messages[0].body.is_none())
        );
    }

    async fn readiness_failure_case(status: Option<&str>, cancel: bool) {
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let client = ChatClient::new(&AuthConfig {
            base_url: format!("http://{}", http.local_addr().unwrap()),
            publishable_key: "mock".into(),
        });
        let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", ws.local_addr().unwrap());
        let status = status.map(str::to_owned);
        let (joined_tx, joined_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (tcp, _) = ws.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            socket.next().await.unwrap().unwrap();
            socket.send(Message::Text(serde_json::json!({"topic":"realtime:group-chat:22222222-2222-4222-8222-222222222222","event":"phx_reply","ref":"1","payload":{"status":"ok"}}).to_string().into())).await.unwrap();
            if let Some(status) = status {
                socket
                    .send(Message::Text(readiness_frame(&status).into()))
                    .await
                    .unwrap();
            }
            joined_tx.send(()).unwrap();
            // Hold the socket open: only the client's readiness deadline or
            // cancellation may complete this case.
            let _ = socket.next().await;
        });
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = seen.clone();
        let events: EventSink = Arc::new(move |event| captured.lock().unwrap().push(event));
        let session: SessionSource = Arc::new(|| Box::pin(async { Ok("mock".into()) }));
        let worker = tokio::spawn(async move {
            let mut state = ChatState::new(ChatScope {
                world_id: "22222222-2222-4222-8222-222222222222".into(),
                generation: 1,
            });
            connection(
                &url,
                &client,
                &session,
                &events,
                &mut state,
                Duration::from_secs(20),
            )
            .await
        });
        joined_rx.await.unwrap();
        if cancel {
            // Dropping the connection future matches the runtime's cancellation
            // select, including disposal of its socket and buffered private rows.
            worker.abort();
            assert!(worker.await.unwrap_err().is_cancelled());
        } else {
            let result = timeout(Duration::from_secs(11), worker)
                .await
                .expect("readiness wait must be bounded")
                .unwrap();
            assert!(matches!(result, Err(ChatError::Transport)));
        }
        assert!(
            seen.lock().unwrap().is_empty(),
            "failed readiness must not publish a snapshot or Connected"
        );
        assert!(
            timeout(Duration::from_millis(25), http.accept())
                .await
                .is_err(),
            "failed readiness must not query history"
        );
        // Aborting this test server releases its held socket without stack changes.
        server.abort();
        let _ = server.await;
    }

    #[tokio::test]
    async fn readiness_error_and_server_timeout_prevent_sync() {
        readiness_failure_case(Some("error"), false).await;
        readiness_failure_case(Some("timeout"), false).await;
    }

    #[tokio::test]
    async fn missing_readiness_expires_without_sync() {
        readiness_failure_case(None, false).await;
    }

    #[tokio::test]
    async fn readiness_wait_can_be_cancelled_without_sync() {
        readiness_failure_case(None, true).await;
    }

    #[tokio::test]
    async fn socket_join_history_race_heartbeat_refresh_and_close() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            let (tcp, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let join = socket.next().await.unwrap().unwrap();
            let value: serde_json::Value = serde_json::from_str(join.to_text().unwrap()).unwrap();
            assert_eq!(value["event"], "phx_join");
            assert_eq!(value["payload"]["access_token"], "old");
            socket.send(Message::Text(serde_json::json!({"topic":value["topic"],"event":"phx_reply","ref":"1","payload":{"status":"ok"}}).to_string().into())).await.unwrap();
            socket
                .send(Message::Text(change_frame(true, 2).into()))
                .await
                .unwrap();
            socket
                .send(Message::Text(change_frame(false, 1).into()))
                .await
                .unwrap();
            socket
                .send(Message::Text(readiness_frame("ok").into()))
                .await
                .unwrap();
            let mut refreshed = false;
            loop {
                let frame = socket.next().await.unwrap().unwrap();
                let frame: serde_json::Value =
                    serde_json::from_str(frame.to_text().unwrap()).unwrap();
                if frame["event"] == "access_token" {
                    assert_eq!(frame["payload"]["access_token"], "fresh");
                    refreshed = true;
                }
                if frame["event"] == "heartbeat" {
                    assert!(refreshed);
                    socket.send(Message::Text(serde_json::json!({"topic":"phoenix","event":"phx_reply","ref":frame["ref"],"payload":{"status":"ok"}}).to_string().into())).await.unwrap();
                    socket.close(None).await.unwrap();
                    break;
                }
            }
        });
        let (http, requests) = spawn_rpc_sequence(vec![
            MockResponse::Json(200, context(1).to_string()),
            MockResponse::Json(
                200,
                serde_json::json!({"messages":[row(false,1)],"next_cursor":null,"has_more":false})
                    .to_string(),
            ),
        ]);
        let client = ChatClient::new(&AuthConfig {
            base_url: http,
            publishable_key: "mock".into(),
        });
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        let session: SessionSource = Arc::new(move || {
            let token = if count.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                "old"
            } else {
                "fresh"
            };
            Box::pin(async move { Ok(token.into()) })
        });
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink_seen = seen.clone();
        let sink: EventSink = Arc::new(move |event| sink_seen.lock().unwrap().push(event));
        let mut state = ChatState::new(ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        });
        assert!(matches!(
            timeout(
                Duration::from_secs(3),
                connection(
                    &url,
                    &client,
                    &session,
                    &sink,
                    &mut state,
                    Duration::from_millis(50)
                )
            )
            .await
            .unwrap(),
            Err(ChatError::Transport)
        ));
        server.await.unwrap();
        let captured = requests.join().unwrap();
        assert_eq!(captured.len(), 2);
        assert_eq!(captured[0].method, "POST");
        assert_eq!(captured[0].path, "/rest/v1/rpc/get_group_chat_context");
        assert_eq!(
            captured[0].headers.get("authorization").unwrap(),
            "Bearer old"
        );
        assert!(state.messages()[0].body.is_none());
        assert_eq!(state.synced_change_seq.get(), 1);
        assert!(seen
            .lock()
            .unwrap()
            .iter()
            .any(|e| matches!(e,ChatEvent::Snapshot{messages,..} if messages[0].body.is_none())));
    }
    #[tokio::test]
    async fn exact_scope_stop_closes_socket_waiting_for_readiness_without_sync() {
        let http = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let ws = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", ws.local_addr().unwrap());
        let (waiting_tx, waiting_rx) = tokio::sync::oneshot::channel();
        let server = tokio::spawn(async move {
            let (tcp, _) = ws.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
            let join = socket.next().await.unwrap().unwrap();
            let join: serde_json::Value = serde_json::from_str(join.to_text().unwrap()).unwrap();
            assert_eq!(join["event"], "phx_join");
            socket.send(Message::Text(serde_json::json!({"topic":join["topic"],"event":"phx_reply","ref":"1","payload":{"status":"ok"}}).to_string().into())).await.unwrap();
            socket
                .send(Message::Text(change_frame(true, 2).into()))
                .await
                .unwrap();
            // Pong proves the real connection consumed the successful join and
            // buffered row, and is still polling the socket without readiness.
            socket.send(Message::Ping(vec![7].into())).await.unwrap();
            assert!(
                matches!(socket.next().await.unwrap().unwrap(), Message::Pong(data) if data.as_ref() == [7])
            );
            waiting_tx.send(()).unwrap();
            assert!(
                matches!(
                    socket.next().await,
                    None | Some(Err(_)) | Some(Ok(Message::Close(_)))
                ),
                "stop must close the active socket"
            );
        });
        let seen = Arc::new(std::sync::Mutex::new(Vec::new()));
        let captured = seen.clone();
        let runtime = ChatRuntime::new(
            AuthConfig {
                base_url: format!("http://{}", http.local_addr().unwrap()),
                publishable_key: "mock".into(),
            },
            Arc::new(|| Box::pin(async { Ok("mock".into()) })),
            Arc::new(move |event| captured.lock().unwrap().push(event)),
        );
        let scope = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        // Production start validates its URL before using this same private
        // lifecycle path. The mock uses an ephemeral port to leave scratch alone.
        runtime.start_connection(scope.clone(), url).await.unwrap();
        timeout(Duration::from_secs(1), waiting_rx)
            .await
            .unwrap()
            .unwrap();
        let worker = runtime
            .active
            .lock()
            .await
            .as_ref()
            .unwrap()
            .task
            .abort_handle();
        runtime
            .stop(&ChatScope {
                generation: 0,
                ..scope.clone()
            })
            .await;
        assert!(runtime.active.lock().await.is_some());
        timeout(Duration::from_millis(250), runtime.stop(&scope))
            .await
            .expect("exact-scope stop must interrupt readiness promptly");
        assert!(runtime.active.lock().await.is_none());
        assert!(
            worker.is_finished(),
            "stop must await the worker's termination"
        );
        timeout(Duration::from_secs(1), server)
            .await
            .unwrap()
            .unwrap();
        assert!(
            timeout(Duration::from_millis(25), http.accept())
                .await
                .is_err(),
            "stop before readiness must not query history"
        );
        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 2);
        assert!(
            matches!(&seen[0], ChatEvent::Connection { scope: s, status: ConnectionStatus::Connecting } if s == &scope)
        );
        assert!(
            matches!(&seen[1], ChatEvent::Connection { scope: s, status: ConnectionStatus::Stopped } if s == &scope)
        );
    }

    #[tokio::test]
    async fn cancellation_interrupts_pending_auth_and_start_stop_are_idempotent() {
        let calls = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let count = calls.clone();
        let session: SessionSource = Arc::new(move || {
            count.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            Box::pin(std::future::pending())
        });
        let statuses = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = statuses.clone();
        let runtime = ChatRuntime::new(
            AuthConfig {
                base_url: "https://example.invalid".into(),
                publishable_key: "mock".into(),
            },
            session,
            Arc::new(move |event| {
                if let ChatEvent::Connection { scope, status } = event {
                    seen.lock().unwrap().push((scope, status));
                }
            }),
        );
        let scope = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        runtime.start(scope.clone()).await.unwrap();
        tokio::task::yield_now().await;
        runtime.start(scope.clone()).await.unwrap();
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
        runtime
            .stop(&ChatScope {
                generation: 0,
                ..scope.clone()
            })
            .await;
        assert!(runtime.active.lock().await.is_some());
        timeout(Duration::from_millis(200), runtime.stop(&scope))
            .await
            .unwrap();
        runtime.stop(&scope).await;
        assert!(runtime.active.lock().await.is_none());
        assert_eq!(
            *statuses.lock().unwrap(),
            vec![
                (scope.clone(), ConnectionStatus::Connecting),
                (scope, ConnectionStatus::Stopped)
            ]
        );
    }
    #[test]
    fn reconnect_history_recovers_offline_tombstone_across_pages() {
        let (http, requests) = spawn_rpc_sequence(vec![
            MockResponse::Json(200, context(3).to_string()),
            MockResponse::Json(
                200,
                serde_json::json!({"messages":[row(true,3)],"next_cursor":null,"has_more":false})
                    .to_string(),
            ),
            MockResponse::Json(
                200,
                serde_json::json!({"messages":[row(true,2)],"next_cursor":"2","has_more":true})
                    .to_string(),
            ),
            MockResponse::Json(
                200,
                serde_json::json!({"messages":[row(true,3)],"next_cursor":"3","has_more":false})
                    .to_string(),
            ),
        ]);
        let client = ChatClient::new(&AuthConfig {
            base_url: http,
            publishable_key: "mock".into(),
        });
        let mut state = ChatState::new(ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        });
        state
            .context(serde_json::from_value(context(1)).unwrap())
            .unwrap();
        state.synced_change_seq = ChatSeq::new(1).unwrap();
        let batch = run_async(history(&client, "mock-token", &state)).unwrap();
        assert_eq!(batch.context.last_change_seq.get(), 3);
        for message in batch.messages {
            state.merge(&state.scope.clone(), message).unwrap();
        }
        assert!(state.messages()[0].body.is_none());
        assert_eq!(state.messages()[0].change_seq.get(), 3);
        let requests = requests.join().unwrap();
        assert_eq!(requests[2].body["p_after_change_seq"], "1");
        assert_eq!(requests[3].body["p_after_change_seq"], "2");
    }
    #[tokio::test]
    async fn disconnected_join_reconnects_and_rejected_join_stops() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("ws://{}", listener.local_addr().unwrap());
        let server = tokio::spawn(async move {
            for attempt in 0..2 {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let join = socket.next().await.unwrap().unwrap();
                let frame: serde_json::Value =
                    serde_json::from_str(join.to_text().unwrap()).unwrap();
                if attempt == 0 {
                    socket.close(None).await.unwrap();
                } else {
                    socket.send(Message::Text(serde_json::json!({"topic":frame["topic"],"event":"phx_reply","ref":"1","payload":{"status":"error","response":{"reason":"private secret"}}}).to_string().into())).await.unwrap();
                }
            }
        });
        let session: SessionSource = Arc::new(|| Box::pin(async { Ok("mock-token".into()) }));
        let client = ChatClient::new(&AuthConfig {
            base_url: "https://example.invalid".into(),
            publishable_key: "mock".into(),
        });
        let statuses = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = statuses.clone();
        let sink: EventSink = Arc::new(move |e| {
            if let ChatEvent::Connection { status, .. } = e {
                seen.lock().unwrap().push(status);
            }
        });
        let scope = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        timeout(
            Duration::from_secs(3),
            run_loop(&url, scope, client, session, sink),
        )
        .await
        .unwrap();
        server.await.unwrap();
        assert_eq!(
            *statuses.lock().unwrap(),
            vec![
                ConnectionStatus::Connecting,
                ConnectionStatus::Reconnecting,
                ConnectionStatus::Unavailable
            ]
        );
    }
    #[tokio::test]
    async fn terminal_unavailable_is_not_overwritten_by_stopped() {
        let events = Arc::new(std::sync::Mutex::new(Vec::new()));
        let seen = events.clone();
        let session: SessionSource = Arc::new(|| Box::pin(async { Err(ChatError::Rejected(401)) }));
        let runtime = ChatRuntime::new(
            AuthConfig {
                base_url: "https://example.invalid".into(),
                publishable_key: "mock".into(),
            },
            session,
            Arc::new(move |event| {
                if let ChatEvent::Connection { status, .. } = event {
                    seen.lock().unwrap().push(status);
                }
            }),
        );
        let scope = ChatScope {
            world_id: "22222222-2222-4222-8222-222222222222".into(),
            generation: 1,
        };
        runtime.start(scope.clone()).await.unwrap();
        timeout(Duration::from_secs(1), async {
            loop {
                if runtime
                    .active
                    .lock()
                    .await
                    .as_ref()
                    .unwrap()
                    .task
                    .is_finished()
                {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            *events.lock().unwrap(),
            vec![ConnectionStatus::Connecting, ConnectionStatus::Unavailable]
        );
        runtime.stop(&scope).await;
        assert_eq!(
            events.lock().unwrap().last(),
            Some(&ConnectionStatus::Unavailable)
        );
    }
}
