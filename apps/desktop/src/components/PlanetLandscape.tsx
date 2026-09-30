import { useEffect, useMemo, useRef, useState } from "react";
import type { EquippedCosmetic, PlanetAvatar, PlanetObject } from "../types/usage";
import { AvatarSprite } from "./AvatarSprite";
import { PlanetObjectSprite } from "./PlanetObjectSprite";
import { STAGE_NAMES } from "./PlanetScene";
import { PlanetLandscapeDecorations } from "./PlanetLandscapeDecorations";
import { fitLandscape, landscapeViewBox } from "./planetLandscapeCamera";
import { layoutLandscape, type LandscapeBounds } from "./planetLandscapeLayout";

const SKY_BAND_HEIGHT = 220;
const DEFAULT_VIEWPORT = { width: 1200, height: 420 };

type PlanetLandscapeProps = {
  stage: number;
  avatar: PlanetAvatar;
  objects?: PlanetObject[];
  equippedCosmetics?: EquippedCosmetic[];
};

function sceneBounds(terrain: LandscapeBounds): LandscapeBounds {
  return {
    x: terrain.x,
    y: terrain.y - SKY_BAND_HEIGHT,
    width: terrain.width,
    height: terrain.height + SKY_BAND_HEIGHT,
  };
}

export function PlanetLandscape({ stage, avatar, objects = [], equippedCosmetics = [] }: PlanetLandscapeProps) {
  const viewportRef = useRef<HTMLDivElement>(null);
  const [viewport, setViewport] = useState(DEFAULT_VIEWPORT);
  const layout = useMemo(() => layoutLandscape(objects), [objects]);
  const landscapeBounds = useMemo(() => sceneBounds(layout.bounds), [layout.bounds]);
  const stageName = STAGE_NAMES[stage] ?? STAGE_NAMES[4];
  const camera = fitLandscape(landscapeBounds);
  const viewBox = landscapeViewBox(landscapeBounds, viewport, camera);
  const avatarX = layout.bounds.x + layout.bounds.width / 2;
  const avatarY = layout.bounds.y + layout.bounds.height - 51;

  useEffect(() => {
    const node = viewportRef.current;
    if (!node || typeof ResizeObserver === "undefined") return;

    const updateViewport = (width: number, height: number) => {
      if (!Number.isFinite(width) || !Number.isFinite(height) || width <= 0 || height <= 0) return;
      setViewport((current) => Math.abs(current.width - width) < 0.5 && Math.abs(current.height - height) < 0.5
        ? current
        : { width, height });
    };
    const initialSize = node.getBoundingClientRect();
    updateViewport(initialSize.width, initialSize.height);
    const observer = new ResizeObserver((entries) => {
      const entry = entries.find((candidate) => candidate.target === node);
      if (entry) updateViewport(entry.contentRect.width, entry.contentRect.height);
    });
    observer.observe(node);
    return () => observer.disconnect();
  }, []);

  return (
    <section className="planet-landscape" aria-label="행성 풍경">
      <div className="planet-landscape-viewport" ref={viewportRef}>
        <svg
          className="planet-landscape-svg"
          viewBox={`${viewBox.x} ${viewBox.y} ${viewBox.width} ${viewBox.height}`}
          preserveAspectRatio="xMidYMid meet"
          role="img"
          aria-label={`${stageName} 평면 풍경`}
          shapeRendering="crispEdges"
        >
          <PlanetLandscapeDecorations
            stage={stage}
            bounds={layout.bounds}
            viewBox={viewBox}
            equippedCosmetics={equippedCosmetics}
          />
          <g className="planet-landscape-objects">
            {layout.objects.map((placement) => (
              <g
                key={placement.id}
                data-landscape-object-id={placement.id}
                data-object-stage={placement.object.stage}
                data-object-kind={placement.object.kind}
              >
                <PlanetObjectSprite object={placement.object} x={placement.x} y={placement.y} scale={1} />
              </g>
            ))}
          </g>
          <g className="planet-landscape-avatar" transform={`translate(${avatarX} ${avatarY})`}>
            <AvatarSprite avatar={avatar} className="planet-landscape-avatar-sprite" />
          </g>
        </svg>
      </div>
    </section>
  );
}
