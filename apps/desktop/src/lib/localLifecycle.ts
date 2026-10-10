import { invoke } from "@tauri-apps/api/core";
export type LocalEnvelope<T> = { generation: number; data: T };
export type LocalContext = { generation: number };
export type DeviceResetState = { generation: number; phase: "idle" | "pending" | "local_committed" | "completed"; request_id: string | null };
export type DeviceResetView = { state: DeviceResetState; actions_blocked: boolean; storage_completed: boolean };
let generation: number | null = null;
let blocked = true;
let acceptedResetState: DeviceResetState | null = null;
const subscribers = new Set<() => void>();
export function localGeneration() { return generation; }
export function localActionsBlocked() { return blocked || generation === null; }
export function subscribeLocalLifecycle(callback: () => void) { subscribers.add(callback); return () => { subscribers.delete(callback); }; }
function publish() { subscribers.forEach(callback => callback()); }
export function resetLocalLifecycleForTests() { generation = null; blocked = true; acceptedResetState = null; }
export function acceptLocalGeneration(next: number) {
  if (!Number.isSafeInteger(next) || next < 0 || (generation !== null && next < generation)) return false;
  if (next !== generation) { generation = next; blocked = true; acceptedResetState = null; publish(); }
  return true;
}
export function acceptResetView(view: DeviceResetView) {
  if (!acceptLocalGeneration(view.state.generation)) return false;
  const phases = { idle: 0, pending: 1, local_committed: 2, completed: 3 };
  if (acceptedResetState && (
    phases[view.state.phase] < phases[acceptedResetState.phase] ||
    acceptedResetState.request_id != null && view.state.request_id !== acceptedResetState.request_id
  )) return false;
  acceptedResetState = { ...view.state };
  if (blocked !== view.actions_blocked) { blocked = view.actions_blocked; publish(); }
  return true;
}
export async function bootstrapLocalLifecycle(): Promise<DeviceResetView> {
  const result = await invoke<LocalEnvelope<DeviceResetView>>("get_device_reset_state");
  if (!result || !acceptLocalGeneration(result.generation) || !acceptResetView(result.data)) throw new Error("초기화 상태를 확인할 수 없습니다.");
  return result.data;
}
export class StaleLocalResponse extends Error { constructor() { super("오래된 기기 응답을 무시했습니다."); } }
export async function invokeLocal<T>(command: string, args: Record<string, unknown> = {}): Promise<T> {
  const reset = command === "reset_device_data" || command === "retry_device_reset";
  if (generation === null || (blocked && !reset)) throw new Error("기기 상태를 확인하거나 진행 중인 초기화를 복구해 주세요.");
  const sent = generation;
  try {
    const result = await invoke<LocalEnvelope<T>>(command, { ...args, context: { generation: sent } });
    if (!result || !Number.isSafeInteger(result.generation)) throw new Error("기기 응답을 확인할 수 없습니다.");
    if (reset) { if (!acceptLocalGeneration(result.generation)) throw new StaleLocalResponse(); }
    else if (sent !== generation || result.generation !== generation || blocked) throw new StaleLocalResponse();
    return result.data;
  } catch (error) {
    if (error instanceof StaleLocalResponse) throw error;
    const localError = error as { generation?: number; code?: string; details?: unknown; message?: string };
    if (reset && typeof localError?.generation === "number") acceptLocalGeneration(localError.generation);
    if (sent !== generation && !reset || typeof localError?.generation === "number" && localError.generation !== generation) throw new StaleLocalResponse();
    if (localError?.code === "reset_recovery_required") { blocked = true; publish(); }
    if (localError?.details !== undefined && localError.details !== null) throw localError.details;
    throw new Error(localError?.message ?? String(error));
  }
}

export function suspendLocalActions() { if (!blocked) { blocked = true; publish(); } }
