import type { ReactNode } from "react";
import type { LandscapeInstance, ShopProduct, PlacementZone } from "../types/usage";

/**
 * The component returns an SVG <g>, never an SVG root. Its local origin is the
 * top-left of the canonical placement footprint; thumbnails and the landscape
 * renderer can reuse the same geometry by placing it in an SVG with these bounds.
 * All art stays inset enough that the largest saved variation fits the footprint.
 */
export const LANDSCAPE_OBJECT_INTRINSIC_BOUNDS = {
  ground: { x: 0, y: 0, width: 64, height: 64 },
  sky: { x: 0, y: 0, width: 96, height: 64 },
} as const;

type Palette = {
  main: string;
  light: string;
  accent: string;
  dark: string;
  outline: string;
};

// Muted terrain/sky hues, dark pixel outlines and warm highlights are sampled
// from AvatarSprite and PlanetLandscapeDecorations rather than adding a new style.
const VARIANT_PALETTES: readonly Palette[] = [
  { main: "#4c9f9e", light: "#d2eee0", accent: "#f1cf89", dark: "#34475a", outline: "#455471" },
  { main: "#5f967f", light: "#b6e3ca", accent: "#e8d59d", dark: "#3e5968", outline: "#4c5d5e" },
  { main: "#7c9b65", light: "#d8e6bd", accent: "#efad7a", dark: "#3d5960", outline: "#566b50" },
  { main: "#63837a", light: "#b7d4ce", accent: "#f0d28b", dark: "#34475a", outline: "#455471" },
  { main: "#8a9b62", light: "#c9ddb0", accent: "#ec8c78", dark: "#572f4b", outline: "#5b5360" },
];

const VARIANT_SCALES: readonly [number, number][] = [
  [0.96, 1.02],
  [0.98, 0.98],
  [1, 1],
  [1.02, 0.98],
  [1.04, 1.02],
];

type ArtRenderer = (palette: Palette, variationIndex: number) => ReactNode;

