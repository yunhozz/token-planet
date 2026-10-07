import { describe, expect, it } from "vitest";
import * as route from "../landscapeAvatarRoute";
import type { LandscapeBounds } from "../planetLandscapeLayout";
type Point = { x: number; y: number };
type Progress = { index: number; direction: 1 | -1 };
const api = route as { landscapeAvatarRoute?: (bounds: LandscapeBounds) => Point[]; advanceLandscapeAvatarRoute?: (length: number, progress: Progress) => Progress };
describe("full terrain avatar route", () => {
  it.each([{ x: 0, y: 0, width: 1420, height: 548 }, { x: -17.5, y: 22.3, width: 2000.1, height: 701.7 }, { x: 4, y: 8, width: 24, height: 33 }])("covers bounds %j with an orthogonal serpentine full-footprint route", (bounds) => {
    expect(api.landscapeAvatarRoute).toBeTypeOf("function");
    if (!api.landscapeAvatarRoute) return;
    const points = api.landscapeAvatarRoute(bounds);
    expect(points[0]).toEqual({ x: bounds.x, y: bounds.y });
    const xs = [...new Set(points.map(p => p.x))].sort((a,b) => a-b), ys = [...new Set(points.map(p => p.y))].sort((a,b) => a-b);
    expect(xs[xs.length - 1]).toBeCloseTo(bounds.x + bounds.width - 24);
    expect(ys[ys.length - 1]).toBeCloseTo(bounds.y + bounds.height - 33);
    expect(points.length).toBe(xs.length * ys.length);
    for (const point of points) { expect(point.x).toBeGreaterThanOrEqual(bounds.x); expect(point.y).toBeGreaterThanOrEqual(bounds.y); expect(point.x + 24).toBeLessThanOrEqual(bounds.x + bounds.width + 1e-9); expect(point.y + 33).toBeLessThanOrEqual(bounds.y + bounds.height + 1e-9); }
    for (let i=1;i<points.length;i++) { const dx=Math.abs(points[i].x-points[i-1].x),dy=Math.abs(points[i].y-points[i-1].y); expect(dx === 0 || dy === 0).toBe(true); expect(dx).toBeLessThanOrEqual(48 + 1e-9); expect(dy).toBeLessThanOrEqual(30 + 1e-9); }
    ys.forEach((y,row) => expect(points.filter(p => p.y === y).map(p => p.x)).toEqual(row % 2 ? [...xs].reverse() : xs));
  });
  it("rejects invalid or smaller-than-footprint bounds", () => {
    expect(api.landscapeAvatarRoute).toBeTypeOf("function");
    if (!api.landscapeAvatarRoute) return;
    for (const bounds of [{ x: NaN,y:0,width:40,height:40 }, {x:0,y:0,width:23,height:30}, {x:0,y:0,width:24,height:29}, {x:0,y:0,width:Infinity,height:30}]) expect(api.landscapeAvatarRoute(bounds)).toEqual([]);
  });
  it("visits every point and reverses only at endpoints without teleporting", () => {
    expect(api.advanceLandscapeAvatarRoute).toBeTypeOf("function");
    if (!api.advanceLandscapeAvatarRoute) return;
    let progress: Progress = {index:0,direction:1}; const visited=[0];
    for (let i=0;i<10;i++) { const next=api.advanceLandscapeAvatarRoute(6,progress); expect(Math.abs(next.index-progress.index)).toBe(1); if(next.direction !== progress.direction) expect([0,5]).toContain(progress.index); visited.push(next.index); progress=next; }
    expect(visited).toEqual([0,1,2,3,4,5,4,3,2,1,0]);
    expect(api.advanceLandscapeAvatarRoute(1,{index:0,direction:1}).index).toBe(0);
  });
});

it("keeps the 30px avatar and maximum 3px foot bob inside the terrain", () => {
  const bounds = { x: 5, y: 7, width: 240, height: 130 };
  const points = route.landscapeAvatarRoute(bounds);
  expect(points.length).toBeGreaterThan(0);
  for (const point of points) expect(point.y + 30 + 3).toBeLessThanOrEqual(bounds.y + bounds.height);
  expect(route.landscapeAvatarRoute({x:0,y:0,width:24,height:32})).toEqual([]);
});

import { layoutLandscape } from "../planetLandscapeLayout";
it("shares the expanded 300-object terrain fixture with Rust save geometry", () => {
  const objects = Array.from({ length: 300 }, (_, ordinal) => ({ stage: 0, ordinal, kind: "tree", x: 25, y: 50, seed: ordinal }));
  const bounds = layoutLandscape(objects).bounds;
  expect(bounds).toEqual({ x: 0, y: 0, width: 1420, height: 883 });
  for (const point of route.landscapeAvatarRoute(bounds)) expect(point.y + 33).toBeLessThanOrEqual(bounds.height);
});
