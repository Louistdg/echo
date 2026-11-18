import type Konva from "konva";
import { clamp, cloneScene, rectFromPoints, type Annotation, type Crop, type Point, type Scene } from "./scene";

export type Tool = "select" | "arrow" | "rect" | "pen" | "text" | "blackout" | "crop";
type KonvaRuntime = typeof import("konva").default;
type Callbacks = {
  changed: (scene: Scene) => void;
  selected: (id: string | null) => void;
  editText: (item: Annotation) => void;
  crop: (crop: Crop | null) => void;
};

/** Owns only the canvas. The serializable scene, disk writes and history stay in the UI. */
export class CaptureCanvas {
  private K: KonvaRuntime;
  private stage: Konva.Stage;
  private layer: Konva.Layer;
  private group: Konva.Group;
  private transformer: Konva.Transformer;
  private image: HTMLImageElement;
  private callbacks: Callbacks;
  private scene: Scene;
  private selectedId: string | null = null;
  private tool: Tool = "select";
  private zoom = 1;
  private enabled = true;
  private drawing: Annotation | null = null;
  private drawingNode: Konva.Shape | null = null;
  private origin: Point | null = null;
  private cropNode: Konva.Rect | null = null;
  private nodes = new Map<string, Konva.Shape>();
  style = { stroke: "#d53f4e", strokeWidth: 4, fontSize: 28 };

  constructor(K: KonvaRuntime, container: HTMLDivElement, image: HTMLImageElement, scene: Scene, callbacks: Callbacks) {
    this.K = K;
    this.image = image;
    this.scene = cloneScene(scene);
    this.callbacks = callbacks;
    this.stage = new K.Stage({ container, width: 1, height: 1 });
    this.layer = new K.Layer();
    this.group = new K.Group();
    this.transformer = new K.Transformer({
      rotateEnabled: true,
      flipEnabled: false,
      borderStroke: "#7560c7",
      anchorStroke: "#7560c7",
      anchorFill: "#ffffff",
      anchorSize: 9,
      anchorCornerRadius: 2,
      padding: 4,
      ignoreStroke: true,
      boundBoxFunc: (oldBox, newBox) => Math.abs(newBox.width) < 8 || Math.abs(newBox.height) < 8 ? oldBox : newBox,
    });
    this.layer.add(this.group);
    this.layer.add(this.transformer);
    this.stage.add(this.layer);
    this.stage.on("pointerdown", event => this.pointerDown(event));
    this.stage.on("pointermove", () => this.pointerMove());
    this.stage.on("pointerup pointercancel", () => this.pointerUp());
    this.stage.on("click tap", event => {
      if (!this.enabled || this.tool !== "select") return;
      if (event.target === this.stage || (event.target as Konva.Node).getAttr("echoSource")) this.select(null);
    });
    this.render();
  }

  private nodeFor(item: Annotation): Konva.Shape {
    const common = {
      id: item.id, x: item.x, y: item.y, rotation: item.rotation,
      scaleX: item.scaleX, scaleY: item.scaleY,
      stroke: item.stroke, strokeWidth: item.strokeWidth,
      draggable: this.tool === "select" && this.enabled,
      name: "annotation",
    };
    switch (item.kind) {
      case "text": return new this.K.Text({ ...common, text: item.text ?? "", width: item.width,
        fontFamily: "Segoe UI, sans-serif", fontSize: item.fontSize ?? 28,
        fill: item.fill ?? item.stroke, strokeWidth: 0, lineHeight: 1.25, padding: 0 });
      case "arrow": return new this.K.Arrow({ ...common, points: item.points ?? [0, 0, item.width, item.height],
        fill: item.stroke, pointerLength: Math.max(10, item.strokeWidth * 3), pointerWidth: Math.max(10, item.strokeWidth * 3),
        lineCap: "round", lineJoin: "round", hitStrokeWidth: Math.max(16, item.strokeWidth + 8) });
      case "pen": return new this.K.Line({ ...common, points: item.points ?? [0, 0],
        lineCap: "round", lineJoin: "round", hitStrokeWidth: Math.max(16, item.strokeWidth + 8) });
      case "blackout": return new this.K.Rect({ ...common, width: item.width, height: item.height,
        fill: "#000000", stroke: "#000000", strokeWidth: 0 });
      default: return new this.K.Rect({ ...common, width: item.width, height: item.height,
        fill: item.fill || undefined, hitStrokeWidth: Math.max(14, item.strokeWidth + 6) });
    }
  }

