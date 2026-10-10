import { act, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { useGroupChat } from "../useGroupChat";
import type { ChatEvent, ChatMessage } from "../../types/chat";
const invoke = vi.hoisted(() => vi.fn());
const listen = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen }));
let handler: (event: { payload: ChatEvent }) => void;
const unlisten = vi.fn();
const scope = { world_id: "world", generation: 1 };
const context = { world_id: "world", author_key: "me", joined_after_seq: "0", last_read_seq: "0", last_message_seq: "1", last_change_seq: "1", unread_count: "1" };
const message = (extra: Partial<ChatMessage> = {}): ChatMessage => ({ id: "one", world_id: "world", message_seq: "1", change_seq: "1", author_key: "other", nickname: "행성 동기화 대기", avatar: "masculine", body: "hello", created_at: "2026-10-10T00:00:00Z", deleted_at: null, ...extra });
beforeEach(() => {
  invoke.mockReset(); listen.mockReset(); unlisten.mockReset();
  listen.mockImplementation(async (_name, cb) => { handler = cb; return unlisten; });
  invoke.mockImplementation(async (command) => {
    if (command === "start_group_chat") return scope;
    if (command === "get_group_chat_context") return context;
    if (command === "list_group_chat_messages") return { messages: [message()], next_cursor: "1", has_more: true };
    if (command === "mark_group_chat_read") return { last_read_seq: "1", unread_count: "0" };
    if (command === "send_group_chat_message") return message({ author_key: "me" });
    return null;
  });
});
it("installs listener before start, loads 50, stops exact scope and unlistens", async () => {
  const { result, unmount } = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(result.current.messages).toHaveLength(1));
  expect(listen.mock.invocationCallOrder[0]).toBeLessThan(invoke.mock.invocationCallOrder[0]);
  expect(invoke).toHaveBeenCalledWith("list_group_chat_messages", { request: { scope, before_seq: null, limit: 50 } });
  unmount(); await waitFor(() => expect(unlisten).toHaveBeenCalledOnce());
  expect(invoke).toHaveBeenCalledWith("stop_group_chat", { request: { scope } });
});
it("ignores stale scope, merges duplicate bigint messages and irreversible tombstones", async () => {
  const { result } = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(result.current.messages).toHaveLength(1));
  act(() => {
    handler({ payload: { kind: "message", scope: { ...scope, generation: 0 }, message: message({ id: "bad" }) } });
    handler({ payload: { kind: "message", scope, message: message({ message_seq: "9007199254740993", change_seq: "9007199254740994", body: null, deleted_at: "2026-10-10T01:00:00Z" }) } });
    handler({ payload: { kind: "message", scope, message: message({ message_seq: "9007199254740993", change_seq: "9007199254740995", body: "restore" }) } });
  });
  expect(result.current.messages).toHaveLength(1);
  expect(result.current.messages[0].body).toBeNull();
});
it("buffers early snapshot before start resolves and cleans late start after unmount", async () => {
  let resolve!: (value: typeof scope) => void;
  invoke.mockImplementation((command) => command === "start_group_chat" ? new Promise(r => { resolve = r; }) : Promise.resolve(null));
  const { unmount } = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(invoke).toHaveBeenCalledWith("start_group_chat", { request: { world_id: "world" } }));
  act(() => handler({ payload: { kind: "snapshot", scope, context, messages: [message()] } }));
  unmount(); await act(async () => resolve(scope));
  expect(invoke).toHaveBeenCalledWith("stop_group_chat", { request: { scope } });
});
it("StrictMode cancels the first listener and retains only the mounted scope", async () => {
  const { result, unmount } = renderHook(() => useGroupChat("world"), { reactStrictMode: true });
  await waitFor(() => expect(result.current.messages).toHaveLength(1));
  expect(unlisten).toHaveBeenCalledTimes(1);
  unmount(); await waitFor(() => expect(unlisten).toHaveBeenCalledTimes(2));
});
it("failed send keeps draft and manual retry reuses only request ID/body", async () => {
  let calls = 0;
  invoke.mockImplementation(async (command, args) => {
    if (command === "start_group_chat") return scope;
    if (command === "get_group_chat_context") return context;
    if (command === "list_group_chat_messages") return { messages: [], next_cursor: null, has_more: false };
    if (command === "send_group_chat_message") { if (calls++ === 0) throw new Error("secret"); return message({ body: args.request.body }); }
    return null;
  });
  const { result } = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(result.current.ready).toBe(true));
  act(() => result.current.setDraft("😀 message"));
  await act(async () => { await result.current.send(); });
  expect(result.current.draft).toBe("😀 message");
  expect(result.current.error).not.toContain("secret");
  await act(async () => { await result.current.retry(); });
  const sent = invoke.mock.calls.filter(c => c[0] === "send_group_chat_message");
  expect(sent[0][1]).toEqual(sent[1][1]);
  expect(Object.keys(sent[0][1].request).sort()).toEqual(["body", "request_id", "scope"]);
  expect(result.current.draft).toBe("");
});
it("loads cursor history, dedupes and never marks read merely by loading", async () => {
  const { result } = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(result.current.ready).toBe(true));
  await act(async () => { await result.current.loadOlder(); });
  expect(invoke).toHaveBeenCalledWith("list_group_chat_messages", { request: { scope, before_seq: "1", limit: 50 } });
  expect(result.current.messages).toHaveLength(1);
  expect(invoke.mock.calls.some(c => c[0] === "mark_group_chat_read")).toBe(false);
});
it("accepts reconnect snapshots without losing history and clears world-bound draft", async () => {
  const { result, rerender } = renderHook(({ world }) => useGroupChat(world), { initialProps: { world: "world" as string | null } });
  await waitFor(() => expect(result.current.ready).toBe(true));
  act(() => { result.current.setDraft("old"); handler({ payload: { kind: "snapshot", scope, context: { ...context, last_change_seq: "2" }, messages: [message({ body: null, deleted_at: "2026-10-10T01:00:00Z", change_seq: "2" })] } }); });
  expect(result.current.messages[0].body).toBeNull();
  rerender({ world: null });
  expect(result.current.messages).toEqual([]); expect(result.current.draft).toBe("");
});

