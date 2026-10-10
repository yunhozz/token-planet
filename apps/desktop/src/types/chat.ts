import type { PlanetAvatar } from "./usage";
// PostgreSQL bigint values cross IPC as decimal strings, never JS numbers.
export type ChatSeq = string;
export type ChatScope = { world_id: string; generation: number };
export type ChatMessage = {
  id: string; world_id: string; message_seq: ChatSeq; change_seq: ChatSeq;
  author_key: string; nickname: string; avatar: PlanetAvatar; body: string | null;
  created_at: string; deleted_at: string | null;
};
export type ChatContext = {
  world_id: string; author_key: string; joined_after_seq: ChatSeq; last_read_seq: ChatSeq;
  last_message_seq: ChatSeq; last_change_seq: ChatSeq; unread_count: ChatSeq;
};
export type ChatPage = { messages: ChatMessage[]; next_cursor: ChatSeq | null; has_more: boolean };
export type ChatReadState = { last_read_seq: ChatSeq; unread_count: ChatSeq };
export type ChatChangePage = { messages: ChatMessage[]; next_cursor: ChatSeq; has_more: boolean };
export type ChatStatus = "connecting" | "connected" | "reconnecting" | "unavailable" | "stopped";
export type ChatEvent =
  | { kind: "connection"; scope: ChatScope; status: ChatStatus }
  | { kind: "snapshot"; scope: ChatScope; context: ChatContext; messages: ChatMessage[] }
  | { kind: "message"; scope: ChatScope; message: ChatMessage };