  private bindNode(node: Konva.Shape, item: Annotation) {
    node.on("click tap", event => {
      if (!this.enabled || this.tool !== "select") return;
      event.cancelBubble = true;
      this.select(item.id);
    });
    node.on("dragstart transformstart", () => this.select(item.id));
    node.on("dragend transformend", () => {
      if (!this.enabled) return;
      const index = this.scene.objects.findIndex(value => value.id === item.id);
      if (index < 0) return;
      const updated = { ...this.scene.objects[index],
        x: node.x(), y: node.y(), rotation: node.rotation(),
        scaleX: node.scaleX(), scaleY: node.scaleY() };
      this.scene.objects[index] = updated;
      this.callbacks.changed(cloneScene(this.scene));
    });
    if (item.kind === "text") node.on("dblclick dbltap", event => {
      if (!this.enabled) return;
      event.cancelBubble = true;
      const value = this.scene.objects.find(value => value.id === item.id);
      if (value) this.callbacks.editText({ ...value });
    });
  }

  private render() {
    this.group.destroyChildren();
    this.nodes.clear();
    this.cropNode = null;
    this.drawing = null; this.drawingNode = null; this.origin = null;
    this.group.add(new this.K.Image({ image: this.image, width: this.scene.sourceWidth,
      height: this.scene.sourceHeight, listening: false, echoSource: true }));
    for (const item of this.scene.objects) {
      const node = this.nodeFor(item);
      this.bindNode(node, item);
      this.nodes.set(item.id, node);
      this.group.add(node);
    }
    this.updateGeometry();
    this.select(this.selectedId, false);
  }

  private updateGeometry() {
    const crop = this.scene.crop;
    this.stage.size({ width: Math.max(1, Math.ceil(crop.width * this.zoom)), height: Math.max(1, Math.ceil(crop.height * this.zoom)) });
    this.group.position({ x: -crop.x * this.zoom, y: -crop.y * this.zoom });
    this.group.scale({ x: this.zoom, y: this.zoom });
    this.layer.batchDraw();
  }

  private position(): Point | null {
    const point = this.group.getRelativePointerPosition();
    if (!point) return null;
    const crop = this.scene.crop;
    return { x: clamp(point.x, crop.x, crop.x + crop.width), y: clamp(point.y, crop.y, crop.y + crop.height) };
  }

  private pointerDown(event: Konva.KonvaEventObject<PointerEvent>) {
    if (!this.enabled || event.evt.button !== 0) return;
    this.stage.container().focus({ preventScroll: true });
    if (this.tool === "select") return;
    const point = this.position();
    if (!point) return;
    event.evt.preventDefault();
    const target = event.evt.target;
    if (target instanceof Element && "setPointerCapture" in target) {
      try { target.setPointerCapture(event.evt.pointerId); } catch { /* The native gesture may already have ended. */ }
    }
    this.select(null);
    if (this.tool === "text") {
      const item = this.newAnnotation("text", point);
      item.width = Math.max(100, Math.min(280, this.scene.crop.x + this.scene.crop.width - point.x));
      item.height = this.style.fontSize * 1.25;
      item.text = "";
      this.callbacks.editText(item);
      return;
    }
    this.origin = point;
    if (this.tool === "crop") {
      this.cropNode?.destroy();
      this.cropNode = new this.K.Rect({ x: point.x, y: point.y, width: 0, height: 0,
        stroke: "#7560c7", strokeWidth: 2, strokeScaleEnabled: false, dash: [7, 5],
        fill: "#7560c722", listening: false });
      this.group.add(this.cropNode);
      this.callbacks.crop(null);
      return;
    }
    this.drawing = this.newAnnotation(this.tool, point);
    this.drawingNode = this.nodeFor(this.drawing);
    this.drawingNode.listening(false);
    this.group.add(this.drawingNode);
  }

  private newAnnotation(kind: Annotation["kind"], point: Point): Annotation {
    return { id: crypto.randomUUID(), kind, x: point.x, y: point.y, width: 0, height: 0,
      points: kind === "arrow" || kind === "pen" ? [0, 0, .1, .1] : undefined,
      stroke: this.style.stroke, strokeWidth: this.style.strokeWidth,
      fill: kind === "text" ? this.style.stroke : undefined,
      fontSize: kind === "text" ? this.style.fontSize : undefined,
      rotation: 0, scaleX: 1, scaleY: 1 };
  }

