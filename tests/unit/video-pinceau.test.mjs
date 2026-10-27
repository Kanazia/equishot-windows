// Le pinceau d'encre de Chine (ADR-024, 0.24) : les mêmes cas que les tests Rust de `effets.rs`.
import assert from "node:assert/strict";
import { test } from "node:test";

import { pinceau } from "../../src/dessin/pinceau.ts";

/** Un tracé droit vers la droite, `n` points à `vitesse` px/s, un toutes les 10 ms. */
const trace = (n, vitesse) => Array.from({ length: n }, (_, i) => [i * vitesse * 0.01, 0, i * 0.01]);

test("le pinceau épaissit le lent et affine le rapide", () => {
  const lent = pinceau(trace(60, 150), 6)[30];
  const rapide = pinceau(trace(60, 4000), 6)[30];
  assert.ok(lent > 9.0 && lent <= 11.4, `lent : ${lent}`);
  assert.ok(rapide < 3.0 && rapide >= 1.8, `rapide : ${rapide}`);
});

test("le pinceau s'effile aux deux bouts", () => {
  const l = pinceau(trace(60, 600), 6);
  assert.ok(l[0] < l[30] * 0.3 && l[59] < l[30] * 0.3, `bouts ${l[0]} et ${l[59]}, milieu ${l[30]}`);
  assert.ok(l[1] > l[0] && l[58] > l[59], "il s'ouvre puis se referme");
});

test("le pinceau garde un point seul et l'immobile", () => {
  assert.deepEqual(pinceau([[1, 1, 0]], 6), [6]);
  assert.deepEqual(pinceau([], 6), []);
  assert.ok(pinceau([[0, 0, 0], [50, 0, 0]], 6).every((w) => Number.isFinite(w) && w > 0));
});

test("le pinceau de la page rend les mêmes épaisseurs que celui du moteur", () => {
  // Valeurs relevées sur `pinceau` de effets.rs (test `le_pinceau_de_la_page_rend_les_memes_epaisseurs`).
  const l = pinceau([[0, 0, 0], [3, 0, 0.01], [9, 0, 0.02], [20, 0, 0.03], [24, 0, 0.04]], 6);
  assert.deepEqual(l.map((w) => Math.round(w * 1000) / 1000), ATTENDU);
});

const ATTENDU = [2.17, 5.424, 5.352, 3.18, 1.804];
