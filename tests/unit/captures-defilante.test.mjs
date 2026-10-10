// La façon de défiler gardée par la page de sélection (ADR-019) : relue avec repli sur
// « vers le bas » quand elle manque ou ne se reconnaît pas.
import assert from "node:assert/strict";
import { test } from "node:test";

import { FACONS, lireFacon } from "../../src/modules/captures/facon.ts";

test("façon absente ou inconnue : vers le bas", () => {
  assert.equal(lireFacon(null), "bas");
  assert.equal(lireFacon(""), "bas");
  assert.equal(lireFacon("gauche"), "bas");
  assert.equal(lireFacon('{"x":1}'), "bas");
});

test("chaque façon connue se relit telle quelle", () => {
  assert.deepEqual(FACONS, ["bas", "haut", "droite", "manuel"]);
  for (const f of FACONS) assert.equal(lireFacon(f), f);
});
