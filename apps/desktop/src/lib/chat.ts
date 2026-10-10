import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { ChatChangePage, ChatContext, ChatEvent, ChatMessage, ChatPage, ChatReadState, ChatScope, ChatSeq } from "../types/chat";
export const chat = {
  listen: (receive: (event: ChatEvent) => void) => listen<ChatEvent>("group-chat", event => receive(event.payload)),
  start: (world_id: string) => invoke<ChatScope>("start_group_chat", { request: { world_id } }),
  stop: (scope: ChatScope) => invoke<void>("stop_group_chat", { request: { scope } }),
  context: (scope: ChatScope) => invoke<ChatContext>("get_group_chat_context", { request: { scope } }),
  list: (scope: ChatScope, before_seq: ChatSeq | null = null) => invoke<ChatPage>("list_group_chat_messages", { request: { scope, before_seq, limit: 50 } }),
  sync: (scope: ChatScope, after_change_seq: ChatSeq, until_change_seq: ChatSeq) => invoke<ChatChangePage>("sync_group_chat_changes", { request: { scope, after_change_seq, until_change_seq, limit: 50 } }),
  send: (scope: ChatScope, request_id: string, body: string) => invoke<ChatMessage>("send_group_chat_message", { request: { scope, request_id, body } }),
  remove: (scope: ChatScope, message_id: string) => invoke<ChatMessage>("delete_group_chat_message", { request: { scope, message_id } }),
  read: (scope: ChatScope, message_seq: ChatSeq) => invoke<ChatReadState>("mark_group_chat_read", { request: { scope, message_seq } }),
};
export function sameChatScope(a: ChatScope | null, b: ChatScope) {
  return a?.world_id === b.world_id && a.generation === b.generation;
}
export function mergeChatMessages(current: ChatMessage[], incoming: ChatMessage[], world: string): ChatMessage[] {
  const byId = new Map(current.map(message => [message.id, message]));
  for (const message of incoming) {
    if (message.world_id !== world) continue;
    const previous = byId.get(message.id);
    if (previous && (BigInt(previous.change_seq) >= BigInt(message.change_seq) || (previous.deleted_at !== null && message.deleted_at === null))) continue;
    byId.set(message.id, message);
  }
  return [...byId.values()].sort((a, b) => BigInt(a.message_seq) < BigInt(b.message_seq) ? -1 : BigInt(a.message_seq) > BigInt(b.message_seq) ? 1 : a.id.localeCompare(b.id));
}

// There is one Rust chat runtime in the main window. Serialize lifecycle IPC
// across React remounts so a delayed stale start is stopped before the next start.
let lifecycleQueue: Promise<void> = Promise.resolve();
export function queueChatLifecycle<T>(operation: () => Promise<T>): Promise<T> {
  const next = lifecycleQueue.then(operation);
  lifecycleQueue = next.then(() => {}, () => {});
  return next;
}