const LANDSCAPE_ART: Record<string, { zone: PlacementZone; render: ArtRenderer }> = {
  land_pond: {
    zone: "ground",
    render: (p, v) => <>
      <path d="M 7 40 H 57 V 53 H 53 V 57 H 13 V 54 H 7 Z" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d="M 11 42 H 53 V 51 H 49 V 54 H 16 V 51 H 11 Z" fill={p.main} />
      <path d={`M ${16 + v} 45 H 25 V 47 H ${16 + v} Z M 37 49 H ${46 - v} V 51 H 37 Z`} fill={p.light} />
      <path d="M 11 38 V 33 H 13 V 38 M 51 39 V 34 H 53 V 39" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <rect x="8" y="55" width="49" height="3" fill={p.accent} />
    </>,
  },
  land_well: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="15" y="34" width="34" height="23" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 12 34 L 32 16 L 52 34 Z" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <rect x="19" y="38" width="26" height="16" fill={p.light} />
      <rect x="27" y="38" width="10" height="18" fill={p.dark} />
      <rect x="20" y="56" width="25" height="3" fill={p.accent} />
      <path d={`M 32 24 V 42 M 30 42 H ${34 + v} V 48 H 30 Z`} fill={p.accent} stroke={p.outline} strokeWidth="2" />
    </>,
  },
  land_greenhouse: {
    zone: "ground",
    render: (p, v) => <>
      <path d="M 7 32 L 32 12 L 57 32 V 56 H 7 Z" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <path d="M 32 14 V 55 M 10 32 H 54 M 16 27 V 55 M 48 27 V 55" stroke={p.dark} strokeWidth="2" />
      <rect x="25" y="43" width="14" height="13" fill={p.main} />
      <path d={`M 32 43 V ${33 - v} M 32 39 L 26 34 M 32 37 L 38 31`} stroke={p.outline} strokeWidth="2" />
      <rect x="5" y="56" width="54" height="3" fill={p.accent} />
    </>,
  },
  land_reservoir: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="16" y="21" width="32" height="22" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 13 21 H 51 V 17 H 13 Z M 17 17 L 21 11 H 43 L 47 17 Z" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <rect x="20" y="43" width="5" height="13" fill={p.dark} />
      <rect x="39" y="43" width="5" height="13" fill={p.dark} />
      <path d={`M 24 28 H ${40 + v} V 34 H 24 Z`} fill={p.accent} />
      <rect x="14" y="56" width="36" height="3" fill={p.outline} />
      <rect x="27" y="21" width="3" height="22" fill={p.light} />
    </>,
  },
  land_crystal: {
    zone: "ground",
    render: (p, v) => <>
      <path d="M 11 53 H 53 V 59 H 11 Z" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d={`M 17 52 V 24 L ${32 + v} 7 L 47 24 V 52 Z`} fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 32 11 V 50 M 20 28 H 44 M 21 28 L 31 50 M 43 28 L 33 50" stroke={p.light} strokeWidth="2" />
      <rect x="27" y="55" width="10" height="4" fill={p.accent} />
      <path d="M 25 20 L 32 13 L 39 20 V 24 H 25 Z" fill={p.light} />
    </>,
  },
  land_school: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="10" y="29" width="44" height="28" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 7 29 L 32 13 L 57 29 Z" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <rect x="16" y="35" width="7" height="8" fill={p.light} />
      <rect x="27" y="35" width="9" height="22" fill={p.dark} />
      <rect x="41" y="35" width="7" height="8" fill={p.light} />
      <rect x="26" y="19" width="2" height="8" fill={p.accent} />
      <path d={`M 28 20 H ${38 + v} V 25 H 28 Z`} fill={p.accent} />
      <rect x="7" y="57" width="50" height="3" fill={p.outline} />
    </>,
  },
  land_observatory: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="10" y="36" width="44" height="21" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d="M 9 36 A 23 21 0 0 1 55 36 Z" fill={p.main} stroke={p.light} strokeWidth="2" />
      <path d={`M 32 32 V ${14 + v} M 26 25 H 38 M 32 26 L 42 18`} stroke={p.accent} strokeWidth="3" />
      <rect x="16" y="42" width="7" height="8" fill={p.light} />
      <rect x="41" y="42" width="7" height="8" fill={p.light} />
      <rect x="28" y="44" width="8" height="13" fill={p.outline} />
      <rect x="7" y="57" width="50" height="3" fill={p.accent} />
    </>,
  },
  land_laboratory: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="9" y="29" width="46" height="28" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <rect x="14" y="21" width="10" height="8" fill={p.dark} />
      <rect x="17" y="14" width="4" height="7" fill={p.main} />
      <path d="M 31 38 H 37 V 48 H 40 V 52 H 28 V 48 H 31 Z" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <rect x="13" y="35" width="8" height="6" fill={p.accent} />
      <rect x="43" y="35" width="7" height="9" fill={p.dark} />
      <path d={`M 44 37 H 49 V ${41 + v} H 44 Z`} fill={p.light} />
      <rect x="7" y="57" width="50" height="3" fill={p.outline} />
    </>,
  },
  land_market: {
    zone: "ground",
    render: (p) => <>
      <rect x="10" y="35" width="44" height="20" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d="M 8 35 L 13 22 H 51 L 56 35 Z" fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <path d="M 18 23 V 35 H 27 V 23 M 37 23 V 35 H 46 V 23" fill={p.light} />
      <rect x="14" y="41" width="9" height="7" fill={p.main} />
      <rect x="27" y="39" width="11" height="16" fill={p.light} />
      <rect x="42" y="41" width="8" height="7" fill={p.main} />
      <rect x="7" y="55" width="50" height="4" fill={p.outline} />
    </>,
  },
  land_trading_post: {
    zone: "ground",
    render: (p, v) => <>
      <path d="M 8 32 L 32 17 L 56 32 V 55 H 8 Z" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 5 32 L 32 11 L 59 32 H 51 L 32 18 L 13 32 Z" fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <rect x="14" y="37" width="11" height="10" fill={p.light} />
      <rect x="39" y="36" width="10" height="19" fill={p.dark} />
      <rect x="27" y="40" width="8" height="15" fill={p.dark} />
      <rect x="5" y="55" width="54" height="4" fill={p.outline} />
      <path d={`M 15 49 H ${24 + v} V 52 H 15 Z`} fill={p.accent} />
    </>,
  },
  land_freight: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="8" y="38" width="22" height="17" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <rect x="34" y="32" width="22" height="23" fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <path d="M 8 44 H 30 M 34 40 H 56 M 19 38 V 55 M 45 32 V 55" stroke={p.dark} strokeWidth="2" />
      <rect x="11" y="31" width="16" height="7" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <path d={`M 11 29 V ${14 + v} H 52 V 29 M 47 18 H 54`} fill="none" stroke={p.outline} strokeWidth="3" />
      <rect x="7" y="55" width="51" height="4" fill={p.dark} />
    </>,
  },
  land_bazaar: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="7" y="30" width="50" height="27" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d="M 5 30 L 11 18 H 53 L 59 30 Z" fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <path d="M 16 19 V 30 H 25 V 19 M 39 19 V 30 H 48 V 19" fill={p.light} />
      <rect x="12" y="37" width="8" height="8" fill={p.light} />
      <rect x="27" y="35" width="10" height="22" fill={p.main} />
      <rect x="44" y="37" width="8" height="8" fill={p.light} />
      <path d={`M 9 48 H ${24 + v} V 51 H 9 Z M 40 48 H 55 V 51 H ${40 + v} Z`} fill={p.accent} />
      <rect x="5" y="57" width="54" height="3" fill={p.outline} />
    </>,
  },
  land_rover: {
    zone: "ground",
    render: (p, v) => <>
      <circle cx="18" cy="52" r="6" fill={p.dark} stroke={p.accent} strokeWidth="2" />
      <circle cx="47" cy="52" r="6" fill={p.dark} stroke={p.accent} strokeWidth="2" />
      <rect x="10" y="35" width="45" height="15" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <rect x="18" y="27" width="17" height="9" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <path d={`M 26 27 V ${13 + v} M 24 18 H 38 L 42 22 H 23 Z`} fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <rect x="12" y="39" width="9" height="5" fill={p.accent} />
      <rect x="38" y="39" width="11" height="6" fill={p.light} />
    </>,
  },
  land_clocktower: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="19" y="21" width="26" height="36" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 15 21 L 32 9 L 49 21 Z" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <rect x="25" y="15" width="14" height="3" fill={p.accent} />
      <circle cx="32" cy="32" r="8" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <path d={`M 32 26 V 32 L ${36 + v} 35`} fill="none" stroke={p.dark} strokeWidth="2" />
      <rect x="28" y="45" width="8" height="12" fill={p.dark} />
      <rect x="16" y="57" width="32" height="3" fill={p.accent} />
    </>,
  },
  land_launchpad: {
    zone: "ground",
    render: (p, v) => <>
      <path d="M 11 51 H 53 V 58 H 11 Z M 18 45 H 46 V 51 H 18 Z" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d={`M 25 43 V 25 L 32 12 L ${39 + v} 25 V 43 Z`} fill={p.light} stroke={p.outline} strokeWidth="2" />
      <path d="M 25 34 L 18 43 H 25 M 39 34 L 46 43 H 39" fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <rect x="29" y="27" width="7" height="8" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 28 44 L 32 51 L 36 44 Z" fill={p.accent} />
      <rect x="7" y="58" width="50" height="2" fill={p.outline} />
    </>,
  },
  land_portal: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="10" y="19" width="7" height="38" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <rect x="47" y="19" width="7" height="38" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d="M 15 24 V 16 H 47 V 24 H 42 V 21 H 20 V 24 Z" fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <path d={`M 21 51 V 29 H ${43 - v} V 51 Z`} fill={p.main} />
      <path d="M 25 49 V 32 H 39 V 49 Z" fill={p.light} />
      <path d="M 5 57 H 59 V 60 H 5 Z" fill={p.outline} />
      <rect x="29" y="17" width="6" height="4" fill={p.light} />
    </>,
  },
  land_toolbox: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="9" y="32" width="46" height="25" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 21 31 V 23 H 43 V 31" fill="none" stroke={p.dark} strokeWidth="4" />
      <rect x="9" y="39" width="46" height="4" fill={p.accent} />
      <rect x="29" y="39" width="7" height="10" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <path d={`M 17 31 V ${18 + v} H 21 V 31 M 45 31 V 20 H 49 V 31`} fill={p.light} stroke={p.outline} strokeWidth="2" />
      <rect x="7" y="57" width="50" height="3" fill={p.dark} />
    </>,
  },
  land_excavator: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="10" y="48" width="36" height="8" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <circle cx="18" cy="52" r="3" fill={p.accent} />
      <circle cx="29" cy="52" r="3" fill={p.accent} />
      <circle cx="40" cy="52" r="3" fill={p.accent} />
      <rect x="18" y="34" width="23" height="13" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <rect x="22" y="26" width="13" height="9" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <path d={`M 39 34 L 47 22 L ${54 - v} 26 L 48 40 L 55 44 L 51 49 L 42 44 L 40 40`} fill={p.accent} stroke={p.outline} strokeWidth="2" />
    </>,
  },
  land_cutter: {
    zone: "ground",
    render: (p) => <>
      <rect x="9" y="43" width="46" height="14" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <rect x="14" y="34" width="18" height="9" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <circle cx="42" cy="34" r="12" fill={p.light} stroke={p.outline} strokeWidth="3" />
      <circle cx="42" cy="34" r="4" fill={p.accent} />
      <path d={`M 42 22 V 25 M 54 34 H 51 M 42 46 V 43 M 30 34 H 33 M 50 26 L 48 28 M 34 42 L 36 40`} stroke={p.dark} strokeWidth="2" />
      <rect x="11" y="49" width="8" height="4" fill={p.accent} />
    </>,
  },
  land_recycler: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="13" y="28" width="38" height="29" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <rect x="20" y="19" width="24" height="9" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <rect x="21" y="34" width="6" height="6" fill={p.light} />
      <rect x="37" y="34" width="6" height="6" fill={p.light} />
      <path d={`M 25 47 H ${39 + v} M 39 47 L 35 43 M 39 47 L 35 51`} stroke={p.accent} strokeWidth="3" />
      <rect x="8" y="57" width="48" height="3" fill={p.dark} />
      <path d="M 28 19 V 14 H 36 V 19" fill="none" stroke={p.accent} strokeWidth="3" />
    </>,
  },
  land_flag: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="14" y="56" width="36" height="4" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <rect x="30" y="13" width="4" height="43" fill={p.dark} />
      <path d={`M 34 15 H ${53 - v} V 31 H 43 L 34 36 Z`} fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <path d="M 38 19 H 47 V 22 H 38 Z M 38 26 H 45 V 29 H 38 Z" fill={p.light} />
      <rect x="27" y="12" width="10" height="3" fill={p.light} />
    </>,
  },
  land_lantern: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="30" y="30" width="5" height="27" fill={p.dark} />
      <path d="M 21 30 H 44 V 13 H 21 Z" fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <rect x="25" y="18" width="15" height="11" fill={p.light} />
      <path d="M 18 13 H 47 M 24 9 H 41 M 27 5 H 38" stroke={p.outline} strokeWidth="3" />
      <path d={`M 18 33 H ${47 + v} V 36 H 18 Z`} fill={p.main} />
      <rect x="23" y="56" width="20" height="3" fill={p.dark} />
    </>,
  },
  land_stars: {
    zone: "sky",
    render: (p, v) => <>
      <path d="M 15 37 L 34 21 L 54 39 L 77 17" fill="none" stroke={p.main} strokeWidth="2" />
      <path d="M 32 12 H 36 V 18 H 42 V 22 H 36 V 28 H 32 V 22 H 26 V 18 H 32 Z" fill={p.accent} />
      <path d="M 65 35 H 68 V 40 H 73 V 43 H 68 V 48 H 65 V 43 H 60 V 40 H 65 Z" fill={p.light} />
      <rect x="12" y="34" width="6" height="6" fill={p.light} />
      <rect x="51" y="36" width="5" height="5" fill={p.accent} />
      <rect x={78 - v} y="13" width="6" height="6" fill={p.light} />
    </>,
  },
  land_aurora: {
    zone: "sky",
    render: (p, v) => <>
      <path d="M 11 41 H 21 V 34 H 31 V 27 H 42 V 33 H 54 V 22 H 65 V 29 H 76 V 20 H 85 V 37 H 74 V 33 H 62 V 40 H 51 V 35 H 40 V 43 H 27 V 39 H 11 Z" fill={p.main} />
      <path d={`M 16 47 H 29 V 42 H 42 V 46 H 56 V 40 H 69 V 45 H ${82 - v} V 50 H 68 V 47 H 55 V 52 H 39 V 49 H 27 V 53 H 16 Z`} fill={p.light} />
      <rect x="13" y="55" width="68" height="2" fill={p.accent} />
      <path d="M 24 22 H 35 V 24 H 24 Z M 70 13 H 80 V 15 H 70 Z" fill={p.accent} />
    </>,
  },
  land_meteors: {
    zone: "sky",
    render: (p, v) => <>
      <path d={`M 18 14 L ${37 + v} 10 L 28 18 L 12 30 Z`} fill={p.light} stroke={p.outline} strokeWidth="2" />
      <path d="M 12 29 L 29 16 L 22 27 L 8 37 Z" fill={p.accent} />
      <path d="M 53 26 L 70 22 L 63 29 L 49 38 Z" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 63 30 L 80 26 L 72 34 L 57 41 Z" fill={p.light} />
      <path d="M 39 43 L 49 41 L 44 47 L 35 52 Z" fill={p.accent} />
      <rect x="78" y="45" width="5" height="5" fill={p.light} />
    </>,
  },
  land_thin_ring: {
    zone: "sky",
    render: (p, v) => <>
      <ellipse cx="48" cy="37" rx="35" ry="9" fill="none" stroke={p.accent} strokeWidth="3" />
      <circle cx="48" cy="34" r="13" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 38 37 H 57 V 42 H 40 Z" fill={p.dark} />
      <path d="M 42 25 H 53 V 28 H 42 Z" fill={p.light} />
      <rect x={65 - v} y="34" width="4" height="3" fill={p.light} />
    </>,
  },
  land_double_ring: {
    zone: "sky",
    render: (p, v) => <>
      <ellipse cx="48" cy="37" rx="38" ry="12" fill="none" stroke={p.accent} strokeWidth="2" />
      <ellipse cx="48" cy="37" rx="30" ry="7" fill="none" stroke={p.light} strokeWidth="2" />
      <circle cx="48" cy="34" r="12" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d="M 38 35 H 58 V 39 H 38 Z" fill={p.main} />
      <path d="M 44 25 H 52 V 28 H 44 Z" fill={p.accent} />
      <rect x="20" y={31 + v} width="4" height="3" fill={p.light} />
    </>,
  },
  land_moonlets: {
    zone: "sky",
    render: (p, v) => <>
      <ellipse cx="48" cy="37" rx="36" ry="15" fill="none" stroke={p.light} strokeWidth="2" />
      <circle cx="48" cy="35" r="13" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <path d="M 38 36 H 57 V 43 H 40 Z" fill={p.dark} />
      <rect x="17" y="32" width="7" height="7" fill={p.accent} stroke={p.outline} strokeWidth="2" />
      <rect x={73 - v} y="42" width="8" height="8" fill={p.light} stroke={p.outline} strokeWidth="2" />
      <rect x="56" y="18" width="5" height="5" fill={p.accent} />
      <rect x="35" y="16" width="4" height="4" fill={p.light} />
    </>,
  },
  land_garden: {
    zone: "ground",
    render: (p, v) => <>
      <path d="M 7 48 H 57 V 57 H 7 Z" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <rect x="12" y="44" width="40" height="5" fill={p.main} />
      <path d={`M 18 44 V ${28 + v} M 32 44 V 23 M 46 44 V ${29 - v}`} stroke={p.outline} strokeWidth="2" />
      <rect x="14" y="25" width="8" height="8" fill={p.accent} />
      <rect x="28" y="19" width="9" height="9" fill={p.light} />
      <rect x="42" y="26" width="8" height="8" fill={p.accent} />
      <rect x="17" y="28" width="3" height="3" fill={p.light} />
      <rect x="31" y="22" width="3" height="3" fill={p.main} />
      <rect x="44" y="29" width="3" height="3" fill={p.light} />
    </>,
  },
  land_tree: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="28" y="34" width="9" height="23" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d={`M 15 38 V 29 H 21 V 20 H 27 V ${13 + v} H 39 V 20 H 46 V 28 H 52 V 39 H 45 V 45 H 21 V 39 Z`} fill={p.main} stroke={p.outline} strokeWidth="2" />
      <rect x="21" y="26" width="8" height="6" fill={p.light} />
      <rect x="38" y="20" width="7" height="6" fill={p.light} />
      <rect x="31" y="37" width="4" height="4" fill={p.accent} />
      <path d="M 19 57 H 47" stroke={p.outline} strokeWidth="3" />
    </>,
  },
  land_bench: {
    zone: "ground",
    render: (p, v) => <>
      <rect x="9" y="33" width="46" height="6" fill={p.main} stroke={p.outline} strokeWidth="2" />
      <rect x="12" y="25" width="8" height="8" fill={p.accent} />
      <rect x="23" y="25" width="8" height="8" fill={p.light} />
      <rect x="34" y="25" width="8" height="8" fill={p.accent} />
      <rect x="45" y="25" width="8" height="8" fill={p.light} />
      <rect x="13" y="39" width="4" height="17" fill={p.dark} />
      <rect x="47" y="39" width="4" height="17" fill={p.dark} />
      <path d={`M 17 48 H ${47 + v} M 17 53 H 47`} stroke={p.outline} strokeWidth="3" />
      <rect x="8" y="56" width="48" height="3" fill={p.dark} />
    </>,
  },
  land_fountain: {
    zone: "ground",
    render: (p, v) => <>
      <path d="M 8 46 H 56 V 53 H 8 Z M 15 38 H 49 V 45 H 15 Z" fill={p.dark} stroke={p.outline} strokeWidth="2" />
      <path d="M 19 39 H 45 V 42 H 19 Z M 12 47 H 52 V 50 H 12 Z" fill={p.main} />
      <rect x="29" y="24" width="6" height="14" fill={p.accent} />
      <path d={`M 32 24 V ${14 + v} M 32 22 L 26 18 M 32 20 L 39 15`} fill="none" stroke={p.light} strokeWidth="2" />
      <rect x="29" y="35" width="6" height="4" fill={p.light} />
      <rect x="10" y="54" width="44" height="5" fill={p.outline} />
    </>,
  },
};

