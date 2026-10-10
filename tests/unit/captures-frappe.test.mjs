// L'enregistreur du raccourci de capture retient la lettre que la touche écrit,
// pas sa position : sur un AZERTY, la touche marquée A a le code `KeyQ`, et le
// greffon de raccourci ferait de `KeyQ` la touche marquée Q.
import assert from "node:assert/strict";
import { test } from "node:test";

import { combinaisonCliquee, combinaisonFrappee, toucheFrappee, touchesLisibles } from "../../src/modules/captures/frappe.ts";

/** Ce que `navigator.keyboard.getLayoutMap()` rend sur un clavier français. */
const AZERTY = new Map([["KeyQ", "a"], ["KeyA", "q"], ["KeyW", "z"], ["KeyZ", "w"], ["KeyS", "s"], ["KeyM", ","], ["Semicolon", "m"], ["Digit1", "&"], ["KeyE", "e"]]);
const QWERTY = new Map([["KeyQ", "q"], ["KeyA", "a"], ["KeyS", "s"], ["KeyM", "m"], ["Semicolon", ";"]]);
const frappe = (code, key, modificateurs = {}) => ({ code, key, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...modificateurs });

test("sur un AZERTY, la lettre écrite l'emporte sur la position", () => {
  assert.equal(toucheFrappee(frappe("KeyQ", "a"), AZERTY), "KeyA");
  assert.equal(toucheFrappee(frappe("KeyA", "q"), AZERTY), "KeyQ");
  assert.equal(toucheFrappee(frappe("KeyW", "z"), AZERTY), "KeyZ");
  assert.equal(toucheFrappee(frappe("KeyS", "s"), AZERTY), "KeyS");
  // La touche marquée M d'un AZERTY est à la place du point-virgule américain.
  assert.equal(toucheFrappee(frappe("Semicolon", "m"), AZERTY), "KeyM");
});

test("sur un QWERTY, rien ne change", () => {
  assert.equal(toucheFrappee(frappe("KeyQ", "q"), QWERTY), "KeyQ");
  assert.equal(toucheFrappee(frappe("KeyA", "a"), QWERTY), "KeyA");
});

test("une touche de ponctuation n'a pas de nom sûr", () => {
  assert.equal(toucheFrappee(frappe("KeyM", ","), AZERTY), null, "la virgule d'un AZERTY");
  assert.equal(toucheFrappee(frappe("Semicolon", ";"), QWERTY), null);
  assert.equal(toucheFrappee(frappe("Minus", ")"), AZERTY), null, "absente de la disposition lue");
});

test("chiffres, fonctions, flèches et pavé gardent leur code", () => {
  // La rangée du haut d'un AZERTY écrit « & » sans Maj : c'est bien la touche du 1.
  assert.equal(toucheFrappee(frappe("Digit1", "&"), AZERTY), "Digit1");
  for (const code of ["F9", "F12", "ArrowUp", "Numpad5", "Space", "PrintScreen"]) {
    assert.equal(toucheFrappee(frappe(code, code), AZERTY), code);
  }
});

test("sans disposition, la lettre de l'évènement fait foi", () => {
  assert.equal(toucheFrappee(frappe("KeyQ", "a"), null), "KeyA", "un vrai clavier AZERTY donne déjà « a »");
  assert.equal(toucheFrappee(frappe("KeyE", "€"), null), null, "AltGr+E écrit l'euro : pas une lettre");
  assert.equal(toucheFrappee(frappe("F9", "F9"), null), "F9");
});

test("la combinaison porte ses modificateurs dans l'ordre que Rust relit", () => {
  const tout = { ctrlKey: true, altKey: true, shiftKey: true, metaKey: true };
  assert.equal(combinaisonFrappee(frappe("KeyQ", "a", { ctrlKey: true, altKey: true }), AZERTY), "Ctrl+Alt+KeyA");
  assert.equal(combinaisonFrappee(frappe("F9", "F9", tout), AZERTY), "Ctrl+Alt+Shift+Super+F9");
  assert.equal(combinaisonFrappee(frappe("ControlLeft", "Control", { ctrlKey: true }), AZERTY), null, "un modificateur seul n'est pas une frappe");
  assert.equal(combinaisonFrappee(frappe("KeyM", ",", { ctrlKey: true, altKey: true }), AZERTY), null);
});

test("les touches se lisent en français", () => {
  assert.deepEqual(touchesLisibles("Ctrl+Alt+Shift+Super+S"), ["Ctrl", "Alt", "Maj", "Win", "S"]);
});

const clic = (button, modificateurs = {}) => ({ button, ctrlKey: false, altKey: false, shiftKey: false, metaKey: false, ...modificateurs });

test("un clic de souris se lit en bouton 4, bouton 5 ou molette, avec ses modificateurs", () => {
  // PointerEvent.button : 0 gauche, 1 molette, 2 droit, 3 bouton 4 (précédent), 4 bouton 5 (suivant).
  assert.equal(combinaisonCliquee(clic(3, { altKey: true })), "Alt+Bouton4");
  assert.equal(combinaisonCliquee(clic(4, { ctrlKey: true, shiftKey: true })), "Ctrl+Shift+Bouton5");
  assert.equal(combinaisonCliquee(clic(1, { ctrlKey: true, altKey: true })), "Ctrl+Alt+Molette");
  // Win passe : c'est Rust qui la refuse, avec sa raison.
  assert.equal(combinaisonCliquee(clic(3, { metaKey: true, altKey: true })), "Alt+Super+Bouton4");
  assert.equal(combinaisonCliquee(clic(0, { altKey: true })), null, "clic gauche");
  assert.equal(combinaisonCliquee(clic(2, { altKey: true })), null, "clic droit");
  assert.equal(combinaisonCliquee(clic(5, { altKey: true })), null, "bouton inconnu");
});

test("les boutons se lisent en français", () => {
  assert.deepEqual(touchesLisibles("Alt+Bouton5"), ["Alt", "Bouton 5"]);
  assert.deepEqual(touchesLisibles("Ctrl+Shift+Molette"), ["Ctrl", "Maj", "Clic molette"]);
});
