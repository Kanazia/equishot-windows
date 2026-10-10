// L'orientation d'une capture (ADR-018) : huit états, trois gestes, les formes et le
// cadre transformés au clic, l'image orientée au dessin. La table des transitions se
// rejoue par calcul sur une grille 3 x 2, jamais par elle-même. Les nombres des tailles
// sont ceux des tests Rust (`stockage.rs`, `fond_taille_du_rendu_cadre_plus_deux_marges`).
import assert from "node:assert/strict";
import { test } from "node:test";

import {
  annuler,
  boite,
  composer,
  matriceOrientation,
  orienterForme,
  orienterFormes,
  orienterHistorique,
  orienterZone,
  remanierHistorique,
  tailleOrientee,
  tailleRendu,
} from "../../src/modules/captures/formes.ts";

// --- Une grille 3 x 2 étiquetée et les trois gestes faits sur des tableaux.
const grille = [
  [0, 1, 2],
  [3, 4, 5],
];
/** Un quart de tour horaire : la colonne x devient la ligne x, lue de bas en haut. */
const R = (g) => {
  const h = g.length;
  const l = g[0].length;
  const r = Array.from({ length: l }, () => Array(h).fill(0));
  for (let y = 0; y < h; y++) for (let x = 0; x < l; x++) r[x][h - 1 - y] = g[y][x];
  return r;
};
const M = (g) => g.map((ligne) => [...ligne].reverse());
const V = (g) => [...g].reverse();
const GESTE = { pivoter: R, horizontal: M, vertical: V };
const BOUTONS = ["pivoter", "horizontal", "vertical"];
/** L'affiché de l'état `o` : le miroir d'abord, puis `q` quarts de tour horaires. */
function affiche(o) {
  let g = o >= 4 ? M(grille) : grille;
  for (let i = 0; i < o % 4; i++) g = R(g);
  return g;
}
const cle = (g) => `${g.length}:${g.flat().join()}`;

test("les huit états sont distincts et chaque geste donne l'état de la table", () => {
  assert.equal(new Set(Array.from({ length: 8 }, (_, o) => cle(affiche(o)))).size, 8);
  const table = {};
  for (const b of BOUTONS) {
    table[b] = [];
    for (let o = 0; o < 8; o++) {
      const apres = composer(o, b);
      assert.deepEqual(GESTE[b](affiche(o)), affiche(apres), `${b} depuis ${o}`);
      table[b].push(`${o}>${apres}`);
    }
  }
  // Mutant « horizontal garde q » : 1>5 au lieu de 1>7 ; M(R(g)) n'est pas R(M(g)).
  assert.equal(table.pivoter.join(" "), "0>1 1>2 2>3 3>0 4>5 5>6 6>7 7>4");
  assert.equal(table.horizontal.join(" "), "0>4 1>7 2>6 3>5 4>0 5>3 6>2 7>1");
  assert.equal(table.vertical.join(" "), "0>6 1>5 2>4 3>7 4>2 5>1 6>0 7>3");
});

// --- Une forme de chaque type, asymétrique, dans une image 100 x 60.
const TAILLE = { largeur: 100, hauteur: 60 };
const FORMES = [
  { type: "fleche", x1: 10, y1: 20, x2: 40, y2: 50 },
  { type: "ligne", x1: 10, y1: 20, x2: 40, y2: 50, couleur: "#007aff", taille: 1 },
  { type: "surligneur", x1: 10, y1: 20, x2: 40, y2: 50 },
  { type: "crayon", points: [0, 0, 10, 5, 20, 0] },
  { type: "rectangle", x: 10, y: 5, largeur: 30, hauteur: 20 },
  { type: "ellipse", x: 10, y: 5, largeur: 30, hauteur: 20 },
  { type: "pixellisation", x: 10, y: 5, largeur: 30, hauteur: 20 },
  { type: "projecteur", x: 10, y: 5, largeur: 30, hauteur: 20 },
  { type: "loupe", x: 10, y: 5, largeur: 30, hauteur: 20 },
  { type: "texte", x: 20, y: 5, texte: "abcd" },
  { type: "etape", x: 10, y: 5, numero: 3 },
  { type: "tampon", x: 10, y: 5, symbole: "OK" },
  // Hors de l'image : transformée comme les autres, jamais bornée ni perdue.
  { type: "rectangle", x: -20, y: 50, largeur: 30, hauteur: 40 },
];
/** La mesure injectée : un caractère par pixel de corps. */
const mesure = (texte, px) => texte.length * px;
/** Arrondi à 1e-9 : les flottants de l'estimation du texte ne font pas tomber une égalité vraie. */
const net = (v) => JSON.parse(JSON.stringify(v, (_, x) => (typeof x === "number" ? Math.round(x * 1e9) / 1e9 : x)));

