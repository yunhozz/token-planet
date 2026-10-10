import { invokeLocal as invoke } from "./localLifecycle";
import type { WorldPlanet } from "../types/usage";

export type SharedWorld = {
  id: string;
  name: string;
  timezone: string;
  is_owner: boolean;
  member_count: number;
};

export type SharingState = {
  phase: "unavailable" | "signed_out" | "signed_in" | "shared";
  user_id: string | null;
  world: SharedWorld | null;
  sync_status: "local" | "queued" | "syncing" | "synced" | "paused" | "failed";
  pending: number;
  last_synced_at: string | null;
  planet_members?: WorldPlanet[];
};

export type CreatedWorldInvite = { invite_id: string; code: string; created_at: string; expires_at: string };
export type WorldInvite = { invite_id: string; created_at: string; expires_at: string; revoked_at: string | null; used_at: string | null; status: "active" | "used" | "revoked" | "expired" };
export type InviteRevokeResult = { status: "revoked" | "already_revoked" | "used" };

export type WorldMember = { user_id: string; role: "owner" | "member" };

export function planetGrowth(local: { stage: number; progress_to_next: number } | null, _state: SharingState | null) {
  return { stage: local?.stage ?? 0, progress: local?.progress_to_next ?? 0 };
}

export const sharing = {
  state: () => invoke<SharingState>("get_sharing_state"),
  startAnonymousSession: () => invoke<SharingState>("start_anonymous_session"),
  createWorld: (name: string, nickname: string) => invoke<SharingState>("create_shared_world", { name, nickname }),
  joinWorld: (code: string, nickname: string) => invoke<SharingState>("join_world", { code, nickname }),
  createInvite: () => invoke<CreatedWorldInvite>("create_world_invite"),
  listInvites: () => invoke<WorldInvite[]>("list_world_invites"),
  revokeInvite: (inviteId: string) => invoke<InviteRevokeResult>("revoke_world_invite", { inviteId }),
  listMembers: () => invoke<WorldMember[]>("list_world_members"),
  pause: (paused: boolean) => invoke<SharingState>("pause_sharing", { paused }),
  transferOwner: (newOwnerId: string) => invoke<SharingState>("transfer_world_owner", { newOwnerId }),
  leave: () => invoke<SharingState>("leave_world"),
  deleteUsage: () => invoke<SharingState>("delete_synced_usage"),
};