it("serializes a deferred stale start/stop before a same-world replacement", async () => {
  let resolve!: (value: typeof scope) => void;
  let starts = 0;
  const next = { ...scope, generation: 2 };
  invoke.mockImplementation((command) => {
    if (command === "start_group_chat") { starts++; return starts === 1 ? new Promise(r => { resolve = r; }) : Promise.resolve(next); }
    if (command === "get_group_chat_context") return Promise.resolve(context);
    if (command === "list_group_chat_messages") return Promise.resolve({ messages: [], next_cursor: null, has_more: false });
    if (command === "send_group_chat_message") return Promise.resolve(message());
    return Promise.resolve(null);
  });
  const { result, rerender } = renderHook(({ world }) => useGroupChat(world), { initialProps: { world: "world" as string | null }, reactStrictMode: true });
  await waitFor(() => expect(starts).toBe(1));
  rerender({ world: null }); rerender({ world: "world" });
  await act(async () => { await Promise.resolve(); });
  expect(starts).toBe(1);
  await act(async () => resolve(scope));
  await waitFor(() => expect(result.current.ready).toBe(true));
  const stopIndex = invoke.mock.calls.findIndex(c => c[0] === "stop_group_chat");
  const nextIndex = invoke.mock.calls.map(c => c[0]).lastIndexOf("start_group_chat");
  expect(stopIndex).toBeLessThan(nextIndex);
  act(() => result.current.setDraft("survives"));
  await act(async () => { await result.current.send(); });
  expect(invoke).toHaveBeenCalledWith("send_group_chat_message", { request: { scope: next, request_id: expect.any(String), body: "survives" } });
});

