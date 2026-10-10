import { useEffect, useRef, useState } from "react";
import { chat, mergeChatMessages, queueChatLifecycle, sameChatScope } from "../lib/chat";
import type { ChatContext, ChatEvent, ChatMessage, ChatScope, ChatStatus } from "../types/chat";
function currentContext(previous: ChatContext | null, next: ChatContext): ChatContext {
  return previous && (BigInt(next.last_change_seq) < BigInt(previous.last_change_seq) || BigInt(next.last_read_seq) < BigInt(previous.last_read_seq)) ? previous : next;
}
function accessDenied(error: unknown) {
  return typeof error === "object" && error !== null && "rejected" in error && (error.rejected === 401 || error.rejected === 403);
}
const empty = () => ({ messages: [] as ChatMessage[], context: null as ChatContext | null, status: "connecting" as ChatStatus, ready: false, hasMore: false, cursor: null as string | null });
export function useGroupChat(worldId: string | null) {
  const [data, setData] = useState(empty);
  const dataRef = useRef(data); dataRef.current = data;
  const [draft, setDraft] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [sending, setSending] = useState(false);
  const [loadingOlder, setLoadingOlder] = useState(false);
  const [failed, setFailed] = useState<{ requestId: string; body: string } | null>(null);
  const scopeRef = useRef<ChatScope | null>(null);
  const lifetime = useRef(0);
  const sendingRef = useRef(false);
  const olderRef = useRef(false);
  const readRef = useRef(false);
  const terminalRef = useRef(false);
  const valid = (scope: ChatScope, epoch: number) => epoch === lifetime.current && sameChatScope(scopeRef.current, scope);
  function clearTerminal(status: ChatStatus = "unavailable") {
    const scope = scopeRef.current;
    terminalRef.current = true;
    scopeRef.current = null;
    setData({ ...empty(), status });
    setDraft(""); setFailed(null); setSending(false); setLoadingOlder(false);
    setError("채팅 연결이 종료되었습니다. 그룹 탭을 다시 열어 주세요.");
    if (scope) void queueChatLifecycle(() => chat.stop(scope)).catch(() => {});
  }
  useEffect(() => {
    ++lifetime.current;
    let active = true;
    let scope: ChatScope | null = null;
    let unlisten: (() => void) | undefined;
    const early: ChatEvent[] = [];
    setData(empty()); setDraft(""); setError(null); setFailed(null); setSending(false); setLoadingOlder(false);
    sendingRef.current = false; olderRef.current = false; readRef.current = false; scopeRef.current = null; terminalRef.current = false;
    function receive(event: ChatEvent) {
      if (!active || terminalRef.current || event.scope.world_id !== worldId) return;
      if (!scope) { early.push(event); return; }
      if (!sameChatScope(scope, event.scope)) return;
      if (event.kind === "connection") {
        if (event.status === "unavailable" || event.status === "stopped") {
          clearTerminal(event.status);
        } else setData(current => ({ ...current, status: event.status }));
      }
      else if (event.kind === "snapshot") setData(current => ({ ...current,
        context: currentContext(current.context, event.context),
        messages: mergeChatMessages(current.messages, event.messages, worldId!),
      }));
      else {
        setData(current => ({ ...current, messages: mergeChatMessages(current.messages, [event.message], worldId!) }));
        void refreshContext(scope, lifetime.current);
      }
    }
    if (worldId) void (async () => {
      try {
        unlisten = await chat.listen(receive);
        if (!active) { unlisten(); return; }
        const start = queueChatLifecycle(async () => {
          if (!active) return null;
          const allocated = await chat.start(worldId);
          if (!active) { await chat.stop(allocated).catch(() => {}); return null; }
          scope = allocated;
          return allocated;
        });
        const allocated = await start;
        if (!allocated || !active) return;
        scope = allocated;
        scopeRef.current = scope;
        early.splice(0).forEach(receive);
        if (terminalRef.current) return;
        const [context, page] = await Promise.all([chat.context(scope), chat.list(scope)]);
        if (!active || terminalRef.current) return;
        setData(current => ({ ...current, ready: true, hasMore: page.has_more, cursor: page.next_cursor,
          context: currentContext(current.context, context),
          messages: mergeChatMessages(current.messages, page.messages, worldId),
        }));
      } catch {
        if (active && !terminalRef.current) clearTerminal();
      }
    })();
    return () => {
      active = false; lifetime.current++;
      scopeRef.current = null;
      unlisten?.();
      if (scope) {
        const stopped = scope;
        void queueChatLifecycle(() => chat.stop(stopped)).catch(() => {});
      }
    };
  }, [worldId]);
  async function refreshContext(scope: ChatScope, epoch: number) {
    try {
      const context = await chat.context(scope);
      if (valid(scope, epoch)) setData(current => ({ ...current, context: currentContext(current.context, context) }));
    } catch (error) { if (valid(scope, epoch) && accessDenied(error)) clearTerminal(); }
  }
  async function deliver(request: { requestId: string; body: string }) {
    const scope = scopeRef.current, epoch = lifetime.current;
    if (!scope || sendingRef.current) return;
    sendingRef.current = true; setSending(true); setError(null);
    try {
      const message = await chat.send(scope, request.requestId, request.body);
      if (!valid(scope, epoch)) return;
      setData(current => ({ ...current, messages: mergeChatMessages(current.messages, [message], scope.world_id) }));
      setDraft(current => current === request.body ? "" : current); setFailed(null);
    } catch (error) {
      if (valid(scope, epoch)) {
        if (accessDenied(error)) clearTerminal();
        else { setFailed(request); setError("메시지를 보내지 못했습니다. 같은 메시지로 다시 시도하세요."); }
      }
    } finally {
      if (valid(scope, epoch)) { sendingRef.current = false; setSending(false); }
    }
  }
  async function send() {
    if (!data.ready || failed || !draft.trim() || Array.from(draft).length > 2000) return;
    await deliver({ requestId: crypto.randomUUID(), body: draft });
  }
  async function retry() { if (failed) await deliver(failed); }
  async function remove(id: string) {
    const scope = scopeRef.current, epoch = lifetime.current;
    if (!scope) return;
    try { const message = await chat.remove(scope, id); if (valid(scope, epoch)) setData(current => ({ ...current, messages: mergeChatMessages(current.messages, [message], scope.world_id) })); }
    catch (error) { if (valid(scope, epoch)) { if (accessDenied(error)) clearTerminal(); else setError("메시지를 삭제하지 못했습니다. 다시 시도하세요."); } }
  }
  async function loadOlder() {
    const scope = scopeRef.current, epoch = lifetime.current, current = dataRef.current;
    if (!scope || !current.hasMore || !current.cursor || olderRef.current) return;
    olderRef.current = true; setLoadingOlder(true);
    try {
      const page = await chat.list(scope, current.cursor);
      if (valid(scope, epoch)) setData(current => ({ ...current, hasMore: page.has_more, cursor: page.next_cursor, messages: mergeChatMessages(current.messages, page.messages, scope.world_id) }));
    } catch (error) { if (valid(scope, epoch)) { if (accessDenied(error)) clearTerminal(); else setError("이전 메시지를 불러오지 못했습니다. 다시 시도하세요."); } }
    finally { if (valid(scope, epoch)) { olderRef.current = false; setLoadingOlder(false); } }
  }
  async function markRead() {
    const scope = scopeRef.current, epoch = lifetime.current, current = dataRef.current;
    const latest = current.messages[current.messages.length - 1]?.message_seq;
    if (!scope || !current.ready || !current.context || !latest || readRef.current || BigInt(latest) <= BigInt(current.context.last_read_seq)) return;
    readRef.current = true;
    try {
      const state = await chat.read(scope, latest);
      if (valid(scope, epoch)) setData(current => {
        if (!current.context || BigInt(state.last_read_seq) < BigInt(current.context.last_read_seq)) return current;
        // The read response count includes arrivals already visible at this point.
        // Advance the optimistic count baseline; context refresh follows below.
        const lastLoaded = current.messages[current.messages.length - 1]?.message_seq ?? "0";
        const lastMessage = BigInt(current.context.last_message_seq) > BigInt(lastLoaded) ? current.context.last_message_seq : lastLoaded;
        return { ...current, context: { ...current.context, ...state, last_message_seq: lastMessage } };
      });
    } catch (error) { if (valid(scope, epoch)) { if (accessDenied(error)) clearTerminal(); else setError("읽음 상태를 저장하지 못했습니다."); } }
    finally { if (valid(scope, epoch)) { readRef.current = false; void refreshContext(scope, epoch); } }
  }
  const context = data.context;
  const unseen = context ? data.messages.filter(message => message.author_key !== context.author_key && message.deleted_at === null && BigInt(message.message_seq) > BigInt(context.last_message_seq) && BigInt(message.message_seq) > BigInt(context.last_read_seq)).length : 0;
  const unread = (BigInt(context?.unread_count ?? "0") + BigInt(unseen)).toString();
  return { ...data, draft, setDraft, error, sending, failed: failed !== null, loadingOlder, unread, send, retry, remove, loadOlder, markRead };
}
