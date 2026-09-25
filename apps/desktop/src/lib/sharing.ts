import { invoke } from "@tauri-apps/api/core";
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
  email: string | null;
  world: SharedWorld | null;
  sync_status: "local" | "queued" | "syncing" | "synced" | "paused" | "failed";
  pending: number;
  last_synced_at: string | null;
  planet_members?: WorldPlanet[];
};

export type InviteLink = { invite_id: string; code: string; expires_at: string };
export type InviteInfo = { invite_id: string; created_at: string; expires_at: string; revoked_at: string | null; used_at: string | null };
export type WorldMember = { user_id: string; role: "owner" | "member" };

export function planetGrowth(local: { stage: number; progress_to_next: number } | null, _state: SharingState | null) {
  return { stage: local?.stage ?? 0, progress: local?.progress_to_next ?? 0 };
}

export const sharing = {
  state: () => invoke<SharingState>("get_sharing_state"),
  requestCode: (email: string) => invoke<void>("request_email_code", { email }),
  verifyCode: (email: string, code: string) => invoke<SharingState>("verify_email_code", { email, code }),
  createWorld: (name: string) => invoke<SharingState>("create_shared_world", { name }),
  joinWorld: (code: string) => invoke<SharingState>("join_world", { code }),
  createInvite: () => invoke<InviteLink>("create_invite"),
  listInvites: () => invoke<InviteInfo[]>("list_invites"),
  listMembers: () => invoke<WorldMember[]>("list_world_members"),
  revokeInvite: (inviteId: string) => invoke<boolean>("revoke_invite", { inviteId }),
  pause: (paused: boolean) => invoke<SharingState>("pause_sharing", { paused }),
  transferOwner: (newOwnerId: string) => invoke<SharingState>("transfer_world_owner", { newOwnerId }),
  leave: () => invoke<SharingState>("leave_world"),
  deleteUsage: () => invoke<SharingState>("delete_synced_usage"),
  signOut: () => invoke<SharingState>("sign_out"),
};
