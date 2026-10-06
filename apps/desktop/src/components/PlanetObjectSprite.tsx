import type { CSSProperties, JSX } from "react";
import type { PlanetObject } from "../types/usage";

type MotionKind = "plant" | "creature" | "water" | "structure" | "signal" | "rock" | "ground";

function motionKind(kind: string): MotionKind {
  if (["tree", "fern", "crops"].includes(kind)) return "plant";
  if (kind === "creature") return "creature";
  if (kind === "water") return "water";
  if (["satellite", "rocket"].includes(kind)) return "signal";
  if (["path", "road", "rail"].includes(kind)) return "ground";
  if (kind === "rock") return "rock";
  return "structure";
}

export function PlanetObjectSprite({ object, x, y, scale }: { object: PlanetObject; x: number; y: number; scale: number }): JSX.Element {
  const color = object.stage < 2 ? "#7db978" : object.stage === 2 ? "#e8bd75" : object.stage === 3 ? "#d18473" : "#80d4ce";
  const phase = Math.abs(Math.trunc((Number.isFinite(object.seed) ? object.seed : object.ordinal) % 23));
  const motionStyle = {
    "--object-motion-duration": `${3.1 + phase * 0.11}s`,
    "--object-motion-delay": `${-(phase % 12) * 0.29}s`,
  } as CSSProperties;
  const motion = motionKind(object.kind);

  let artwork: JSX.Element;
  switch (object.kind) {
    case "rail":
      artwork = <><path d="M2 -2V7M7 -2V7M12 -2V7M17 -2V7" stroke="#8e6f55" strokeWidth="2"/><path d="M0 0H20M0 5H20" stroke="#acbcc0" strokeWidth="2"/></>;
      break;
    case "road":
      artwork = <><rect y="-1" width="20" height="8" fill="#394c55"/><rect width="20" height="5" fill="#56646e"/><path d="M1 2H5M8 2H12M15 2H19" stroke="#edd494"/><path d="M0 0H20" stroke="#a2b4af"/><path d="M0 6H20" stroke="#293f49"/></>;
      break;
    case "path":
      artwork = <><path d="M0 0H20V7H0Z" fill="#806a50"/><path d="M0 1H20V5H0Z" fill="#b08f70"/><path d="M0 1H20" stroke="#d4b98a"/><path d="M2 3H5M8 4H12M16 2H18" stroke="#e0bc88"/><path d="M5 5H8M13 6H17" stroke="#927458"/></>;
      break;
    case "satellite":
      artwork = <><rect x="7" y="2" width="6" height="9" fill="#dfd9bb"/><path d="M-3 3H5V10H-3ZM15 3H23V10H15Z" fill="#4d7b9f" stroke="#88c9d0"/><path d="M10 2V-5H15" fill="none" stroke="#d0c4a2"/><rect x="9" y="5" width="2" height="2" fill="#e8bf79"/></>;
      break;
    case "solar":
      artwork = <><path d="M0 2H20L17 12H-3Z" fill="#476f9e" stroke="#8dc9d0"/><path d="M5 2 2 12M12 2 9 12M0 7H18" stroke="#8dc9d0"/><path d="M7 12V18M2 18H13" stroke="#a5b1aa" strokeWidth="2"/></>;
      break;
    case "habitat":
      artwork = <><path d="M-1 18V7H1V3H4V0H8V-2H13V0H17V3H20V7H22V18Z" fill="#8ac4bf"/><path d="M2 7H19M6 0V16M15 0V16" stroke="#537d8d"/><rect x="8" y="10" width="5" height="8" fill="#e0d7bc"/><path d="M3 10H5M16 10H19" stroke="#d9ece0" strokeWidth="2"/></>;
      break;
    case "laboratory":
      artwork = <><rect x="0" y="6" width="21" height="12" fill="#a5c4c2"/><path d="M3 6V-2H17V6Z" fill="#74949d"/><rect x="6" y="0" width="8" height="3" fill="#aee5dc"/><path d="M3 10H8V14H3ZM13 10H18V14H13Z" fill="#527c91"/><rect x="9" y="11" width="3" height="7" fill="#416675"/><path d="M10 -2V-6H15" fill="none" stroke="#e8bf79"/></>;
      break;
    case "tower":
      artwork = <><path d="M4 18V-7H16V18Z" fill="#789aab"/><path d="M2 -7H18V-4H2Z" fill="#a7ccd0"/><path d="M7 -2H9M11 -2H13M7 3H9M11 3H13M7 8H9M11 8H13" stroke="#e8d291" strokeWidth="2"/><rect x="9" y="13" width="3" height="5" fill="#3b5666"/></>;
      break;
    case "factory":
      artwork = <><path d="M0 18V5L6 1V5L12 1V5H20V18Z" fill="#ae8173"/><rect x="16" y="-8" width="3" height="13" fill="#71818b"/><path d="M3 9H6V12H3ZM10 9H13V12H10Z" fill="#edc77d"/><rect x="15" y="12" width="3" height="6" fill="#493f49"/><path d="M1 16H13" stroke="#d0a28b"/></>;
      break;
    case "power":
      artwork = <><path d="M2 18 6 -5H14L18 18ZM4 10H16M5 3H15" fill="none" stroke="#a3b9b6" strokeWidth="2"/><path d="M-2 -2H22M0 5H20" stroke="#6f8186" strokeWidth="2"/><path d="M10 2 7 8H11L9 14 14 6H10Z" fill="#e8bf79"/></>;
      break;
    case "district":
      artwork = <><path d="M0 18V3H7V18M8 18V-4H15V18M16 18V6H23V18" fill="#bca58b"/><path d="M2 6H5M2 10H5M10 0H13M10 5H13M10 10H13M18 9H21" stroke="#8bcbcc" strokeWidth="2"/></>;
      break;
    case "plaza":
      artwork = <><path d="M-2 12H22V18H-2Z" fill="#a7a999"/><rect x="4" y="7" width="12" height="5" fill="#768b90"/><rect x="8" y="0" width="4" height="8" fill="#b1caca"/><path d="M6 3H14M2 15H18" stroke="#d5e7d1"/><rect x="8" y="-3" width="4" height="3" fill="#80d4ce"/></>;
      break;
    case "workshop":
      artwork = <><rect x="0" y="5" width="20" height="13" fill="#b68b66"/><path d="M-2 5H22L17 -2H3Z" fill="#646b71"/><rect x="3" y="9" width="9" height="9" fill="#564b46"/><path d="M4 11H11M4 14H11" stroke="#997d5a"/><rect x="15" y="8" width="3" height="4" fill="#e8bf79"/><rect x="15" y="-6" width="3" height="5" fill="#807773"/></>;
      break;
    case "well":
      artwork = <><rect x="2" y="10" width="16" height="7" fill="#83949a"/><rect x="5" y="10" width="10" height="3" fill="#385c67"/><path d="M2 10V0H18V10M0 1H20" fill="none" stroke="#9f795b" strokeWidth="2"/><rect x="9" y="2" width="2" height="9" fill="#c8ae78"/><path d="M3 15H8M11 13H17" stroke="#c0c8b7"/></>;
      break;
    case "market":
      artwork = <><rect x="2" y="5" width="17" height="12" fill="#b88b69"/><path d="M-2 3H22L19 -2H1Z" fill="#e7b866"/><path d="M0 3V6H4V3M8 3V6H12V3M16 3V6H20V3" stroke="#895c60" strokeWidth="3"/><rect x="4" y="9" width="13" height="5" fill="#493e45"/><path d="M5 12H8M10 12H13M15 12H17" stroke="#8fbd7f" strokeWidth="2"/></>;
      break;
    case "house":
      artwork = <><rect x="1" y="1" width="18" height="17" fill="#e0cba0"/><path d="M-1 2V-1H4V-4H16V-1H21V2Z" fill="#925c5a"/><rect x="9" y="11" width="4" height="7" fill="#574b51"/><path d="M4 5H7V8H4ZM13 5H16V8H13Z" fill="#85bfcb"/><rect x="16" y="-6" width="2" height="5" fill="#b08876"/></>;
      break;
    case "cottage":
      artwork = <><rect x="2" y="5" width="16" height="12" fill="#c8946e"/><path d="M0 6V3H4V0H8V-3H12V0H16V3H20V6Z" fill="#715450"/><rect x="5" y="8" width="4" height="4" fill="#a3d6bf"/><rect x="12" y="9" width="4" height="8" fill="#59473c"/><path d="M3 14H10M4 6H17" stroke="#e7bf87"/></>;
      break;
    case "camp":
      artwork = <><path d="M0 17 10 -3 22 17Z" fill="#bd805c"/><path d="M8 17 12 6 17 17Z" fill="#463e3c"/><path d="M10 -3 6 7H8L12 -1Z" fill="#e8bd75"/><rect x="-2" y="17" width="26" height="1" fill="#785343"/></>;
      break;
    case "fern":
      artwork = <><path d="M1 16H19V18H1Z" fill="#3b6550" opacity=".55"/>
        <path d="M9 16V5M8 16 4 9 1 7M12 16 16 9 19 7" fill="none" stroke="#456f4b" strokeWidth="2"/>
        <path d="M9 7H6V4H8V5H9ZM11 8H14V5H12V6H11ZM8 11H3V8H5V9H8ZM12 12H17V9H15V10H12ZM7 14H1V11H4V12H7ZM13 15H19V12H16V13H13Z" fill="#6ba66c"/>
        <path d="M6 4H8M3 8H5M1 11H4M12 5H14M15 9H17M16 12H19" stroke="#a0ca7b"/>
        <path d="M9 16V6M11 16V9M6 15 4 11M14 15 16 11" stroke="#8fbd70"/>
        <rect x="7" y="15" width="6" height="2" fill="#56794b"/></>;
      break;
    case "rock":
      artwork = <><rect width="12" height="7" fill="#87949b" /><rect x="3" y="-4" width="6" height="4" fill="#aeb7ae" /><rect x="1" y="5" width="10" height="2" fill="#566970" /><path d="M6 -1H8V2H7V4" fill="none" stroke="#6b7e87" /><rect className="planet-object-rock-glint" x="4" y="-3" width="2" height="2" fill="#e5ead7" /></>;
      break;
    case "water":
      artwork = <><rect width="18" height="5" fill="#70c7c8" /><rect className="planet-object-water-glint" x="4" y="-3" width="10" height="3" fill="#a6e2d1" /><rect x="1" y="4" width="16" height="2" fill="#438c9b" /><rect x="2" y="1" width="4" height="1" fill="#cff1df" /><rect x="11" y="3" width="3" height="1" fill="#e7e3be" /><path className="planet-object-water-ripple" d="M 2 3 h 5 m 7 0 h 3" stroke="#e0fff0" strokeWidth="1" /></>;
      break;
    case "tree":
      artwork = <g className="planet-object-plant-art"><rect x="6" y="7" width="4" height="9" fill="#956449" /><rect x="2" y="2" width="12" height="7" fill="#548f61" /><rect x="5" y="-2" width="7" height="5" fill="#83bf76" /><path d="M1 6H5V10H2ZM10 4H16V8H13Z" fill="#3d7051" /><rect x="5" y="0" width="3" height="1" fill="#b1d993" /><rect x="3" y="3" width="3" height="1" fill="#a0cd82" /><rect x="7" y="10" width="1" height="5" fill="#c18c5d" /><rect x="5" y="15" width="7" height="1" fill="#674e3c" /></g>;
      break;
    case "creature":
      artwork = <><rect x="1" y="2" width="13" height="8" fill="#f0b969" /><rect x="11" width="6" height="6" fill="#f0b969" /><rect x="14" y="1" width="1" height="1" fill="#292d3f" /><rect x="12" y="-2" width="2" height="2" fill="#b27c57" /><rect x="4" y="2" width="5" height="2" fill="#ffe0a0" /><rect x="2" y="7" width="10" height="2" fill="#cc925d" /><rect x="16" y="3" width="2" height="2" fill="#624f44" /><path d="M1 4H-2V1H-3V6H1Z" fill="#b27c57" /><rect x="3" y="9" width="2" height="4" fill="#9b6658" /><rect x="11" y="9" width="2" height="4" fill="#9b6658" /></>;
      break;
    case "crops":
      artwork = <><rect width="20" height="5" fill="#92b95c" /><g className="planet-object-crop-stems"><rect x="3" y="-4" width="2" height="4" fill={color} /><rect x="9" y="-4" width="2" height="4" fill={color} /><rect x="15" y="-4" width="2" height="4" fill={color} /></g></>;
      break;
    case "rocket":
      artwork = <><rect x="7" y="-2" width="6" height="17" fill="#e7ddbd" /><path d="m7 1-5 6h5m6-6 5 6h-5" fill="#70c7c8" /><rect x="8" y="-6" width="4" height="4" fill="#e17e6e" /><rect x="8" y="14" width="4" height="4" fill="#e17e6e" /><circle className="planet-object-signal-light" cx="10" cy="-8" r="2" fill="#f1cf89" /></>;
      break;
    default:
      artwork = <><rect x="2" y="2" width="16" height="11" fill={color} /><rect x="5" y="-2" width="10" height="4" fill="#e7ddbd" /><rect className="planet-object-window" x="6" y="5" width="3" height="3" fill="#a4ddcc" /><rect className="planet-object-window planet-object-window--late" x="12" y="5" width="3" height="3" fill="#a4ddcc" /></>;
      break;
  }

  return <g className="planet-object-sprite" transform={`translate(${x} ${y})`}>
    <g transform={`scale(${scale})`}>
      <g className={`planet-object-motion planet-object-motion--${motion}`} style={motionStyle}>
        {artwork}
      </g>
    </g>
  </g>;
}
