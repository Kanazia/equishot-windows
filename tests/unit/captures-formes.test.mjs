// `zonePhysique` de la page est le miroir de `stockage::zone_physique` de Rust :
// Rust recadre l'original avec son calcul et refuse un rendu d'une autre taille.
// Les cas sont ceux des tests Rust, mêmes nombres, mêmes résultats attendus.
import assert from "node:assert/strict";
import { test } from "node:test";

import { zonePhysique } from "../../src/modules/captures/formes.ts";

const zone = (x, y, largeur, hauteur) => ({ x, y, largeur, hauteur });

test("zone CSS vers physique à 150 %", () => {
  assert.deepEqual(zonePhysique(zone(100, 40, 300, 200), 1.5, { largeur: 2880, hauteur: 1620 }), zone(150, 60, 450, 300));
});

test("les bords sont arrondis, pas la largeur", () => {
  // 10,4 à 20,4 CSS à 125 % : bords à 13 et 25,5, soit 13 et 26, largeur 13.
  assert.deepEqual(zonePhysique(zone(10.4, 0, 10, 8), 1.25, { largeur: 1000, hauteur: 1000 }), zone(13, 0, 13, 10));
});

test("zone bornée à l'image, zone vide refusée", () => {
  assert.deepEqual(zonePhysique(zone(900, 500, 400, 400), 1, { largeur: 1000, hauteur: 600 }), zone(900, 500, 100, 100));
  assert.equal(zonePhysique(zone(10, 10, 0.2, 50), 1, { largeur: 1000, hauteur: 600 }), null);
  assert.equal(zonePhysique(zone(10, 10, 50, 50), 0, { largeur: 1000, hauteur: 600 }), null);
  assert.equal(zonePhysique(zone(Number.NaN, 10, 50, 50), 1, { largeur: 1000, hauteur: 600 }), null);
});

// --- Le style des formes (ADR-005) ---------------------------------------------

import { PALETTE, luminance, numeroSuivant, simplifierTrace, styleDe } from "../../src/modules/captures/formes.ts";

test("une forme sans style garde le rouge et la taille moyenne d'avant", () => {
  assert.deepEqual(styleDe({ type: "fleche", x1: 0, y1: 0, x2: 9, y2: 9 }), { couleur: "#ff3b30", taille: 2 });
  assert.deepEqual(styleDe({ type: "ellipse", x: 0, y: 0, largeur: 5, hauteur: 5, couleur: "#007aff", taille: 3 }), { couleur: "#007aff", taille: 3 });
  // Un style abîmé retombe sur les défauts plutôt que de casser le dessin.
  assert.deepEqual(styleDe({ type: "ligne", x1: 0, y1: 0, x2: 1, y2: 1, couleur: "bleu", taille: 7 }), { couleur: "#ff3b30", taille: 2 });
});

