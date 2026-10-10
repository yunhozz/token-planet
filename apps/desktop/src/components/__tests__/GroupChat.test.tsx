import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
import { GroupChat } from "../GroupChat";
const mock = vi.hoisted(() => ({ current: {} as Record<string, unknown> }));
vi.mock("../../hooks/useGroupChat", () => ({ useGroupChat: () => mock.current }));
beforeEach(() => { mock.current = { ready: true, messages: [{ id: "one", world_id: "world", message_seq: "1", change_seq: "1", author_key: "other", nickname: "예전 이름", avatar: "feminine", body: "<script>text</script>", created_at: "2026-10-10T00:00:00Z", deleted_at: null }], context: { author_key: "me" }, unread: "3", status: "connected", draft: "", setDraft: vi.fn(), send: vi.fn(), retry: vi.fn(), remove: vi.fn(), markRead: vi.fn(), loadOlder: vi.fn(async () => {}), hasMore: true, loadingOlder: false, sending: false, failed: false, error: null }; });
it("keeps a contextual collapsed unread status and shows server avatar/plain text", () => {
  render(<GroupChat worldId="world" />);
  expect(screen.getByRole("status")).toHaveTextContent("그룹 채팅에 읽지 않은 메시지 3개");
  expect(screen.queryByLabelText("메시지 입력")).not.toBeInTheDocument();
  fireEvent.click(screen.getByRole("button", { name: /그룹 채팅/ }));
  expect(screen.getByRole("img", { name: "예전 이름의 프로필 아바타" })).toBeInTheDocument();
  expect(screen.getByText("<script>text</script>")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "메시지 삭제" })).not.toBeInTheDocument();
});
it("tombstones preserve author and own-message deletion control disappears", () => {
  mock.current.messages = [{ ...(mock.current.messages as object[])[0], author_key: "me", body: null, deleted_at: "2026-10-10T01:00:00Z" }];
  render(<GroupChat worldId="world" />); fireEvent.click(screen.getByRole("button", { name: /그룹 채팅/ }));
  expect(screen.getByText("삭제된 메시지")).toBeInTheDocument();
  expect(screen.getByRole("img")).toBeInTheDocument();
  expect(screen.queryByRole("button", { name: "메시지 삭제" })).not.toBeInTheDocument();
});
it("counts Unicode points and prevents blank/oversized/composing submissions", () => {
  mock.current.draft = "😀".repeat(2000);
  const { rerender } = render(<GroupChat worldId="world" />); fireEvent.click(screen.getByRole("button", { name: /그룹 채팅/ }));
  expect(screen.getByText("2,000 / 2,000자")).toBeInTheDocument();
  expect(screen.getByRole("button", { name: "전송" })).toBeEnabled();
  fireEvent.compositionStart(screen.getByLabelText("메시지 입력"));
  expect(screen.getByRole("button", { name: "전송" })).toBeDisabled();
  fireEvent.compositionEnd(screen.getByLabelText("메시지 입력"));
  mock.current.draft = "😀".repeat(2001); rerender(<GroupChat worldId="world" />);
  expect(screen.getByRole("button", { name: "전송" })).toBeDisabled();
  mock.current.draft = "  \n"; rerender(<GroupChat worldId="world" />);
  expect(screen.getByRole("button", { name: "전송" })).toBeDisabled();
});
it("marks read only open at bottom, preserves history scroll, offers new-message cue", async () => {
  const { rerender } = render(<GroupChat worldId="world" />);
  expect(mock.current.markRead).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: /그룹 채팅/ }));
  await waitFor(() => expect(mock.current.markRead).toHaveBeenCalled());
  vi.mocked(mock.current.markRead as () => void).mockClear();
  const log = screen.getByRole("region", { name: "그룹 대화 기록" });
  Object.defineProperties(log, { scrollHeight: { configurable: true, value: 800 }, clientHeight: { configurable: true, value: 200 } });
  log.scrollTop = 100; fireEvent.scroll(log);
  mock.current.messages = [...mock.current.messages as object[], { ...(mock.current.messages as object[])[0], id: "two", message_seq: "2", change_seq: "2", body: "new" }];
  rerender(<GroupChat worldId="world" />);
  expect(log.scrollTop).toBe(100);
  expect(mock.current.markRead).not.toHaveBeenCalled();
  fireEvent.click(screen.getByRole("button", { name: "새 메시지 보기" }));
  await waitFor(() => expect(mock.current.markRead).toHaveBeenCalled());
});
it("history loading anchors the existing reader position and failure exposes retry", async () => {
  const { rerender } = render(<GroupChat worldId="world" />); fireEvent.click(screen.getByRole("button", { name: /그룹 채팅/ }));
  const log = screen.getByRole("region", { name: "그룹 대화 기록" });
  Object.defineProperty(log, "scrollHeight", { configurable: true, value: 500 }); log.scrollTop = 80;
  mock.current.loadOlder = vi.fn(async () => { Object.defineProperty(log, "scrollHeight", { configurable: true, value: 700 }); });
  rerender(<GroupChat worldId="world" />);
  fireEvent.click(screen.getByRole("button", { name: "이전 메시지 더 보기" }));
  await waitFor(() => expect(log.scrollTop).toBe(280));
  mock.current.failed = true; mock.current.error = "메시지를 보내지 못했습니다."; rerender(<GroupChat worldId="world" />);
  fireEvent.click(screen.getByRole("button", { name: "같은 메시지 다시 전송" }));
  expect(mock.current.retry).toHaveBeenCalledOnce();
});

it("offers a new-message cue in history even when an own-author message has no unread count", () => {
  mock.current.unread = "0";
  const { rerender } = render(<GroupChat worldId="world" />); fireEvent.click(screen.getByRole("button", { name: /그룹 채팅/ }));
  const log = screen.getByRole("region", { name: "그룹 대화 기록" });
  Object.defineProperties(log, { scrollHeight: { configurable: true, value: 800 }, clientHeight: { configurable: true, value: 200 } }); log.scrollTop = 100; fireEvent.scroll(log);
  mock.current.messages = [...mock.current.messages as object[], { ...(mock.current.messages as object[])[0], id: "two", message_seq: "2", change_seq: "2", author_key: "me" }];
  rerender(<GroupChat worldId="world" />);
  expect(screen.getByRole("button", { name: "새 메시지 보기" })).toBeInTheDocument();
});

it("anchors an existing visible row so concurrent appended messages do not shift history", async () => {
  const { rerender } = render(<GroupChat worldId="world" />); fireEvent.click(screen.getByRole("button", { name: /그룹 채팅/ }));
  const log = screen.getByRole("region", { name: "그룹 대화 기록" });
  const row = screen.getByText("<script>text</script>").closest("li")!;
  let rowTop = 40;
  vi.spyOn(row, "getBoundingClientRect").mockImplementation(() => ({ top: rowTop, bottom: rowTop + 40 } as DOMRect));
  Object.defineProperty(log, "scrollHeight", { configurable: true, value: 500 }); log.scrollTop = 80;
  mock.current.loadOlder = vi.fn(async () => { rowTop = 70; Object.defineProperty(log, "scrollHeight", { configurable: true, value: 700 }); });
  rerender(<GroupChat worldId="world" />);
  fireEvent.click(screen.getByRole("button", { name: "이전 메시지 더 보기" }));
  await waitFor(() => expect(log.scrollTop).toBe(110));
});
