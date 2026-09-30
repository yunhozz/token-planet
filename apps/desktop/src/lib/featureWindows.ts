import { isTauri } from "@tauri-apps/api/core";
import { WebviewWindow } from "@tauri-apps/api/webviewWindow";

export type FeatureWindow = "cosmetic-shop" | "growth-journal";

const featureWindowOptions: Record<FeatureWindow, { title: string; width: number; height: number; minWidth: number; minHeight: number }> = {
  "cosmetic-shop": { title: "행성 꾸미기 · Token Planet", width: 460, height: 780, minWidth: 380, minHeight: 560 },
  "growth-journal": { title: "성장 일지 · Token Planet", width: 680, height: 760, minWidth: 460, minHeight: 520 },
};

const openingWindows = new Map<FeatureWindow, Promise<void>>();

async function focusExistingWindow(label: FeatureWindow) {
  const existing = await WebviewWindow.getByLabel(label);
  if (!existing) return false;
  await existing.unminimize();
  await existing.show();
  await existing.setFocus();
  return true;
}

export function openFeatureWindow(feature: FeatureWindow): Promise<void> {
  if (!isTauri()) {
    const url = new URL(window.location.href);
    url.searchParams.set("window", feature);
    window.open(url, "_blank", "popup,width=760,height=760");
    return Promise.resolve();
  }

  const opening = openingWindows.get(feature);
  if (opening) return opening;

  const task = (async () => {
    if (await focusExistingWindow(feature)) return;

    const options = featureWindowOptions[feature];
    const webview = new WebviewWindow(feature, {
      url: `index.html?window=${feature}`,
      title: options.title,
      width: options.width,
      height: options.height,
      minWidth: options.minWidth,
      minHeight: options.minHeight,
      center: true,
      resizable: true,
      focus: true,
      visible: true,
    });

    await new Promise<void>((resolve, reject) => {
      void webview.once("tauri://created", () => resolve()).catch(reject);
      void webview.once("tauri://error", (event) => reject(event.payload)).catch(reject);
    });
  })();
  openingWindows.set(feature, task);
  return task.finally(() => {
    if (openingWindows.get(feature) === task) openingWindows.delete(feature);
  });
}
