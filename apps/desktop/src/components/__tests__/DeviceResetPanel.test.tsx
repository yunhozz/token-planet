import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { beforeEach, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({invoke}));
import { DeviceResetPanel } from "../DeviceResetPanel";
import { bootstrapLocalLifecycle, resetLocalLifecycleForTests } from "../../lib/localLifecycle";
const view = (phase = "idle", generation = 0) => ({state:{phase,generation,request_id:phase === "idle" ? null : "same-request"},actions_blocked:phase === "pending",storage_completed:phase === "completed"});
beforeEach(async () => { resetLocalLifecycleForTests(); invoke.mockReset(); invoke.mockResolvedValue({generation:0,data:view()}); await bootstrapLocalLifecycle(); });
it("canceling confirmation does not delete data", async () => {
 render(<DeviceResetPanel onChanged={vi.fn()} />);
 fireEvent.click(screen.getByRole("button",{name:"기기 데이터 전체 초기화"}));
 expect(screen.getByText(/현재 익명 계정/)).toBeInTheDocument();
 fireEvent.click(screen.getByRole("button",{name:"취소"}));
 expect(invoke.mock.calls.some(([command]) => command === "reset_device_data")).toBe(false);
});
it("a partial failure retries the same persisted request", async () => {
 let completed = false;
 invoke.mockImplementation(async (command: string) => {
   if (command === "reset_device_data") throw {generation:1,code:"reset_recovery_required",message:"부분 실패"};
   if (command === "retry_device_reset") { completed = true; return {generation:1,data:{...view("completed",1),storage_completed:true}}; }
   return {generation:1,data:view(completed ? "completed" : "pending",1)};
 });
 render(<DeviceResetPanel onChanged={vi.fn()} />);
 fireEvent.click(screen.getByRole("button",{name:"기기 데이터 전체 초기화"}));
 fireEvent.click(screen.getByRole("button",{name:"전체 초기화 및 로그아웃"}));
 fireEvent.click(await screen.findByRole("button",{name:"진행 중인 초기화 복구"}));
 await waitFor(() => expect(invoke).toHaveBeenCalledWith("retry_device_reset",{requestId:"same-request",context:{generation:1}}));
 expect(await screen.findByText(/데이터 초기화와 로그아웃이 완료/)).toBeInTheDocument();
});

it("refreshes a completed view without deleting data again", async () => {
  invoke.mockImplementation(async (command: string) => command === "reset_device_data" ? {generation:1,data:{state:view("completed",1).state,storage_completed:true}} : {generation:1,data:view("completed",1)});
  const onChanged = vi.fn().mockRejectedValueOnce(new Error("view failed")).mockResolvedValueOnce(undefined);
  render(<DeviceResetPanel onChanged={onChanged} />);
  fireEvent.click(screen.getByRole("button",{name:"기기 데이터 전체 초기화"}));
  fireEvent.click(screen.getByRole("button",{name:"전체 초기화 및 로그아웃"}));
  fireEvent.click(await screen.findByRole("button",{name:"완료된 화면 다시 확인"}));
  await waitFor(() => expect(screen.queryByRole("button",{name:"완료된 화면 다시 확인"})).not.toBeInTheDocument());
  expect(invoke.mock.calls.filter(([command]) => command === "reset_device_data")).toHaveLength(1);
  expect(invoke.mock.calls.some(([command]) => command === "retry_device_reset")).toBe(false);
});
it("traps keyboard focus and returns focus on cancel", () => {
 render(<DeviceResetPanel onChanged={vi.fn()} />);
 const trigger = screen.getByRole("button",{name:"기기 데이터 전체 초기화"});
 fireEvent.click(trigger);
 const confirm = screen.getByRole("button",{name:"전체 초기화 및 로그아웃"});
 const cancel = screen.getByRole("button",{name:"취소"});
 expect(confirm).toHaveFocus();
 fireEvent.keyDown(confirm,{key:"Tab",shiftKey:true}); expect(cancel).toHaveFocus();
 fireEvent.keyDown(cancel,{key:"Tab"}); expect(confirm).toHaveFocus();
 fireEvent.keyDown(confirm,{key:"Escape"}); expect(trigger).toHaveFocus();
 expect(screen.queryByRole("dialog")).not.toBeInTheDocument();
});