export function LandscapeObjectSprite({
  instance,
  product,
  selected = false,
}: {
  instance: LandscapeInstance;
  product: ShopProduct;
  selected?: boolean;
}) {
  const illustration = LANDSCAPE_ART[instance.sku];
  if (!illustration || product.sku !== instance.sku || product.category !== "landscape") return null;

  const variationIndex = Number.isInteger(instance.variation_index)
    && instance.variation_index >= 0
    && instance.variation_index < VARIANT_PALETTES.length
    ? instance.variation_index
    : 0;
  const palette = VARIANT_PALETTES[variationIndex];
  const bounds = LANDSCAPE_OBJECT_INTRINSIC_BOUNDS[illustration.zone];
  const [scaleX, scaleY] = VARIANT_SCALES[variationIndex];
  const centerX = bounds.width / 2;
  const centerY = bounds.height / 2;

  return (
    <g
      data-landscape-object={instance.instance_id}
      data-landscape-sku={instance.sku}
      data-landscape-bounds={`${bounds.width}x${bounds.height}`}
      data-landscape-seed={instance.seed}
      data-landscape-variation-version={instance.variation_version}
    >
      <g
        data-landscape-art={instance.sku}
        data-landscape-zone={illustration.zone}
        data-landscape-variation={variationIndex}
        transform={`translate(${centerX} ${centerY}) scale(${scaleX} ${scaleY}) translate(${-centerX} ${-centerY})`}
        shapeRendering="crispEdges"
      >
        {illustration.render(palette, variationIndex)}
      </g>
      {selected && (
        <rect
          data-landscape-selection="true"
          x="1"
          y="1"
          width={bounds.width - 2}
          height={bounds.height - 2}
          fill="none"
          stroke="#f1cf89"
          strokeWidth="2"
          shapeRendering="crispEdges"
        />
      )}
    </g>
  );
}
