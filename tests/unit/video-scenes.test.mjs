// Les scènes (ADR-017, 0.19) : la logique pure du panneau et des réglages.
import assert from "node:assert/strict";
import { test } from "node:test";

import { libelleCible, nomDeScene, pourcent, resumeScene, toucheDuPanneau } from "../../src/enregistrement/mise-en-scene.ts";

test("le nom d'une scène se lit comme Rust le garde", () => {
  assert.deepEqual(nomDeScene("  Démo  "), { nom: "Démo" });
  assert.deepEqual(nomDeScene("   "), { erreur: "Donnez un nom à la scène." });
  assert.deepEqual(nomDeScene("é".repeat(40)), { nom: "é".repeat(40) });
  assert.deepEqual(nomDeScene("é".repeat(41)), { erreur: "40 caractères au plus pour le nom d'une scène." });
});

test("formulaire ouvert, Entrée enregistre et Échap le ferme, sans lancer ni abandonner", () => {
  assert.equal(toucheDuPanneau("Enter", true), "valider");
  assert.equal(toucheDuPanneau("Escape", true), "fermer");
  assert.equal(toucheDuPanneau("a", true), null);
  assert.equal(toucheDuPanneau("Enter", false), "lancer");
  assert.equal(toucheDuPanneau("Escape", false), "abandonner");
});

test("la cible proposée se nomme selon la mise en scène", () => {
  const ecran = { nom: "\\\\.\\DISPLAY1", position: [0, 0], taille: [1920, 1080] };
  assert.equal(libelleCible({ genre: "ecran", ecran }), "Cet écran");
  assert.equal(libelleCible({ genre: "zone", ecran, zone: [0, 0, 640, 480] }), "Cette zone");
  assert.equal(libelleCible(null), "Cette zone");
});

test("le résumé d'une scène dit sa cible, son son et sa bulle", () => {
  const ecran = { nom: "\\\\.\\DISPLAY1", position: [0, 0], taille: [1920, 1080] };
  const mise = { micro: false, microId: null, sonSysteme: true, sortieId: null, volumeMicro: 1, volumeSon: 0.5, camera: true, cameraId: null, cameraTaille: "petite", cameraMiroir: true, cameraPlace: [1, 1] };
  assert.equal(resumeScene({ cible: { genre: "zone", ecran, zone: [10, 20, 640, 480] }, mise }), `Zone 640 x 480 · sans micro, son de l'ordinateur ${pourcent(0.5)} · caméra petite`);
  assert.equal(resumeScene({ cible: { genre: "ecran", ecran }, mise: { ...mise, micro: true, volumeMicro: 1.5 } }), `Écran entier 1920 x 1080 · micro ${pourcent(1.5)}, son de l'ordinateur ${pourcent(0.5)} · caméra petite`);
  assert.equal(resumeScene({ cible: { genre: "aTracer" }, mise: { ...mise, sonSysteme: false, camera: false } }), "Zone à tracer · sans micro, sans son de l'ordinateur · sans caméra");
});
