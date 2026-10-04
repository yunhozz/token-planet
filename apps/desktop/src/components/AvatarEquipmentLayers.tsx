import type { ReactNode } from "react";
import type { AvatarEquipment, AvatarSlot, PlanetAvatar } from "../types/usage";

type Facing = "left" | "right";

/** Renders static cosmetic layers around the caller's existing avatar body. */
export function AvatarEquipmentLayers({
  avatar,
  equipment,
  facing = "right",
  children,
}: {
  avatar: PlanetAvatar;
  equipment: AvatarEquipment;
  facing?: Facing;
  children: ReactNode;
}) {
  const transform = facing === "left" ? "translate(16 0) scale(-1 1)" : undefined;
  return (
    <g data-avatar-equipment-layers="true" data-avatar-style={avatar} transform={transform}>
      <g data-avatar-layer="back">{renderEquipment("back", equipment.back.sku)}</g>
      <g data-avatar-layer="base">{children}</g>
      <g data-avatar-layer="outfit">{renderEquipment("outfit", equipment.outfit.sku)}</g>
      <g data-avatar-layer="face">{renderEquipment("face", equipment.face.sku)}</g>
      <g data-avatar-layer="head">{renderEquipment("head", equipment.head.sku)}</g>
    </g>
  );
}

function renderEquipment(slot: AvatarSlot, sku: string | null): ReactNode {
  switch (sku) {
    case "avatar_backpack":
      return slot === "back" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <rect x="11" y="9" width="4" height="8" fill="#8c6655" stroke="#34475a" strokeWidth="1" />
        <rect x="11" y="8" width="4" height="3" fill="#c69b6e" stroke="#34475a" strokeWidth="1" />
        <rect x="12" y="12" width="3" height="2" fill="#f1cf89" />
        <rect x="10" y="10" width="2" height="6" fill="#4c9b9a" />
      </g>;
    case "avatar_cape":
      return slot === "back" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <path d="M 4 10 H 12 V 12 H 14 V 16 H 13 V 18 H 11 V 19 H 6 V 17 H 4 Z" fill="#ec8c78" stroke="#572f4b" strokeWidth="1" />
        <rect x="5" y="14" width="2" height="3" fill="#f1cf89" />
        <rect x="10" y="17" width="2" height="1" fill="#a6d9e2" />
      </g>;
    case "avatar_jetpack":
      return slot === "back" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <rect x="11" y="8" width="4" height="8" fill="#596c82" stroke="#34475a" strokeWidth="1" />
        <rect x="10" y="10" width="2" height="5" fill="#a6d9e2" stroke="#34475a" strokeWidth="1" />
        <rect x="13" y="10" width="2" height="5" fill="#a6d9e2" stroke="#34475a" strokeWidth="1" />
        <path d="M 11 16 V 18 H 13 V 19 H 15 V 17 L 14 16 Z" fill="#f1cf89" />
        <rect x="12" y="9" width="2" height="2" fill="#ec8c78" />
      </g>;
    case "avatar_wings":
      return slot === "back" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <path d="M 4 9 H 7 V 11 H 9 V 14 H 7 V 16 H 5 V 14 H 3 V 12 H 4 Z" fill="#a6d9e2" stroke="#455471" strokeWidth="1" />
        <path d="M 11 11 H 13 V 9 H 15 V 13 H 14 V 16 H 11 V 14 H 9 V 12 H 11 Z" fill="#d2eee0" stroke="#455471" strokeWidth="1" />
        <rect x="4" y="12" width="2" height="2" fill="#f1cf89" />
        <rect x="13" y="12" width="2" height="2" fill="#f1cf89" />
      </g>;
    case "avatar_workwear":
      return slot === "outfit" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <rect x="4" y="12" width="9" height="5" fill="#4c9b9a" stroke="#34475a" strokeWidth="1" />
        <path d="M 6 12 V 14 H 11 V 12 M 8 14 V 17" fill="none" stroke="#d2eee0" strokeWidth="1" />
        <rect x="5" y="16" width="7" height="1" fill="#f1cf89" />
        <rect x="7" y="14" width="1" height="1" fill="#f1cf89" />
      </g>;
    case "avatar_labwear":
      return slot === "outfit" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <path d="M 4 12 H 13 V 17 H 4 Z" fill="#d2eee0" stroke="#455471" strokeWidth="1" />
        <path d="M 7 12 L 8 14 L 6 16 M 10 12 L 9 14 L 11 16" fill="none" stroke="#5f967f" strokeWidth="1" />
        <rect x="9" y="15" width="2" height="1" fill="#f1cf89" />
        <rect x="5" y="16" width="7" height="1" fill="#596c82" />
      </g>;
    case "avatar_spacesuit":
      return slot === "outfit" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <rect x="4" y="12" width="9" height="5" fill="#e8e5d4" stroke="#455471" strokeWidth="1" />
        <rect x="7" y="13" width="4" height="3" fill="#596c82" stroke="#34475a" strokeWidth="1" />
        <rect x="8" y="14" width="2" height="1" fill="#a6d9e2" />
        <rect x="5" y="12" width="2" height="1" fill="#f1cf89" />
        <rect x="5" y="16" width="7" height="1" fill="#ec8c78" />
      </g>;
    case "avatar_nebula_suit":
      return slot === "outfit" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <path d="M 4 12 H 13 V 18 H 11 V 17 H 6 V 18 H 4 Z" fill="#596c82" stroke="#34475a" strokeWidth="1" />
        <path d="M 6 12 L 8 14 L 10 12" fill="none" stroke="#d2eee0" strokeWidth="1" />
        <rect x="7" y="15" width="1" height="1" fill="#f1cf89" />
        <rect x="10" y="14" width="1" height="1" fill="#a6d9e2" />
        <rect x="5" y="16" width="2" height="1" fill="#ec8c78" />
      </g>;
    case "avatar_glasses":
      return slot === "face" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <rect x="4" y="7" width="4" height="3" fill="#a6d9e2" fillOpacity=".7" stroke="#34475a" strokeWidth="1" />
        <rect x="9" y="7" width="4" height="3" fill="#a6d9e2" fillOpacity=".7" stroke="#34475a" strokeWidth="1" />
        <rect x="8" y="8" width="1" height="1" fill="#f1cf89" />
        <path d="M 3 7 H 4 M 13 7 H 14" stroke="#34475a" strokeWidth="1" />
      </g>;
    case "avatar_sunglasses":
      return slot === "face" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <rect x="4" y="7" width="4" height="3" fill="#253c55" stroke="#34475a" strokeWidth="1" />
        <rect x="9" y="7" width="4" height="3" fill="#253c55" stroke="#34475a" strokeWidth="1" />
        <rect x="5" y="8" width="1" height="1" fill="#a6d9e2" />
        <rect x="10" y="8" width="1" height="1" fill="#a6d9e2" />
        <rect x="8" y="8" width="1" height="1" fill="#f1cf89" />
      </g>;
    case "avatar_goggles":
      return slot === "face" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <rect x="3" y="6" width="11" height="1" fill="#ec8c78" />
        <rect x="4" y="7" width="4" height="3" fill="#8ec7c5" stroke="#34475a" strokeWidth="1" />
        <rect x="9" y="7" width="4" height="3" fill="#8ec7c5" stroke="#34475a" strokeWidth="1" />
        <rect x="8" y="8" width="1" height="1" fill="#f1cf89" />
        <rect x="3" y="8" width="1" height="1" fill="#34475a" />
      </g>;
    case "avatar_hud":
      return slot === "face" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <path d="M 4 6 H 13 V 10 H 4 Z" fill="#5db4b2" fillOpacity=".35" stroke="#a6d9e2" strokeWidth="1" />
        <rect x="5" y="7" width="2" height="1" fill="#f1cf89" />
        <rect x="8" y="7" width="3" height="1" fill="#d2eee0" />
        <rect x="10" y="9" width="2" height="1" fill="#ec8c78" />
        <path d="M 3 6 H 4 M 13 6 H 14" stroke="#34475a" strokeWidth="1" />
      </g>;
    case "avatar_explorer_hat":
      return slot === "head" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <rect x="5" y="1" width="7" height="3" fill="#8c6655" stroke="#34475a" strokeWidth="1" />
        <rect x="3" y="4" width="11" height="2" fill="#c69b6e" stroke="#34475a" strokeWidth="1" />
        <rect x="8" y="2" width="2" height="2" fill="#f1cf89" />
        <rect x="5" y="4" width="6" height="1" fill="#4c9b9a" />
      </g>;
    case "avatar_crown":
      return slot === "head" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <path d="M 4 5 L 4 2 L 7 4 L 9 1 L 11 4 L 14 2 L 13 6 H 5 Z" fill="#f1cf89" stroke="#572f4b" strokeWidth="1" />
        <rect x="5" y="5" width="8" height="1" fill="#ec8c78" />
        <rect x="6" y="4" width="1" height="1" fill="#a6d9e2" />
        <rect x="10" y="4" width="1" height="1" fill="#a6d9e2" />
      </g>;
    case "avatar_space_helmet":
      return slot === "head" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <path d="M 3 7 V 4 H 5 V 2 H 11 V 4 H 13 V 7 M 3 7 V 10 H 4 M 12 10 H 13 V 7" fill="none" stroke="#d2eee0" strokeWidth="2" />
        <rect x="4" y="3" width="8" height="2" fill="#596c82" stroke="#34475a" strokeWidth="1" />
        <rect x="2" y="6" width="2" height="3" fill="#a6d9e2" stroke="#34475a" strokeWidth="1" />
        <rect x="13" y="6" width="2" height="3" fill="#a6d9e2" stroke="#34475a" strokeWidth="1" />
        <rect x="5" y="5" width="6" height="1" fill="#f1cf89" />
      </g>;
    case "avatar_halo":
      return slot === "head" && <g data-avatar-equipment={sku} data-avatar-slot={slot}>
        <ellipse cx="8" cy="2" rx="5" ry="1" fill="none" stroke="#f1cf89" strokeWidth="1" />
        <rect x="7" y="0" width="2" height="1" fill="#fff0ad" />
        <rect x="3" y="2" width="1" height="1" fill="#a6d9e2" />
        <rect x="12" y="2" width="1" height="1" fill="#a6d9e2" />
        <path d="M 5 4 H 11" stroke="#f1cf89" strokeWidth="1" />
      </g>;
    default:
      return null;
  }
}
