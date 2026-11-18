export type Point = { x: number; y: number };
export type Crop = { x: number; y: number; width: number; height: number };
export type AnnotationKind = "rect" | "arrow" | "pen" | "text" | "blackout";

export type Annotation = {
  id: string;
  kind: AnnotationKind;
  x: number;
  y: number;
  width: number;
  height: number;
  points?: number[];
  text?: string;
  stroke: string;
  strokeWidth: number;
  fill?: string;
  fontSize?: number;
  rotation: number;
  scaleX: number;
  scaleY: number;
};

export type Scene = {
  version: 1;
  sourceWidth: number;
  sourceHeight: number;
  crop: Crop;
  objects: Annotation[];
};

export type CaptureDocument = {
  id: string;
  source: string;
  width: number;
  height: number;
  workspace: string;
  document: string | null;
};

export function clamp(value: number, low: number, high: number): number {
  return Math.min(high, Math.max(low, value));
}

/** Coordinates are always pixels of the immutable source, never screen/DPI units. */
export function rectFromPoints(a: Point, b: Point, width: number, height: number): Crop {
  const left = clamp(Math.floor(Math.min(a.x, b.x)), 0, width);
  const top = clamp(Math.floor(Math.min(a.y, b.y)), 0, height);
  const right = clamp(Math.ceil(Math.max(a.x, b.x)), left, width);
  const bottom = clamp(Math.ceil(Math.max(a.y, b.y)), top, height);
  return { x: left, y: top, width: right - left, height: bottom - top };
}

/** The actual image bounds give both origin and independent scales, including mixed DPI. */
export function sourcePoint(client: Point, bounds: Crop, width: number, height: number): Point {
  return {
    x: clamp((client.x - bounds.x) * width / Math.max(1, bounds.width), 0, width),
    y: clamp((client.y - bounds.y) * height / Math.max(1, bounds.height), 0, height),
  };
}

export function emptyScene(width: number, height: number): Scene {
  return { version: 1, sourceWidth: width, sourceHeight: height,
    crop: { x: 0, y: 0, width, height }, objects: [] };
}

export function cloneScene(scene: Scene): Scene {
  return JSON.parse(JSON.stringify(scene)) as Scene;
}

export function readScene(json: string | null, width: number, height: number): Scene {
  if (!json) return emptyScene(width, height);
  const raw: unknown = JSON.parse(json);
  if (!raw || typeof raw !== "object") throw new Error("Le brouillon de capture est illisible.");
  const value = raw as Scene;
  if (value.version !== 1 || value.sourceWidth !== width || value.sourceHeight !== height ||
      !Array.isArray(value.objects) || value.objects.length > 2000 || !value.crop) {
    throw new Error("Ce brouillon utilise un format de capture incompatible. L’original reste conservé.");
  }
  const finite = (n: unknown) => typeof n === "number" && Number.isFinite(n);
  const c = value.crop;
  if (![c.x, c.y, c.width, c.height].every(finite) || c.width < 1 || c.height < 1 ||
      c.x < 0 || c.y < 0 || c.x + c.width > width || c.y + c.height > height) {
    throw new Error("Le recadrage du brouillon dépasse l’image originale.");
  }
  for (const item of value.objects) {
    if (!item || typeof item.id !== "string" ||
        !["rect", "arrow", "pen", "text", "blackout"].includes(item.kind) ||
        ![item.x, item.y, item.width, item.height, item.strokeWidth, item.rotation, item.scaleX, item.scaleY].every(finite) ||
        typeof item.stroke !== "string" || (item.fill != null && typeof item.fill !== "string") ||
        item.width < 0 || item.height < 0 || item.strokeWidth < 0 || item.strokeWidth > 100 ||
        Math.abs(item.scaleX) > 100 || Math.abs(item.scaleY) > 100 ||
        (item.text != null && (typeof item.text !== "string" || item.text.length > 20000)) ||
        (item.fontSize != null && (!finite(item.fontSize) || item.fontSize < 1 || item.fontSize > 1000)) ||
        (item.points != null && (!Array.isArray(item.points) || item.points.length > 100000 || !item.points.every(finite)))) {
      throw new Error("Une annotation du brouillon est illisible. L’original reste conservé.");
    }
  }
  if (new Set(value.objects.map(item => item.id)).size !== value.objects.length) {
    throw new Error("Le brouillon contient des identifiants d’annotation en double.");
  }
  return cloneScene(value);
}

/** Bounded structural snapshots; no copy of source bitmap enters history. */
export class SceneHistory {
  private entries: string[];
  private position = 0;
  private limit: number;

  constructor(scene: Scene, limit = 60) {
    this.entries = [JSON.stringify(scene)];
    this.limit = Math.max(2, limit);
  }

  get canUndo(): boolean { return this.position > 0; }
  get canRedo(): boolean { return this.position < this.entries.length - 1; }

  commit(scene: Scene): boolean {
    const value = JSON.stringify(scene);
    if (this.entries[this.position] === value) return false;
    this.entries = this.entries.slice(0, this.position + 1);
    this.entries.push(value);
    if (this.entries.length > this.limit) this.entries.shift();
    this.position = this.entries.length - 1;
    return true;
  }

  undo(): Scene | null {
    if (!this.canUndo) return null;
    return JSON.parse(this.entries[--this.position]) as Scene;
  }

  redo(): Scene | null {
    if (!this.canRedo) return null;
    return JSON.parse(this.entries[++this.position]) as Scene;
  }
}
