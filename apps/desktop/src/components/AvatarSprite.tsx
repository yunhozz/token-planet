import type { PlanetAvatar } from "../types/usage";

export function AvatarSprite({ avatar, className = "", label, facing = "right", eyesClosed = false, walking = false }: { avatar: PlanetAvatar; className?: string; label?: string; facing?: "left" | "right"; eyesClosed?: boolean; walking?: boolean }) {
  const feminine = avatar === "feminine";
  const spriteClass = ["avatar-sprite", className, facing === "left" ? "avatar-sprite--facing-left" : "", walking ? "avatar-sprite--walking" : ""].filter(Boolean).join(" ");
  return (
    <svg className={spriteClass} viewBox="0 0 16 20" role={label ? "img" : undefined} aria-label={label} aria-hidden={label ? undefined : true} shapeRendering="crispEdges">
      <g transform={facing === "left" ? "translate(16 0) scale(-1 1)" : undefined}>
        <rect x="5" y="1" width="7" height="2" fill={feminine ? "#51395f" : "#253c55"} />
        <rect x="3" y="3" width="11" height="5" fill={feminine ? "#51395f" : "#253c55"} />
        <rect x="4" y="5" width="9" height="6" fill="#f3c995" />
        {eyesClosed ? <><rect x="5" y="8" width="2" height="1" fill="#292b3b" /><rect x="10" y="8" width="2" height="1" fill="#292b3b" /></> : <><rect x="5" y="7" width="1" height="1" fill="#292b3b" /><rect x="10" y="7" width="1" height="1" fill="#292b3b" /></>}
        <rect x="7" y="9" width="2" height="1" fill="#b86458" />
        {feminine && <><rect x="2" y="4" width="2" height="8" fill="#51395f" /><rect x="13" y="4" width="2" height="8" fill="#51395f" /></>}
        <rect x="4" y="12" width="9" height="5" fill={feminine ? "#d57875" : "#4c9b9a"} />
        <rect x="2" y="13" width="2" height="4" fill={feminine ? "#d57875" : "#4c9b9a"} />
        <rect x="13" y="13" width="2" height="4" fill={feminine ? "#d57875" : "#4c9b9a"} />
        <rect className="avatar-leg avatar-leg--left" x="5" y="17" width="3" height="2" fill="#34374a" />
        <rect className="avatar-leg avatar-leg--right" x="10" y="17" width="3" height="2" fill="#34374a" />
      </g>
    </svg>
  );
}
