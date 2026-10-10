import { beforeEach, expect, it, vi } from "vitest";
const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));
import { acceptLocalGeneration, invokeLocal, resetLocalLifecycleForTests, bootstrapLocalLifecycle, suspendLocalActions, acceptResetView, localActionsBlocked, type DeviceResetView } from "../localLifecycle";
beforeEach(() => { resetLocalLifecycleForTests(); invoke.mockReset(); });
it("blocks data commands before bootstrap", async () => {
  await expect(invokeLocal("apply_shop_action")).rejects.toThrow();
  expect(invoke).not.toHaveBeenCalled();
});
it("sends generation and discards late success after a reset", async () => {
  invoke.mockResolvedValueOnce({generation:0,data:{state:{generation:0,phase:"idle"},actions_blocked:false,storage_completed:false}});
  await bootstrapLocalLifecycle();
  let finish!: (value: unknown) => void;
  invoke.mockImplementationOnce(() => new Promise(resolve => { finish = resolve; }));
  const pending = invokeLocal("current_usage");
  acceptLocalGeneration(1);
  finish({generation:0,data:{old:true}});
  await expect(pending).rejects.toThrow("오래된");
  expect(invoke).toHaveBeenLastCalledWith("current_usage",{context:{generation:0}});
});
it("discards late errors and preserves current confirmed reset details", async () => {
  invoke.mockResolvedValueOnce({generation:2,data:{state:{generation:2,phase:"completed"},actions_blocked:false,storage_completed:true}});
  await bootstrapLocalLifecycle();
  invoke.mockRejectedValueOnce({generation:1,code:"command_failed",message:"old"});
  await expect(invokeLocal("current_usage")).rejects.toThrow("오래된");
  const details = {kind:"confirmed_reset_view_unavailable",request_id:"request"};
  invoke.mockRejectedValueOnce({generation:2,code:"command_failed",message:"failed",details});
  await expect(invokeLocal("reset_planet")).rejects.toEqual(details);
});

it("blocks old work while focus is rechecking reset state", async () => {
  invoke.mockResolvedValueOnce({generation:0,data:{state:{generation:0,phase:"idle"},actions_blocked:false,storage_completed:false}});
  await bootstrapLocalLifecycle();
  suspendLocalActions();
  await expect(invokeLocal("apply_shop_action")).rejects.toThrow();
  expect(invoke).toHaveBeenCalledTimes(1);
});

function resetView(generation: number, phase: DeviceResetView["state"]["phase"], request = "request"): DeviceResetView {
  return { state: { generation, phase, request_id: request }, actions_blocked: phase === "pending" || phase === "local_committed", storage_completed: phase === "local_committed" || phase === "completed" };
}
for (const latePhase of ["pending", "local_committed"] as const) {
  it(`rejects late same-generation ${latePhase} after completion`, () => {
    expect(acceptResetView(resetView(1, "pending"))).toBe(true);
    expect(acceptResetView(resetView(1, "local_committed"))).toBe(true);
    expect(acceptResetView(resetView(1, "completed"))).toBe(true);
    expect(localActionsBlocked()).toBe(false);
    expect(acceptResetView(resetView(1, latePhase))).toBe(false);
    expect(localActionsBlocked()).toBe(false);
    suspendLocalActions();
    expect(acceptResetView(resetView(1, "completed"))).toBe(true);
    expect(localActionsBlocked()).toBe(false);
  });
}
it("preserves forward phases and permits the next generation reset", () => {
  expect(acceptResetView(resetView(1, "pending"))).toBe(true);
  expect(acceptResetView(resetView(1, "local_committed"))).toBe(true);
  expect(acceptResetView(resetView(1, "pending"))).toBe(false);
  expect(acceptResetView(resetView(1, "completed"))).toBe(true);
  expect(acceptResetView(resetView(1, "completed", "different-request"))).toBe(false);
  expect(acceptResetView(resetView(2, "pending", "next-request"))).toBe(true);
  expect(localActionsBlocked()).toBe(true);
  expect(acceptResetView(resetView(2, "completed", "next-request"))).toBe(true);
  expect(localActionsBlocked()).toBe(false);
});