  private pointerMove() {
    if (!this.enabled || !this.origin) return;
    const point = this.position();
    if (!point) return;
    if (this.tool === "crop" && this.cropNode) {
      this.cropNode.setAttrs(rectFromPoints(this.origin, point, this.scene.sourceWidth, this.scene.sourceHeight));
      this.layer.batchDraw();
      return;
    }
    if (!this.drawing || !this.drawingNode) return;
    const item = this.drawing;
    if (item.kind === "rect" || item.kind === "blackout") {
      Object.assign(item, rectFromPoints(this.origin, point, this.scene.sourceWidth, this.scene.sourceHeight));
      this.drawingNode.setAttrs({ x: item.x, y: item.y, width: item.width, height: item.height });
    } else {
      const dx = point.x - this.origin.x, dy = point.y - this.origin.y;
      if (item.kind === "arrow") item.points = [0, 0, dx, dy];
      else {
        const points = item.points!;
        const lastX = points[points.length - 2], lastY = points[points.length - 1];
        if (Math.hypot(dx - lastX, dy - lastY) < .8 / this.zoom || points.length >= 100000) return;
        points.push(dx, dy);
      }
      item.width = Math.abs(dx); item.height = Math.abs(dy);
      this.drawingNode.setAttr("points", item.points);
    }
    this.layer.batchDraw();
  }

  private pointerUp() {
    if (!this.origin) return;
    this.pointerMove();
    if (this.tool === "crop" && this.cropNode) {
      const crop = { x: this.cropNode.x(), y: this.cropNode.y(), width: this.cropNode.width(), height: this.cropNode.height() };
      if (crop.width >= 4 && crop.height >= 4) this.callbacks.crop(crop);
      else { this.cropNode.destroy(); this.cropNode = null; }
    } else if (this.drawing) {
      const item = this.drawing;
      const valid = item.kind === "pen" ? (item.points?.length ?? 0) >= 6
        : item.kind === "arrow" ? Math.hypot(item.width, item.height) >= 4 : item.width >= 2 && item.height >= 2;
      this.drawingNode?.destroy();
      this.drawingNode = null;
      if (valid && this.scene.objects.length < 2000) {
        this.scene.objects.push(item);
        const node = this.nodeFor(item);
        this.bindNode(node, item);
        this.group.add(node);
        this.nodes.set(item.id, node);
        this.callbacks.changed(cloneScene(this.scene));
      }
      this.drawing = null;
    }
    this.origin = null;
    this.layer.batchDraw();
  }

  setScene(scene: Scene) { this.scene = cloneScene(scene); this.render(); }
  setZoom(zoom: number) { this.zoom = clamp(zoom, .05, 4); this.updateGeometry(); }
  setTool(tool: Tool) {
    this.pointerUp(); this.tool = tool;
    this.cropNode?.destroy(); this.cropNode = null;
    this.callbacks.crop(null);
    for (const node of this.nodes.values()) node.draggable(this.enabled && tool === "select");
    this.transformer.visible(tool === "select" && this.enabled);
    this.stage.container().style.cursor = tool === "select" ? "default" : tool === "text" ? "text" : "crosshair";
    this.layer.batchDraw();
  }
  setEnabled(enabled: boolean) {
    if (!enabled) this.pointerUp();
    this.enabled = enabled;
    for (const node of this.nodes.values()) node.draggable(enabled && this.tool === "select");
    this.transformer.visible(enabled && this.tool === "select");
    this.layer.batchDraw();
  }
  select(id: string | null, notify = true) {
    const node = id ? this.nodes.get(id) : undefined;
    this.selectedId = node ? id : null;
    this.transformer.nodes(node ? [node] : []);
    this.transformer.visible(this.enabled && this.tool === "select");
    this.layer.batchDraw();
    if (notify) this.callbacks.selected(this.selectedId);
  }
  hideText(id: string | null) {
    for (const [key, node] of this.nodes) node.visible(key !== id);
    this.transformer.visible(!id && this.enabled && this.tool === "select");
    this.layer.batchDraw();
  }
  clearCrop() { this.cropNode?.destroy(); this.cropNode = null; this.callbacks.crop(null); this.layer.batchDraw(); }

  exportPng(): string {
    // Export from a fresh source-sized scene: no zoom, selection handles or viewport shadows.
    const crop = this.scene.crop;
    const container = document.createElement("div");
    const stage = new this.K.Stage({ container, width: crop.width, height: crop.height });
    const layer = new this.K.Layer();
    const group = new this.K.Group({ x: -crop.x, y: -crop.y });
    group.add(new this.K.Image({ image: this.image, width: this.scene.sourceWidth, height: this.scene.sourceHeight }));
    for (const item of this.scene.objects) group.add(this.nodeFor(item));
    layer.add(group); stage.add(layer); stage.draw();
    try { return stage.toDataURL({ mimeType: "image/png", pixelRatio: 1 }); }
    finally { stage.destroy(); }
  }

  destroy() { this.stage.destroy(); this.nodes.clear(); }
}