it("serializes cleanup across an actual unmount/remount with a deferred start", async () => {
  let resolve!: (value: typeof scope) => void;
  let starts = 0;
  invoke.mockImplementation(command => {
    if (command === "start_group_chat") { starts++; return starts === 1 ? new Promise(r => { resolve = r; }) : Promise.resolve({ ...scope, generation: 2 }); }
    if (command === "get_group_chat_context") return Promise.resolve(context);
    if (command === "list_group_chat_messages") return Promise.resolve({ messages: [], next_cursor: null, has_more: false });
    return Promise.resolve(null);
  });
  const first = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(starts).toBe(1)); first.unmount();
  const next = renderHook(() => useGroupChat("world"));
  await act(async () => { await Promise.resolve(); });
  expect(starts).toBe(1);
  await act(async () => resolve(scope));
  await waitFor(() => expect(next.result.current.ready).toBe(true));
  const commands = invoke.mock.calls.map(c => c[0]);
  expect(commands.indexOf("stop_group_chat")).toBeLessThan(commands.lastIndexOf("start_group_chat"));
});
it("refreshes authoritative unread count after deletion and ignores a stale read cursor snapshot", async () => {
  let latestContext = context;
  invoke.mockImplementation(async command => {
    if (command === "start_group_chat") return scope;
    if (command === "get_group_chat_context") return latestContext;
    if (command === "list_group_chat_messages") return { messages: [message()], next_cursor: null, has_more: false };
    if (command === "mark_group_chat_read") return { last_read_seq: "1", unread_count: "0" };
    return null;
  });
  const { result } = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(result.current.unread).toBe("1"));
  latestContext = { ...context, last_change_seq: "2", unread_count: "0" };
  act(() => handler({ payload: { kind: "message", scope, message: message({ body: null, deleted_at: "2026-10-10T01:00:00Z", change_seq: "2" }) } }));
  await waitFor(() => expect(result.current.unread).toBe("0"));
  await act(async () => { await result.current.markRead(); });
  act(() => handler({ payload: { kind: "snapshot", scope, context: { ...context, last_change_seq: "2" }, messages: [] } }));
  expect(result.current.context?.last_read_seq).toBe("1");
});

it("terminal unavailable clears the entire scope UI and ignores late events", async () => {
  const { result } = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(result.current.ready).toBe(true));
  act(() => result.current.setDraft("private draft"));
  act(() => handler({ payload: { kind: "connection", scope, status: "unavailable" } }));
  expect(result.current.status).toBe("unavailable");
  expect(result.current.ready).toBe(false);
  expect(result.current.context).toBeNull(); expect(result.current.messages).toEqual([]);
  expect(result.current.draft).toBe(""); expect(result.current.unread).toBe("0");
  act(() => handler({ payload: { kind: "snapshot", scope, context, messages: [message()] } }));
  expect(result.current.messages).toEqual([]);
  await act(async () => { await result.current.send(); });
  expect(invoke.mock.calls.some(c => c[0] === "send_group_chat_message")).toBe(false);
});

it("auth-rejected send clears draft and private scope instead of offering retry", async () => {
  invoke.mockImplementation(async command => {
    if (command === "start_group_chat") return scope;
    if (command === "get_group_chat_context") return context;
    if (command === "list_group_chat_messages") return { messages: [message()], next_cursor: null, has_more: false };
    if (command === "send_group_chat_message") throw { rejected: 403 };
    return null;
  });
  const { result } = renderHook(() => useGroupChat("world"));
  await waitFor(() => expect(result.current.ready).toBe(true));
  act(() => result.current.setDraft("private"));
  await act(async () => { await result.current.send(); });
  expect(result.current.draft).toBe(""); expect(result.current.messages).toEqual([]);
  expect(result.current.failed).toBe(false); expect(result.current.status).toBe("unavailable");
});