test("la palette compte huit couleurs #rrggbb, le rouge d'origine en tête", () => {
  assert.equal(PALETTE.length, 8);
  assert.equal(PALETTE[0].couleur, "#ff3b30");
  for (const { couleur } of PALETTE) assert.match(couleur, /^#[0-9a-f]{6}$/);
});

test("l'étape suivante prend le plus grand numéro plus un", () => {
  assert.equal(numeroSuivant([]), 1);
  const etape = (numero) => ({ type: "etape", x: 0, y: 0, numero });
  assert.equal(numeroSuivant([etape(1), { type: "texte", x: 0, y: 0, texte: "9" }, etape(3)]), 4);
});

test("le tracé du crayon perd les points trop proches, garde le premier et le dernier", () => {
  assert.deepEqual(simplifierTrace([0, 0, 1, 0, 2, 0, 5, 0, 5, 1], 3), [0, 0, 5, 0, 5, 1]);
  assert.deepEqual(simplifierTrace([4, 4], 3), [4, 4]);
});

test("la luminance range le noir, le rouge et le jaune dans l'ordre", () => {
  assert.ok(luminance("#1c1c1e") < 0.1);
  assert.ok(luminance("#ff3b30") > 0.1 && luminance("#ff3b30") < 0.5);
  assert.ok(luminance("#ffcc00") > 0.5);
});

// --- Reprendre une forme (ADR-006) ---------------------------------------------

import { annuler, atteint, boite, deplacer, engager, formeSous, historique, poignees, restyler, retablir, tirer } from "../../src/modules/captures/formes.ts";

const rect = { type: "rectangle", x: 10, y: 10, largeur: 100, hauteur: 50 };
const fleche = { type: "fleche", x1: 0, y1: 0, x2: 100, y2: 0 };

test("la boîte d'une forme, quel que soit son type", () => {
  assert.deepEqual(boite(rect, 1), zone(10, 10, 100, 50));
  assert.deepEqual(boite({ type: "ligne", x1: 50, y1: 40, x2: 10, y2: 0 }, 1), zone(10, 0, 40, 40));
  assert.deepEqual(boite({ type: "crayon", points: [5, 9, 1, 3, 8, 2] }, 1), zone(1, 2, 7, 7));
  // Étape moyenne : rayon 13 à l'échelle 1, 26 à l'échelle 2.
  assert.deepEqual(boite({ type: "etape", x: 50, y: 50, numero: 1 }, 2), zone(24, 24, 52, 52));
  // Le texte se mesure par la fonction donnée.
  assert.deepEqual(boite({ type: "texte", x: 5, y: 6, texte: "abc" }, 1, (t, px) => t.length * px), zone(5, 6, 60, 20));
});

test("un trait s'atteint près de lui, pas loin", () => {
  assert.ok(atteint(fleche, { x: 50, y: 5 }, 1, 4));
  assert.ok(!atteint(fleche, { x: 50, y: 20 }, 1, 4));
  // Au-delà du bout, la distance compte depuis le bout.
  assert.ok(!atteint(fleche, { x: 120, y: 0 }, 1, 4));
  // Le surligneur est épais : 18 de large, on l'atteint à 12 du milieu.
  assert.ok(atteint({ ...fleche, type: "surligneur" }, { x: 50, y: 12 }, 1, 4));
});

test("un rectangle s'atteint sur son bord, son intérieur reste libre", () => {
  assert.ok(atteint(rect, { x: 60, y: 11 }, 1, 4));
  assert.ok(atteint(rect, { x: 60, y: 6 }, 1, 4));
  assert.ok(!atteint(rect, { x: 60, y: 35 }, 1, 4));
  assert.ok(!atteint({ ...rect, type: "projecteur" }, { x: 60, y: 35 }, 1, 4));
  assert.ok(atteint({ ...rect, type: "projecteur" }, { x: 109, y: 35 }, 1, 4));
  // La pixellisation, elle, s'atteint dedans.
  assert.ok(atteint({ ...rect, type: "pixellisation" }, { x: 60, y: 35 }, 1, 4));
  // La loupe aussi : elle remplit sa zone (ADR-013).
  assert.ok(atteint({ ...rect, type: "loupe" }, { x: 60, y: 35 }, 1, 4));
  assert.ok(!atteint({ ...rect, type: "loupe" }, { x: 200, y: 35 }, 1, 4));
  assert.deepEqual(poignees({ ...rect, type: "loupe" }).map((q) => q.id), ["no", "ne", "so", "se"]);
});

test("une ellipse s'atteint sur son contour", () => {
  const ellipse = { type: "ellipse", x: 0, y: 0, largeur: 200, hauteur: 100 };
  assert.ok(atteint(ellipse, { x: 200, y: 50 }, 1, 4));
  assert.ok(atteint(ellipse, { x: 100, y: 1 }, 1, 4));
  assert.ok(!atteint(ellipse, { x: 100, y: 50 }, 1, 4));
  assert.ok(!atteint(ellipse, { x: 5, y: 5 }, 1, 4));
});

test("crayon, étape et texte s'atteignent", () => {
  const crayon = { type: "crayon", points: [0, 0, 100, 0, 100, 100] };
  assert.ok(atteint(crayon, { x: 102, y: 50 }, 1, 4));
  assert.ok(!atteint(crayon, { x: 50, y: 50 }, 1, 4));
  assert.ok(atteint({ type: "crayon", points: [10, 10] }, { x: 12, y: 12 }, 1, 4));
  assert.ok(atteint({ type: "etape", x: 50, y: 50, numero: 1 }, { x: 60, y: 50 }, 1, 4));
  assert.ok(!atteint({ type: "etape", x: 50, y: 50, numero: 1 }, { x: 80, y: 50 }, 1, 4));
  assert.ok(atteint({ type: "texte", x: 0, y: 0, texte: "abcdef" }, { x: 50, y: 10 }, 1, 4));
});

test("la forme sous le point est la plus haute atteinte", () => {
  const formes = [rect, { ...rect, couleur: "#007aff" }, fleche];
  assert.equal(formeSous(formes, { x: 60, y: 10 }, 1, 4), 1);
  assert.equal(formeSous(formes, { x: 50, y: 0 }, 1, 4), 2);
  assert.equal(formeSous(formes, { x: 60, y: 35 }, 1, 4), null);
});

test("déplacer décale chaque type, sans toucher au style", () => {
  assert.deepEqual(deplacer({ ...rect, couleur: "#007aff" }, 5, -5), { ...rect, x: 15, y: 5, couleur: "#007aff" });
  assert.deepEqual(deplacer(fleche, 1, 2), { type: "fleche", x1: 1, y1: 2, x2: 101, y2: 2 });
  assert.deepEqual(deplacer({ type: "crayon", points: [0, 0, 4, 4] }, 1, 2), { type: "crayon", points: [1, 2, 5, 6] });
  assert.deepEqual(deplacer({ type: "etape", x: 1, y: 1, numero: 3 }, 1, 1), { type: "etape", x: 2, y: 2, numero: 3 });
});

test("les poignées : quatre coins, deux bouts, ou aucune", () => {
  assert.deepEqual(poignees(rect).map((p) => p.id), ["no", "ne", "so", "se"]);
  assert.deepEqual(poignees(rect).find((p) => p.id === "se"), { id: "se", x: 110, y: 60 });
  assert.deepEqual(poignees(fleche), [{ id: "a", x: 0, y: 0 }, { id: "b", x: 100, y: 0 }]);
  assert.deepEqual(poignees({ type: "etape", x: 1, y: 1, numero: 1 }), []);
  assert.deepEqual(poignees({ type: "crayon", points: [0, 0] }), []);
});

test("tirer une poignée garde le coin opposé, et retourne la boîte au besoin", () => {
  assert.deepEqual(tirer(rect, "se", { x: 150, y: 90 }), { ...rect, largeur: 140, hauteur: 80 });
  // Le coin bas droit tiré au-delà du haut gauche : la boîte se retourne, normalisée.
  assert.deepEqual(tirer(rect, "se", { x: 0, y: 0 }), { ...rect, x: 0, y: 0, largeur: 10, hauteur: 10 });
  assert.deepEqual(tirer(rect, "no", { x: 20, y: 20 }), { ...rect, x: 20, y: 20, largeur: 90, hauteur: 40 });
  assert.deepEqual(tirer(fleche, "b", { x: 7, y: 8 }), { ...fleche, x2: 7, y2: 8 });
  const etape = { type: "etape", x: 1, y: 1, numero: 1 };
  assert.equal(tirer(etape, "se", { x: 9, y: 9 }), etape);
});

test("restyler pose la couleur et la taille", () => {
  assert.deepEqual(restyler(fleche, { couleur: "#007aff", taille: 3 }), { ...fleche, couleur: "#007aff", taille: 3 });
});

test("l'historique annule et rétablit, une nouvelle action efface l'avenir", () => {
  let h = historique([]);
  h = engager(h, [rect]);
  h = engager(h, [rect, fleche]);
  h = annuler(h);
  assert.deepEqual(h.present, [rect]);
  h = annuler(h);
  assert.deepEqual(h.present, []);
  assert.equal(annuler(h), h, "rien à annuler : rien ne change");
  h = retablir(h);
  assert.deepEqual(h.present, [rect]);
  h = engager(h, [fleche]);
  assert.equal(retablir(h), h, "après une action nouvelle, plus rien à rétablir");
  assert.deepEqual(annuler(h).present, [rect]);
});

test("l'historique garde cent pas au plus", () => {
  let h = historique([]);
  for (let i = 1; i <= 150; i++) h = engager(h, Array(i).fill(rect));
  assert.equal(h.passe.length, 100);
  for (let i = 0; i < 200; i++) h = annuler(h);
  assert.equal(h.present.length, 50);
});

// --- Le recadrage (ADR-007) ------------------------------------------------------

import { glisserRecadrage, recadrageNet } from "../../src/modules/captures/formes.ts";

const image = { largeur: 400, hauteur: 300 };

test("un recadrage se normalise, s'arrondit et se borne à l'image", () => {
  assert.deepEqual(recadrageNet(zone(10.4, 20.6, 100, 50), image, 8), zone(10, 21, 100, 50));
  assert.deepEqual(recadrageNet(zone(110, 70, -100, -50), image, 8), zone(10, 20, 100, 50), "tracé vers le haut à gauche");
  assert.deepEqual(recadrageNet(zone(-20, 250, 100, 100), image, 8), zone(0, 250, 80, 50), "rogné aux bords de l'image");
  assert.equal(recadrageNet(zone(10, 10, 5, 100), image, 8), null, "trop étroit");
  assert.equal(recadrageNet(zone(500, 10, 50, 50), image, 8), null, "hors de l'image");
  assert.equal(recadrageNet(zone(Number.NaN, 10, 50, 50), image, 8), null);
});

test("un recadrage glissé garde sa taille et reste dans l'image", () => {
  assert.deepEqual(glisserRecadrage(zone(10, 20, 100, 50), 30.4, -5, image), zone(40, 15, 100, 50));
  assert.deepEqual(glisserRecadrage(zone(10, 20, 100, 50), -50, -50, image), zone(0, 0, 100, 50), "arrêté au coin haut gauche");
  assert.deepEqual(glisserRecadrage(zone(10, 20, 100, 50), 999, 999, image), zone(300, 250, 100, 50), "arrêté au coin bas droit");
});

// --- La fenêtre au clic (ADR-008) ---------------------------------------------------

import { fenetreSous, versCss } from "../../src/modules/captures/formes.ts";

test("une fenêtre en pixels physiques passe en pixels CSS de la sélection", () => {
  assert.deepEqual(versCss(zone(150, 60, 450, 300), 1.5), zone(100, 40, 300, 200));
});

test("la fenêtre sous le point est la plus haute, rien hors de toutes", () => {
  // Rangées comme Windows les donne : la plus haute d'abord.
  const fenetres = [zone(100, 100, 200, 100), zone(0, 0, 1000, 600), zone(150, 120, 50, 50)];
  assert.equal(fenetreSous(fenetres, { x: 160, y: 130 }), 0, "dans les trois : la première");
  assert.equal(fenetreSous(fenetres, { x: 50, y: 50 }), 1);
  assert.equal(fenetreSous(fenetres, { x: 300, y: 150 }), 1, "le bord droit n'est plus dans la fenêtre");
  assert.equal(fenetreSous(fenetres, { x: 100, y: 100 }), 0, "le bord gauche y est");
  assert.equal(fenetreSous(fenetres, { x: 1200, y: 50 }), null);
});

// `tailleRendu` est le miroir de `Capture::taille_rendu` (ADR-012) : mêmes nombres que le test Rust.
import { tailleRendu } from "../../src/modules/captures/formes.ts";

test("taille du rendu : le cadre plus deux marges du fond", () => {
  const fond = { couleurs: ["#000000", "#000000"], marge: 10, arrondi: 0, ombre: false };
  const capture = { largeur: 200, hauteur: 100, recadrage: null, fond: null };
  assert.deepEqual(tailleRendu(capture), { largeur: 200, hauteur: 100 });
  const cadre = { x: 0, y: 0, largeur: 50, hauteur: 40 };
  assert.deepEqual(tailleRendu({ ...capture, recadrage: cadre }), { largeur: 50, hauteur: 40 });
  assert.deepEqual(tailleRendu({ ...capture, recadrage: cadre, fond }), { largeur: 60, hauteur: 50 }, "5 px : 10 % du plus grand côté du cadre");
  assert.deepEqual(tailleRendu({ ...capture, fond: { ...fond, marge: 8 } }), { largeur: 232, hauteur: 132 }, "16 px sur l'original entier");
});

// La pipette (ADR-013) : le code que Rust revalide, `#rrggbb` en minuscules.
import { couleurHex } from "../../src/modules/captures/formes.ts";

test("la pipette écrit la couleur en #rrggbb", () => {
  assert.equal(couleurHex([0, 0, 0]), "#000000");
  assert.equal(couleurHex([255, 15, 160]), "#ff0fa0");
  assert.equal(couleurHex(new Uint8ClampedArray([1, 2, 3, 255])), "#010203", "l'alpha est ignoré");
});

// Le tampon (ADR-014) : un symbole posé en son centre, de la famille de l'étape.
import { SYMBOLES } from "../../src/modules/captures/formes.ts";

test("un tampon : sa boîte, son atteinte, son déplacement, sans poignée", () => {
  const tampon = { type: "tampon", x: 50, y: 50, symbole: "✅" };
  // Tampon moyen : 36 de côté à l'échelle 1, 72 à l'échelle 2 ; grand : 36 x 1,6.
  assert.deepEqual(boite(tampon, 2), zone(14, 14, 72, 72));
  assert.deepEqual(boite({ ...tampon, taille: 3 }, 1), zone(50 - 28.8, 50 - 28.8, 57.6, 57.6));
  assert.ok(atteint(tampon, { x: 66, y: 66 }, 1, 4), "dans un coin de la boîte");
  assert.ok(!atteint(tampon, { x: 80, y: 50 }, 1, 4));
  assert.deepEqual(deplacer(tampon, 1, -1), { ...tampon, x: 51, y: 49 });
  assert.deepEqual(poignees(tampon), []);
});

test("douze symboles de tampon, tous différents et courts", () => {
  assert.equal(SYMBOLES.length, 12);
  assert.equal(new Set(SYMBOLES.map((s) => s.symbole)).size, 12);
  // Miroir de `valider_forme` : de 1 à 16 caractères.
  for (const s of SYMBOLES) assert.ok([...s.symbole].length >= 1 && [...s.symbole].length <= 16 && s.nom, s.symbole);
});