test("aller-retour : quatre pivots, deux horizontaux, deux verticaux rendent l'état et les formes", () => {
  for (let o = 0; o < 8; o++) {
    let etat = o;
    let formes = FORMES;
    let taille = TAILLE;
    for (let i = 0; i < 4; i++) {
      formes = orienterFormes(formes, "pivoter", taille, 1, mesure);
      taille = tailleOrientee(taille, 1);
      etat = composer(etat, "pivoter");
    }
    assert.equal(etat, o);
    assert.deepEqual(taille, TAILLE);
    assert.deepEqual(net(formes), net(FORMES), `quatre pivots depuis ${o}`);
    for (const b of ["horizontal", "vertical"]) {
      assert.equal(composer(composer(o, b), b), o, `${b} deux fois depuis ${o}`);
      assert.deepEqual(net(orienterFormes(orienterFormes(FORMES, b, TAILLE, 1, mesure), b, TAILLE, 1, mesure)), net(FORMES), b);
    }
  }
});

test("chaque type, chaque geste : valeurs écrites à la main", () => {
  const apres = (forme, b) => orienterForme(forme, b, TAILLE, 1, mesure);
  // Les bouts transformés, la pointe (x2, y2) reste la pointe : la flèche garde son sens.
  assert.deepEqual(apres(FORMES[0], "pivoter"), { type: "fleche", x1: 40, y1: 10, x2: 10, y2: 40 });
  assert.deepEqual(apres(FORMES[0], "horizontal"), { type: "fleche", x1: 90, y1: 20, x2: 60, y2: 50 });
  assert.deepEqual(apres(FORMES[0], "vertical"), { type: "fleche", x1: 10, y1: 40, x2: 40, y2: 10 });
  assert.deepEqual(apres(FORMES[1], "pivoter"), { type: "ligne", x1: 40, y1: 10, x2: 10, y2: 40, couleur: "#007aff", taille: 1 }, "le style suit");
  assert.deepEqual(apres(FORMES[2], "vertical"), { type: "surligneur", x1: 10, y1: 40, x2: 40, y2: 10 });
  assert.deepEqual(apres(FORMES[3], "pivoter"), { type: "crayon", points: [60, 0, 55, 10, 60, 20] });
  assert.deepEqual(apres(FORMES[3], "horizontal"), { type: "crayon", points: [100, 0, 90, 5, 80, 0] });
  assert.deepEqual(apres(FORMES[3], "vertical"), { type: "crayon", points: [0, 60, 10, 55, 20, 60] });
  for (const type of ["rectangle", "ellipse", "pixellisation", "projecteur", "loupe"]) {
    const boiteDe = { type, x: 10, y: 5, largeur: 30, hauteur: 20 };
    assert.deepEqual(apres(boiteDe, "pivoter"), { type, x: 35, y: 10, largeur: 20, hauteur: 30 }, `${type} pivoté : largeur et hauteur échangées`);
    assert.deepEqual(apres(boiteDe, "horizontal"), { type, x: 60, y: 5, largeur: 30, hauteur: 20 }, type);
    assert.deepEqual(apres(boiteDe, "vertical"), { type, x: 10, y: 35, largeur: 30, hauteur: 20 }, type);
  }
  assert.deepEqual(apres(FORMES[10], "pivoter"), { type: "etape", x: 55, y: 10, numero: 3 }, "le centre suit, le numéro reste");
  assert.deepEqual(apres(FORMES[10], "horizontal"), { type: "etape", x: 90, y: 5, numero: 3 });
  assert.deepEqual(apres(FORMES[11], "vertical"), { type: "tampon", x: 10, y: 55, symbole: "OK" }, "le centre suit, le symbole reste");
  // Hors de l'image : (-20, 50) et (10, 90) pivotés en (10, -20) et (-30, 10).
  assert.deepEqual(apres(FORMES[12], "pivoter"), { type: "rectangle", x: -30, y: -20, largeur: 40, hauteur: 30 });
  for (const forme of FORMES) {
    for (const b of BOUTONS) {
      const f = apres(forme, b);
      if ("largeur" in f) assert.ok(f.largeur >= 0 && f.hauteur >= 0, `${forme.type} ${b} : boîte positive`);
    }
  }
});

test("le texte suit par le centre de sa boîte mesurée, sans tourner ni s'étirer", () => {
  // x 20, y 5, « abcd » en taille moyenne : px 20, boîte 80 x 20, centre (60, 15).
  const texte = FORMES[9];
  const pivote = orienterForme(texte, "pivoter", TAILLE, 1, mesure);
  assert.deepEqual(pivote, { type: "texte", x: 5, y: 50, texte: "abcd" });
  assert.deepEqual(boite(pivote, 1, mesure), { x: 5, y: 50, largeur: 80, hauteur: 20 }, "la boîte garde l x px, non échangés");
  for (const b of ["horizontal", "vertical"]) {
    assert.deepEqual(
      boite(orienterForme(texte, b, TAILLE, 1, mesure), 1, mesure),
      orienterZone(boite(texte, 1, mesure), b, TAILLE),
      `${b} : la boîte est exactement là où la boîte retournée serait`,
    );
  }
  // Sans mesure : l'estimation (4 x 20 x 0,6 = 48), même règle du centre.
  assert.deepEqual(net(orienterForme(texte, "horizontal", TAILLE, 1)), { type: "texte", x: 32, y: 5, texte: "abcd" });
  // Mutant « texte traité comme une boîte » : pivoté, l'ancrage serait (35, 20) et la boîte 20 x 80.
});

