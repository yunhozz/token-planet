import { useEffect, useId, useLayoutEffect, useRef, useState } from "react";
import { useGroupChat } from "../hooks/useGroupChat";
import { AvatarSprite } from "./AvatarSprite";
const statuses = { connecting: "연결 중", connected: "연결됨", reconnecting: "다시 연결 중", unavailable: "연결할 수 없음", stopped: "연결 종료" };
export function GroupChat({ worldId }: { worldId: string }) {
  const chat = useGroupChat(worldId);
  const [open, setOpen] = useState(false);
  const [atLatest, setAtLatest] = useState(true);
  const [composing, setComposing] = useState(false);
  const region = useRef<HTMLDivElement>(null);
  const anchoring = useRef(false);
  const id = useId();
  const historyLatest = useRef<string | null>(null);
  const count = Array.from(chat.draft).length;
  const latestChange = chat.messages[chat.messages.length - 1]?.change_seq;
  useLayoutEffect(() => {
    const node = region.current;
    if (open && atLatest && node && !anchoring.current) node.scrollTop = node.scrollHeight;
  }, [open, atLatest, chat.messages]);
  useEffect(() => {
    if (open && atLatest && !anchoring.current) void chat.markRead();
  }, [open, atLatest, latestChange, chat.ready, chat.context?.last_read_seq]);
  async function older() {
    const node = region.current;
    if (!node) return;
    const height = node.scrollHeight, top = node.scrollTop;
    const viewportTop = node.getBoundingClientRect().top;
    const anchor = Array.from(node.querySelectorAll<HTMLLIElement>(".chat-message")).find(row => row.getBoundingClientRect().bottom > viewportTop);
    const anchorTop = anchor?.getBoundingClientRect().top;
    historyLatest.current = chat.messages[chat.messages.length - 1]?.message_seq ?? null;
    anchoring.current = true; setAtLatest(false);
    try { await chat.loadOlder(); }
    finally {
      // The hook's state commit has rendered before the next paint.
      requestAnimationFrame(() => {
        if (region.current === node) {
          const addedAbove = anchor && anchorTop !== undefined ? anchor.getBoundingClientRect().top - anchorTop : node.scrollHeight - height;
          node.scrollTop = top + addedAbove;
        }
        anchoring.current = false;
      });
    }
  }
  return <section className="group-chat" aria-label="그룹 채팅">
    <div className="group-chat-entry">
      <button type="button" aria-expanded={open} aria-controls={`${id}-panel`} onClick={() => { setOpen(value => !value); setAtLatest(true); }}>그룹 채팅 {open ? "접기" : "열기"}</button>
      <span className="chat-unread" role="status" aria-atomic="true">{chat.unread !== "0" ? `그룹 채팅에 읽지 않은 메시지 ${BigInt(chat.unread).toLocaleString()}개` : "그룹 채팅에 읽지 않은 메시지 없음"}</span>
    </div>
    {open && <div id={`${id}-panel`} className="group-chat-panel">
      <p className="chat-connection">{statuses[chat.status]}</p>
      <div ref={region} className="chat-history" role="region" aria-label="그룹 대화 기록" tabIndex={0} onScroll={event => {
        if (anchoring.current) return;
        const node = event.currentTarget;
        const latest = node.scrollHeight - node.scrollTop - node.clientHeight <= 24;
        if (atLatest && !latest) historyLatest.current = chat.messages[chat.messages.length - 1]?.message_seq ?? null;
        setAtLatest(latest);
      }}>
        {chat.hasMore && <button className="chat-older" type="button" disabled={chat.loadingOlder} onClick={() => void older()}>{chat.loadingOlder ? "이전 메시지 불러오는 중" : "이전 메시지 더 보기"}</button>}
        {!chat.ready && chat.status !== "unavailable" && chat.status !== "stopped" && <p>대화를 불러오는 중입니다.</p>}
        {chat.ready && chat.messages.length === 0 && <p className="empty-note">그룹원에게 첫 메시지를 보내 보세요.</p>}
        <ol className="chat-messages">{chat.messages.map(message => <li className="chat-message" key={message.id}>
          <AvatarSprite avatar={message.avatar} label={`${message.nickname}의 프로필 아바타`} className="chat-avatar" />
          <div className="chat-message-content">
            <div className="chat-author"><strong>{message.nickname}</strong><time dateTime={message.created_at} title={new Date(message.created_at).toLocaleString()}>{new Date(message.created_at).toLocaleTimeString([], { hour: "2-digit", minute: "2-digit" })}</time></div>
            <p className={message.deleted_at ? "chat-tombstone" : "chat-body"}>{message.deleted_at ? "삭제된 메시지" : message.body}</p>
            {!message.deleted_at && message.author_key === chat.context?.author_key && <button className="chat-delete" type="button" aria-label="메시지 삭제" onClick={() => { if (window.confirm("이 메시지를 삭제할까요?")) void chat.remove(message.id); }}>삭제</button>}
          </div>
        </li>)}</ol>
      </div>
      {!atLatest && (chat.unread !== "0" || chat.messages[chat.messages.length - 1]?.message_seq !== historyLatest.current) && <button type="button" className="chat-new" onClick={() => setAtLatest(true)}>새 메시지 보기</button>}
      {chat.error && <p className="chat-error" role="alert">{chat.error}</p>}
      <form className="chat-composer" onSubmit={event => { event.preventDefault(); if (!composing && count <= 2000 && chat.draft.trim()) void chat.send(); }}>
        <label htmlFor={`${id}-input`}>메시지 입력</label>
        <textarea id={`${id}-input`} value={chat.draft} rows={3} disabled={!chat.ready || chat.sending || chat.failed} aria-describedby={`${id}-count`} aria-invalid={count > 2000} onChange={event => chat.setDraft(event.target.value)} onCompositionStart={() => setComposing(true)} onCompositionEnd={() => setComposing(false)} />
        <span id={`${id}-count`} className={count > 2000 ? "chat-error" : "chat-count"}>{count.toLocaleString()} / 2,000자</span>
        {chat.failed ? <button type="button" disabled={chat.sending || composing} onClick={() => void chat.retry()}>같은 메시지 다시 전송</button> : <button type="submit" disabled={!chat.ready || chat.sending || composing || !chat.draft.trim() || count > 2000}>{chat.sending ? "전송 중" : "전송"}</button>}
      </form>
    </div>}
  </section>;
}
