import assert from "node:assert/strict";
import test from "node:test";
import { emptyScene, rectFromPoints, readScene, SceneHistory, sourcePoint } from "./scene.ts";

/** @type {import("./scene.ts").Annotation} */
const rectangle = { id: "rectangle-1", kind: "rect", x: 80, y: 30,
  width: 100, height: 70, stroke: "#d53f4e", strokeWidth: 4, rotation: 0, scaleX: 1, scaleY: 1 };

test("a reverse drag includes fractional edge pixels and stays within the source", () => {
  assert.deepEqual(rectFromPoints({ x: 123.4, y: 95.8 }, { x: -4, y: 22.2 }, 120, 80),
    { x: 0, y: 22, width: 120, height: 58 });
});

test("mixed-DPI selection maps actual image bounds, including an offset monitor", () => {
  const bounds = { x: -640, y: 100, width: 1920, height: 1080 };
  assert.deepEqual(sourcePoint({ x: 320, y: 640 }, bounds, 3840, 2160), { x: 1920, y: 1080 });
  assert.deepEqual(sourcePoint({ x: -900, y: 1500 }, bounds, 3840, 2160), { x: 0, y: 2160 });
});

test("cropping does not change or remove annotations outside the visible crop", () => {
  const scene = emptyScene(800, 600);
  scene.objects.push(rectangle);
  scene.crop = { x: 350, y: 250, width: 200, height: 180 };
  const restored = readScene(JSON.stringify(scene), 800, 600);
  assert.deepEqual(restored.objects[0], rectangle);
  assert.equal(restored.sourceWidth, 800);
  assert.deepEqual(restored.crop, { x: 350, y: 250, width: 200, height: 180 });
});

test("clearing annotations is reversible while the immutable source and crop survive", () => {
  const scene = emptyScene(800, 600);
  scene.objects.push(rectangle);
  scene.crop = { x: 30, y: 20, width: 700, height: 550 };
  const history = new SceneHistory(scene);
  history.commit({ ...scene, objects: [] });
  const previous = history.undo();
  assert.deepEqual(previous, scene);
  assert.deepEqual(history.redo()?.crop, scene.crop);
});

test("new edits after undo discard only redo history, not prior edits", () => {
  const scene = emptyScene(800, 600);
  const history = new SceneHistory(scene, 4);
  const first = { ...scene, objects: [rectangle] };
  history.commit(first);
  history.commit({ ...first, crop: { x: 5, y: 5, width: 500, height: 300 } });
  assert.deepEqual(history.undo(), first);
  const second = { ...first, objects: [{ ...rectangle, x: 160 }] };
  history.commit(second);
  assert.equal(history.canRedo, false);
  assert.deepEqual(history.undo(), first);
  assert.deepEqual(history.undo(), scene);
});

test("malformed drafts are refused without silently replacing them by an empty scene", () => {
  const scene = emptyScene(800, 600);
  assert.throws(() => readScene(JSON.stringify({ ...scene, version: 2 }), 800, 600), /incompatible/);
  assert.throws(() => readScene(JSON.stringify({ ...scene, crop: { x: 0, y: 0, width: 900, height: 500 } }), 800, 600), /dépasse/);
  assert.throws(() => readScene(JSON.stringify({ ...scene, objects: [rectangle, rectangle] }), 800, 600), /double/);
  assert.throws(() => readScene(JSON.stringify({ ...scene, objects: [{ ...rectangle, x: "invalid" }] }), 800, 600), /illisible/);
  assert.throws(() => readScene(JSON.stringify({ ...scene, objects: [{ ...rectangle, points: [1, "bad"] }] }), 800, 600), /illisible/);
});
