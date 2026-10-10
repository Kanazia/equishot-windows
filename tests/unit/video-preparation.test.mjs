// La logique pure du panneau de préparation (ADR-017) : la valeur affichée d'une
// liste de périphériques, et le volume en pourcentage.
import assert from "node:assert/strict";
import { test } from "node:test";

import { choixAffiche, choixLu, fileDeGlisser, fileDeUn, pourcent, REPLI_CAMERA } from "../../src/enregistrement/mise-en-scene.ts";

const liste = [
  { id: "a", nom: "Casque" },
  { id: "b", nom: "Webcam" },
];

test("un micro gardé absent retombe sur le défaut et le dit", () => {
  assert.deepEqual(choixAffiche(true, "z", liste), { valeur: "defaut", note: "Le micro choisi n'est plus branché : micro par défaut." });
  assert.deepEqual(choixAffiche(true, "b", liste), { valeur: "b", note: null });
  assert.deepEqual(choixAffiche(false, "b", liste), { valeur: "aucun", note: null });
  assert.deepEqual(choixAffiche(true, null, liste), { valeur: "defaut", note: null });
});

test("une caméra gardée absente retombe sur le défaut et le dit", () => {
  const cameras = [{ id: "c1", nom: "Webcam" }];
  assert.deepEqual(choixAffiche(true, "c9", cameras, REPLI_CAMERA), { valeur: "defaut", note: "La caméra choisie n'est plus branchée : caméra par défaut." });
  assert.deepEqual(choixAffiche(false, "c1", cameras, REPLI_CAMERA), { valeur: "aucun", note: null });
  assert.deepEqual(choixAffiche(true, null, [], REPLI_CAMERA), { valeur: "defaut", note: null });
});

test("une valeur de liste se relit en réglages", () => {
  assert.deepEqual(choixLu("aucun"), { actif: false, id: null });
  assert.deepEqual(choixLu("defaut"), { actif: true, id: null });
  assert.deepEqual(choixLu("b"), { actif: true, id: "b" });
});

test("un curseur glissé écrit dans l'ordre, la dernière valeur gagne, et le lancement l'attend", async () => {
  const ecrits = [];
  const enCours = [];
  const file = fileDeUn((v) => new Promise((ok) => enCours.push(() => (ecrits.push(v), ok()))), () => {});
  file.pousser(1);
  file.pousser(2);
  file.pousser(3);
  assert.equal(enCours.length, 1, "un seul envoi en vol");
  let fini = false;
  const attente = file.fini().then(() => (fini = true));
  enCours.shift()();
  await new Promise((ok) => setTimeout(ok));
  assert.equal(fini, false, "le lancement attend la dernière valeur");
  enCours.shift()();
  await attente;
  assert.deepEqual(ecrits, [1, 3]);
});

test("trois glissers rapprochés : aucun « fini » n'est écrasé par l'appui suivant", async () => {
  const ecrits = [];
  const enCours = [];
  const file = fileDeGlisser((v) => new Promise((ok) => enCours.push(() => (ecrits.push(v), ok()))));
  const tour = () => new Promise((ok) => setTimeout(ok));
  for (const g of ["a", "b", "c"]) {
    file.appui();
    file.pousser(`${g}1`);
    file.pousser(`${g}2`);
    file.pousser(`${g}-fini`);
    await Promise.resolve();
  }
  for (let i = 0; i < 20 && (await tour(), enCours.length); i++) enCours.shift()();
  assert.deepEqual(
    ecrits.filter((v) => v.endsWith("fini")),
    ["a-fini", "b-fini", "c-fini"],
  );
});

test("le volume s'affiche en pourcentage entier", () => {
  assert.equal(pourcent(1), "100 %");
  assert.equal(pourcent(1.555), "156 %");
});
