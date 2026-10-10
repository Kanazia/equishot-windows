// La bulle de caméra (ADR-017, 0.18) : la logique pure de sa page.
import assert from "node:assert/strict";
import { test } from "node:test";

import { camerasDe, contraintes, estAbsente, messageDe } from "../../src/camera/bulle.ts";

test("seules les entrées vidéo font des caméras, nommées à défaut d'étiquette", () => {
  const appareils = [
    { kind: "audioinput", deviceId: "m", label: "Micro" },
    { kind: "videoinput", deviceId: "c1", label: "Webcam" },
    { kind: "videoinput", deviceId: "c2", label: "" },
    { kind: "videoinput", deviceId: "", label: "sans autorisation" },
  ];
  assert.deepEqual(camerasDe(appareils), [
    { id: "c1", nom: "Webcam" },
    { id: "c2", nom: "Caméra 2" },
  ]);
});

test("la caméra choisie est exigée, celle par défaut est libre", () => {
  assert.deepEqual(contraintes("c1"), { audio: false, video: { deviceId: { exact: "c1" } } });
  assert.deepEqual(contraintes(null), { audio: false, video: true });
});

test("une caméra absente se reconnaît à son erreur", () => {
  assert.equal(estAbsente({ name: "OverconstrainedError" }), true);
  assert.equal(estAbsente({ name: "NotFoundError" }), true);
  assert.equal(estAbsente({ name: "NotAllowedError" }), false);
  assert.equal(estAbsente("rien"), false);
  assert.equal(messageDe({ name: "NotFoundError" }), "Aucune caméra branchée.");
  assert.equal(messageDe({ name: "NotAllowedError" }), "Caméra indisponible : NotAllowedError.");
});