test("l'historique entier est transformé, et Ctrl+Z rend des formes transformées", () => {
  const [A, B, C] = [FORMES[4], FORMES[10], FORMES[0]];
  const t = (f) => orienterForme(f, "pivoter", TAILLE, 1, mesure);
  const h = { passe: [[], [A]], present: [A, B], futur: [[A, B, C]] };
  const transforme = orienterHistorique(h, "pivoter", TAILLE, 1, mesure);
  assert.deepEqual(transforme, { passe: [[], [t(A)]], present: [t(A), t(B)], futur: [[t(A), t(B), t(C)]] });
  assert.deepEqual(annuler(transforme).present, [t(A)], "Ctrl+Z après un pivot rend A à sa place pivotée");
  assert.deepEqual(remanierHistorique(h, (formes) => formes.slice(0, 1)), { passe: [[], [A]], present: [A], futur: [[A]] });
});

test("le recadrage se transforme comme un rectangle", () => {
  const z = { x: 10, y: 5, largeur: 30, hauteur: 20 };
  assert.deepEqual(orienterZone(z, "pivoter", TAILLE), { x: 35, y: 10, largeur: 20, hauteur: 30 });
  assert.deepEqual(orienterZone(z, "horizontal", TAILLE), { x: 60, y: 5, largeur: 30, hauteur: 20 });
  assert.deepEqual(orienterZone(z, "vertical", TAILLE), { x: 10, y: 35, largeur: 30, hauteur: 20 });
});

test("tailleOrientee et tailleRendu : les nombres des tests Rust, plus un pivot", () => {
  const original = { largeur: 200, hauteur: 100 };
  assert.deepEqual(tailleOrientee(original, 0), { largeur: 200, hauteur: 100 });
  assert.deepEqual(tailleOrientee(original, 1), { largeur: 100, hauteur: 200 });
  assert.deepEqual(tailleOrientee(original, 2), { largeur: 200, hauteur: 100 });
  assert.deepEqual(tailleOrientee(original, 7), { largeur: 100, hauteur: 200 });
  const fond = { couleurs: ["#000000", "#000000"], marge: 10, arrondi: 0, ombre: false };
  const cadre = { x: 0, y: 0, largeur: 50, hauteur: 40 };
  assert.deepEqual(tailleRendu({ ...original, orientation: 0, recadrage: null, fond: null }), { largeur: 200, hauteur: 100 });
  assert.deepEqual(tailleRendu({ ...original, orientation: 0, recadrage: cadre, fond: null }), { largeur: 50, hauteur: 40 });
  assert.deepEqual(tailleRendu({ ...original, orientation: 0, recadrage: cadre, fond }), { largeur: 60, hauteur: 50 });
  assert.deepEqual(tailleRendu({ ...original, orientation: 1, recadrage: null, fond: null }), { largeur: 100, hauteur: 200 }, "un pivot échange les côtés");
  assert.deepEqual(tailleRendu({ ...original, orientation: 1, recadrage: null, fond }), { largeur: 140, hauteur: 240 }, "20 px de marge : 10 % du plus grand côté orienté");
  assert.deepEqual(tailleRendu({ ...original, orientation: 1, recadrage: cadre, fond }), { largeur: 60, hauteur: 50 }, "le cadre, déjà dans l'espace orienté, ne change pas");
});

test("la matrice d'affichage envoie chaque pixel de l'original à sa place, huit états", () => {
  const applique = (m, x, y) => [m[0] * x + m[2] * y + m[4], m[1] * x + m[3] * y + m[5]];
  for (let o = 0; o < 8; o++) {
    const m = matriceOrientation(o, 3, 2);
    const a = affiche(o);
    const t = tailleOrientee({ largeur: 3, hauteur: 2 }, o);
    assert.equal(a[0].length, t.largeur);
    assert.equal(a.length, t.hauteur);
    for (let v = 0; v < 2; v++) {
      for (let u = 0; u < 3; u++) {
        const [x, y] = applique(m, u + 0.5, v + 0.5);
        assert.equal(a[Math.floor(y)][Math.floor(x)], grille[v][u], `état ${o}, pixel ${u},${v}`);
      }
    }
  }
  assert.deepEqual(matriceOrientation(0, 3, 2), [1, 0, 0, 1, 0, 0]);
  assert.deepEqual(matriceOrientation(1, 3, 2), [0, 1, -1, 0, 2, 0], "un quart horaire : (x, y) devient (h - y, x)");
});
