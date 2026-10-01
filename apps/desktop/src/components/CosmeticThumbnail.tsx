import { styleIdForSku } from "./cosmeticStyles";

const SURFACE_COLORS: Record<string, string> = {
  flag: "#ec8c78",
  crystal_tower: "#a6d9e2",
  flower_garden: "#e9a1bb",
  observatory: "#8ec7c5",
};

export function CosmeticThumbnail({ sku, slotId, className = "" }: { sku: string; slotId: string; className?: string }) {
  const styleId = styleIdForSku(sku);
  return (
    <svg className={`cosmetic-art ${className}`} viewBox="0 0 100 72" role="img" aria-label="장식 미리보기" shapeRendering="crispEdges">
      <rect width="100" height="72" fill="#1d2d43" />
      <circle cx="76" cy="15" r="7" fill="#f1cf89" opacity=".8" />
      <path d="M0 48h17v-8h16v5h13v-12h15v8h15v-6h15v13h9v24H0Z" fill="#3a5d69" />
      <path d="M0 57h100v15H0Z" fill="#527e70" />
      {slotId === "sky" && <g fill="#fff0ad">
        {styleId === "star_cluster" && <><rect x="18" y="18" width="6" height="6"/><rect x="31" y="11" width="4" height="4"/><rect x="43" y="22" width="5" height="5"/><rect x="55" y="14" width="3" height="3"/><rect x="67" y="27" width="5" height="5"/></>}
        {styleId === "aurora" && <g fill="none" stroke="#9fe2ce"><path d="M9 31h17v-8h18v6h15v-9h23" strokeWidth="5"/><path d="M17 38h22v-5h19v4h17" stroke="#72c7c7" strokeWidth="3"/></g>}
        {styleId === "meteor_shower" && <><path d="m18 17 17-12h4L22 20Z"/><path d="m57 31 22-16h4L62 34Z" fill="#f3d38e"/><path d="m22 15 8-6m35 20 11-8" stroke="#fff0ad" strokeWidth="2"/></>}
      </g>}
      {slotId === "ring" && <g fill="none" stroke="#a6d9a2" strokeWidth="3">
        <ellipse cx="50" cy="40" rx="38" ry="13" transform="rotate(-17 50 40)" />
        {(styleId === "double_ring" || styleId === "moonlets") && <ellipse cx="50" cy="40" rx="31" ry="8" transform="rotate(-17 50 40)" stroke="#f1cf89" />}
        {styleId === "moonlets" && <g fill="#d7d5bd" stroke="#7e9a9a"><rect x="19" y="30" width="7" height="7"/><rect x="69" y="42" width="8" height="8"/></g>}
        <circle cx="50" cy="38" r="10" fill="#72c7c7" stroke="#f6eed3" />
      </g>}
      {slotId === "surface" && <g transform="translate(35 25)">
        {styleId === "flag" && <><rect x="13" y="4" width="3" height="32" fill="#8c6655"/><path d="M16 5h29v13H27l-11 7Z" fill="#ec8c78" stroke="#572f4b" strokeWidth="2"/><rect x="7" y="35" width="24" height="4" fill="#926e58"/></>}
        {styleId === "crystal_tower" && <><rect x="6" y="28" width="44" height="12" fill="#536c91" stroke="#c2c6cf" strokeWidth="2"/><path d="M12 28V13L28 1l16 12v15Z" fill="#a6d9e2" stroke="#e3f0de" strokeWidth="2"/><path d="M28 5v20M18 17h20" stroke="#6ba9bb" strokeWidth="3"/></>}
        {styleId === "flower_garden" && <><path d="M1 34h60v6H1Z" fill="#8c7653"/><path d="M8 33V16m19 17V10m20 23V15" stroke="#4d875a" strokeWidth="3"/><rect x="4" y="11" width="8" height="8" fill="#efad7a"/><rect x="23" y="5" width="9" height="9" fill="#f1d27f"/><rect x="45" y="10" width="8" height="8" fill="#d992b1"/></>}
        {styleId === "observatory" && <><rect x="3" y="18" width="58" height="24" fill="#596c82" stroke="#d6c58d" strokeWidth="2"/><path d="M0 20a32 22 0 0 1 64 0Z" fill="#8ec7c5" stroke="#f0d28b" strokeWidth="2"/><path d="M28 5h8v15h-8zm-5 7h18v4H23Z" fill="#d7e6cd"/></>}
      </g>}
      {slotId === "forecourt" && <g transform="translate(28 31)">
        {styleId === "pond" && <><path d="M1 9h64v25H1Z" fill="#8d7757" stroke="#d9c288" strokeWidth="2"/><path d="M7 14h52v13H7Z" fill="#4c9f9e"/><path d="M13 16h14v3H13zm27 7h14v3H40Z" fill="#b6e3ca"/></>}
        {styleId === "lantern" && <><rect x="30" y="12" width="4" height="30" fill="#8f6b4e"/><rect x="20" y="1" width="24" height="22" fill="#e8c67d" stroke="#754e52" strokeWidth="2"/><rect x="25" y="5" width="14" height="13" fill="#fff0ad"/><path d="M17 1h30M22 -3h20" stroke="#754e52" strokeWidth="2"/></>}
        {styleId === "rover" && <><rect x="3" y="13" width="58" height="21" fill="#ce9b68" stroke="#564d58" strokeWidth="2"/><rect x="17" y="2" width="26" height="14" fill="#8ac8c4" stroke="#564d58" strokeWidth="2"/><circle cx="17" cy="35" r="7" fill="#34475a" stroke="#e2d59d" strokeWidth="2"/><circle cx="48" cy="35" r="7" fill="#34475a" stroke="#e2d59d" strokeWidth="2"/></>}
        {styleId === "greenhouse" && <><path d="M1 37V14L33 1l32 13v23Z" fill="#84c8bd" fillOpacity=".82" stroke="#d8e6bd" strokeWidth="2"/><path d="M5 16h56M13 12v25m20-30v30m21-24v24" stroke="#607e79" strokeWidth="2"/><rect x="27" y="24" width="13" height="13" fill="#8eb76c"/></>}
      </g>}
      {SURFACE_COLORS[styleId] && !["flag", "crystal_tower", "flower_garden", "observatory"].includes(styleId) && <rect x="46" y="28" width="8" height="8" fill={SURFACE_COLORS[styleId]} />}
    </svg>
  );
}
