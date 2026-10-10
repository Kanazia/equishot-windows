/**
 * Formes d'annotation d'une capture (ADR-075) : des données, en pixels
 * physiques relatifs à la zone capturée. Un seul moteur de dessin, le canevas
 * 2D, sert à l'édition et à l'aplatissement : ce qu'on voit est ce qui est
 * enregistré. Chaque forme porte sa couleur et sa taille (ADR-005) ; sans
 * elles, le rouge et la taille moyenne d'avant.
 */

/** Rouge d'annotation d'origine : la couleur d'une forme qui n'en dit pas. */
export const COULEUR_ANNOTATION = "#ff3b30";

// ponytail: huit couleurs, pas de couleur libre (le sélecteur de Windows est modal, ADR-005).
export const PALETTE = [
  { couleur: "#ff3b30", nom: "Rouge" },
  { couleur: "#ff9500", nom: "Orange" },
  { couleur: "#ffcc00", nom: "Jaune" },
  { couleur: "#34c759", nom: "Vert" },
  { couleur: "#007aff", nom: "Bleu" },
  { couleur: "#af52de", nom: "Violet" },
  { couleur: "#1c1c1e", nom: "Noir" },
  { couleur: "#ffffff", nom: "Blanc" },
] as const;

export type Taille = 1 | 2 | 3;

/** Multiplicateur des traits, du texte et des blocs pour chaque taille. */
export const TAILLES: Record<Taille, { facteur: number; nom: string }> = {
  1: { facteur: 0.6, nom: "Fine" },
  2: { facteur: 1, nom: "Moyenne" },
  3: { facteur: 1.6, nom: "Grande" },
};

export type Outil = "fleche" | "rectangle" | "ellipse" | "ligne" | "crayon" | "surligneur" | "texte" | "etape" | "pixellisation" | "projecteur" | "loupe" | "tampon";

// ponytail: douze symboles fixes, pas de sélecteur d'émojis complet (ADR-014).
export const SYMBOLES = [
  { symbole: "✅", nom: "Validé" },
  { symbole: "❌", nom: "Refusé" },
  { symbole: "⚠️", nom: "Attention" },
  { symbole: "❓", nom: "Question" },
  { symbole: "❗", nom: "Important" },
  { symbole: "👍", nom: "D'accord" },
  { symbole: "👎", nom: "Pas d'accord" },
  { symbole: "⭐", nom: "Étoile" },
  { symbole: "❤️", nom: "Cœur" },
  { symbole: "🔥", nom: "Urgent" },
  { symbole: "👀", nom: "À regarder" },
  { symbole: "💡", nom: "Idée" },
] as const;

export interface Style {
  couleur?: string;
  taille?: Taille;
}

export type Forme = Style &
  (
    | { type: "fleche" | "ligne" | "surligneur"; x1: number; y1: number; x2: number; y2: number }
    | { type: "rectangle" | "ellipse" | "pixellisation" | "projecteur" | "loupe"; x: number; y: number; largeur: number; hauteur: number }
    | { type: "texte"; x: number; y: number; texte: string }
    | { type: "etape"; x: number; y: number; numero: number }
    /** Le symbole centré sur x, y (ADR-014). */
    | { type: "tampon"; x: number; y: number; symbole: string }
    /** Points à plat : x0, y0, x1, y1… */
    | { type: "crayon"; points: number[] }
  );

export interface Zone {
  x: number;
  y: number;
  largeur: number;
  hauteur: number;
}

/** Le fond d'une capture (ADR-012), miroir de `stockage::Fond`. */
export interface Fond {
  /** Dégradé en diagonale ; deux fois la même couleur pour un fond uni. */
  couleurs: [string, string];
  /** En pourcents du plus grand côté de l'image montrée, 0 à 25. */
  marge: number;
  /** Rayon des coins en pourcents du plus petit côté, 0 à 10. */
  arrondi: number;
  ombre: boolean;
}

/** La marge en pixels : miroir de `stockage::marge_px`. */
export function margePx(taille: { largeur: number; hauteur: number }, fond: Fond): number {
  return Math.round((Math.max(taille.largeur, taille.hauteur) * fond.marge) / 100);
}

/** La taille du rendu, le cadre plus deux marges : miroir de `Capture::taille_rendu`. */
export function tailleRendu(capture: { largeur: number; hauteur: number; orientation: number; recadrage: Zone | null; fond: Fond | null }) {
  // Le cadre est dans l'espace orienté (ADR-018) ; sans cadre, l'original orienté.
  const cadre = capture.recadrage ?? tailleOrientee(capture, capture.orientation);
  const m = capture.fond ? margePx(cadre, capture.fond) : 0;
  return { largeur: cadre.largeur + 2 * m, hauteur: cadre.hauteur + 2 * m };
}

/** Taille du texte d'annotation moyen, en pixels CSS : multipliée par l'échelle de l'écran. */
export const TAILLE_TEXTE = 20;

/** La police des textes et des chiffres d'annotation, à `px` pixels. */
export const police = (px: number) => `600 ${px}px Inter, "Segoe UI", sans-serif`;

const COULEUR_VALIDE = /^#[0-9a-fA-F]{6}$/;

/** Le style d'une forme, ses défauts posés. Un style abîmé retombe sur les défauts. */
export function styleDe(forme: Style): Required<Style> {
  const couleur = typeof forme.couleur === "string" && COULEUR_VALIDE.test(forme.couleur) ? forme.couleur : COULEUR_ANNOTATION;
  const taille = forme.taille === 1 || forme.taille === 3 ? forme.taille : 2;
  return { couleur, taille };
}

/** Le numéro de la prochaine étape : le plus grand posé, plus un. */
export function numeroSuivant(formes: readonly Forme[]): number {
  return formes.reduce((max, f) => (f.type === "etape" ? Math.max(max, f.numero) : max), 0) + 1;
}

/** Retire du tracé les points à moins de `ecart` du dernier gardé ; le dernier point reste. */
export function simplifierTrace(points: readonly number[], ecart: number): number[] {
  const garde: number[] = [];
  for (let i = 0; i + 1 < points.length; i += 2) {
    const n = garde.length;
    const dernier = i + 2 >= points.length;
    if (n === 0 || dernier || Math.hypot(points[i] - garde[n - 2], points[i + 1] - garde[n - 1]) >= ecart) {
      garde.push(points[i], points[i + 1]);
    }
  }
  return garde;
}

/** Luminance relative (WCAG) d'une couleur `#rrggbb`, de 0 (noir) à 1 (blanc). */
export function luminance(couleur: string): number {
  const canal = (i: number) => {
    const c = parseInt(couleur.slice(i, i + 2), 16) / 255;
    return c <= 0.03928 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * canal(1) + 0.7152 * canal(3) + 0.0722 * canal(5);
}

/** La couleur d'un pixel (rouge, vert, bleu, l'alpha ignoré) en `#rrggbb` : la pipette (ADR-013). */
export function couleurHex(rvb: ArrayLike<number>): string {
  return `#${[rvb[0], rvb[1], rvb[2]].map((c) => c.toString(16).padStart(2, "0")).join("")}`;
}

// --- Reprendre une forme (ADR-006) ----------------------------------------------

/** Largeur d'un texte de `px` pixels : le canevas dans la page, une estimation sans lui. */
export type Mesure = (texte: string, px: number) => number;
const MESURE_ESTIMEE: Mesure = (texte, px) => texte.length * px * 0.6;

/** Épaisseur de base du trait de chaque forme, avant la taille et l'échelle. */
const TRAIT: Record<Forme["type"], number> = {
  fleche: 5,
  ligne: 5,
  surligneur: 18,
  rectangle: 4,
  ellipse: 4,
  crayon: 4,
  pixellisation: 0,
  projecteur: 0,
  loupe: 4,
  texte: 0,
  etape: 0,
  tampon: 0,
};

const rayonEtape = (forme: Style, echelle: number) => 13 * TAILLES[styleDe(forme).taille].facteur * echelle;
const coteTampon = (forme: Style, echelle: number) => 36 * TAILLES[styleDe(forme).taille].facteur * echelle;
const pxTexte = (forme: Style, echelle: number) => TAILLE_TEXTE * TAILLES[styleDe(forme).taille].facteur * echelle;

function entre(x0: number, y0: number, x1: number, y1: number): Zone {
  return { x: Math.min(x0, x1), y: Math.min(y0, y1), largeur: Math.abs(x1 - x0), hauteur: Math.abs(y1 - y0) };
}

/** Le rectangle qui contient la forme, sans l'épaisseur de son trait. */
export function boite(forme: Forme, echelle: number, mesure: Mesure = MESURE_ESTIMEE): Zone {
  switch (forme.type) {
    case "fleche":
    case "ligne":
    case "surligneur":
      return entre(forme.x1, forme.y1, forme.x2, forme.y2);
    case "crayon": {
      const xs = forme.points.filter((_, i) => i % 2 === 0);
      const ys = forme.points.filter((_, i) => i % 2 === 1);
      return entre(Math.min(...xs), Math.min(...ys), Math.max(...xs), Math.max(...ys));
    }
    case "etape": {
      const r = rayonEtape(forme, echelle);
      return { x: forme.x - r, y: forme.y - r, largeur: 2 * r, hauteur: 2 * r };
    }
    case "tampon": {
      const c = coteTampon(forme, echelle);
      return { x: forme.x - c / 2, y: forme.y - c / 2, largeur: c, hauteur: c };
    }
    case "texte": {
      const px = pxTexte(forme, echelle);
      return { x: forme.x, y: forme.y, largeur: mesure(forme.texte, px), hauteur: px };
    }
    default:
      return entre(forme.x, forme.y, forme.x + forme.largeur, forme.y + forme.hauteur);
  }
}

function distanceSegment(p: Point, x1: number, y1: number, x2: number, y2: number): number {
  const dx = x2 - x1;
  const dy = y2 - y1;
  const l2 = dx * dx + dy * dy;
  const t = l2 === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - x1) * dx + (p.y - y1) * dy) / l2));
  return Math.hypot(p.x - (x1 + t * dx), p.y - (y1 + t * dy));
}

function dedans(p: Point, z: Zone, marge: number): boolean {
  return p.x >= z.x - marge && p.x <= z.x + z.largeur + marge && p.y >= z.y - marge && p.y <= z.y + z.hauteur + marge;
}

/** Distance au contour d'un rectangle, de l'intérieur comme de l'extérieur. */
function distanceBord(p: Point, z: Zone): number {
  const dx = Math.max(z.x - p.x, 0, p.x - z.x - z.largeur);
  const dy = Math.max(z.y - p.y, 0, p.y - z.y - z.hauteur);
  if (dx > 0 || dy > 0) return Math.hypot(dx, dy);
  return Math.min(p.x - z.x, z.x + z.largeur - p.x, p.y - z.y, z.y + z.hauteur - p.y);
}

/**
 * Le point touche-t-il la forme ? Près du trait pour ce qui a un trait ou un
 * bord (l'intérieur d'un rectangle reste libre pour y tracer), n'importe où
 * dedans pour l'étape, le texte, la pixellisation et la loupe. `marge` en pixels du canevas.
 */
export function atteint(forme: Forme, p: Point, echelle: number, marge: number, mesure: Mesure = MESURE_ESTIMEE): boolean {
  const tolerance = marge + (TRAIT[forme.type] * TAILLES[styleDe(forme).taille].facteur * echelle) / 2;
  switch (forme.type) {
    case "fleche":
    case "ligne":
    case "surligneur":
      return distanceSegment(p, forme.x1, forme.y1, forme.x2, forme.y2) <= tolerance;
    case "crayon": {
      const pts = forme.points;
      if (pts.length === 2) return Math.hypot(p.x - pts[0], p.y - pts[1]) <= tolerance;
      for (let i = 0; i + 3 < pts.length; i += 2) {
        if (distanceSegment(p, pts[i], pts[i + 1], pts[i + 2], pts[i + 3]) <= tolerance) return true;
      }
      return false;
    }
    case "rectangle":
    case "projecteur":
      return distanceBord(p, boite(forme, echelle)) <= tolerance;
    case "ellipse": {
      const z = boite(forme, echelle);
      const rx = z.largeur / 2;
      const ry = z.hauteur / 2;
      if (rx < 1 || ry < 1) return distanceBord(p, z) <= tolerance;
      // ponytail: distance radiale ramenée au petit rayon, juste pour un clic ; exacte au cercle.
      const radial = Math.hypot((p.x - z.x - rx) / rx, (p.y - z.y - ry) / ry);
      return Math.abs(radial - 1) * Math.min(rx, ry) <= tolerance;
    }
    case "etape":
      return Math.hypot(p.x - forme.x, p.y - forme.y) <= rayonEtape(forme, echelle) + marge;
    default:
      return dedans(p, boite(forme, echelle, mesure), marge);
  }
}

/** L'indice de la forme la plus haute sous le point, ou `null`. */
export function formeSous(formes: readonly Forme[], p: Point, echelle: number, marge: number, mesure?: Mesure): number | null {
  for (let i = formes.length - 1; i >= 0; i--) if (atteint(formes[i], p, echelle, marge, mesure)) return i;
  return null;
}

export function deplacer(forme: Forme, dx: number, dy: number): Forme {
  switch (forme.type) {
    case "fleche":
    case "ligne":
    case "surligneur":
      return { ...forme, x1: forme.x1 + dx, y1: forme.y1 + dy, x2: forme.x2 + dx, y2: forme.y2 + dy };
    case "crayon":
      return { ...forme, points: forme.points.map((v, i) => v + (i % 2 === 0 ? dx : dy)) };
    default:
      return { ...forme, x: forme.x + dx, y: forme.y + dy };
  }
}

/** Les coins d'une boîte, les bouts d'un trait. */
export type Poignee = "no" | "ne" | "so" | "se" | "a" | "b";

export function poignees(forme: Forme): { id: Poignee; x: number; y: number }[] {
  switch (forme.type) {
    case "fleche":
    case "ligne":
    case "surligneur":
      return [
        { id: "a", x: forme.x1, y: forme.y1 },
        { id: "b", x: forme.x2, y: forme.y2 },
      ];
    case "rectangle":
    case "ellipse":
    case "pixellisation":
    case "projecteur":
    case "loupe": {
      const { x, y, largeur, hauteur } = forme;
      return [
        { id: "no", x, y },
        { id: "ne", x: x + largeur, y },
        { id: "so", x, y: y + hauteur },
        { id: "se", x: x + largeur, y: y + hauteur },
      ];
    }
    default:
      return [];
  }
}

/** La forme dont la poignée est tirée jusqu'au point ; une boîte garde son coin opposé. */
export function tirer(forme: Forme, poignee: Poignee, p: Point): Forme {
  switch (forme.type) {
    case "fleche":
    case "ligne":
    case "surligneur":
      if (poignee === "a") return { ...forme, x1: p.x, y1: p.y };
      if (poignee === "b") return { ...forme, x2: p.x, y2: p.y };
      return forme;
    case "rectangle":
    case "ellipse":
    case "pixellisation":
    case "projecteur":
    case "loupe": {
      const oppose = poignees(forme).find((q) => q.id === { no: "se", ne: "so", so: "ne", se: "no" }[poignee as "no"]);
      if (!oppose) return forme;
      return { ...forme, ...entre(oppose.x, oppose.y, p.x, p.y) };
    }
    default:
      return forme;
  }
}

export function restyler(forme: Forme, style: Style): Forme {
  return { ...forme, ...style };
}

/** Les formes d'avant, celles d'aujourd'hui, celles qu'on peut rétablir. */
export interface Historique {
  passe: Forme[][];
  present: Forme[];
  futur: Forme[][];
}

/** Pas d'annulation gardés : au-delà, les plus anciens tombent. */
const PAS_MAX = 100;

export const historique = (formes: Forme[]): Historique => ({ passe: [], present: formes, futur: [] });

export function engager(h: Historique, formes: Forme[]): Historique {
  return { passe: [...h.passe, h.present].slice(-PAS_MAX), present: formes, futur: [] };
}

export function annuler(h: Historique): Historique {
  if (h.passe.length === 0) return h;
  return { passe: h.passe.slice(0, -1), present: h.passe[h.passe.length - 1], futur: [h.present, ...h.futur] };
}

export function retablir(h: Historique): Historique {
  if (h.futur.length === 0) return h;
  return { passe: [...h.passe, h.present], present: h.futur[0], futur: h.futur.slice(1) };
}

interface Point {
  x: number;
  y: number;
}

/**
 * Passe d'une zone CSS à une zone physique bornée à l'image, ou `null` si elle
 * est vide. Miroir de `stockage::zone_physique` côté Rust : les deux bords
 * sont arrondis, pas la largeur. Rust recadre l'original avec ce calcul et
 * refuse un rendu d'une autre taille.
 */
export function zonePhysique(zone: Zone, facteur: number, image: { largeur: number; hauteur: number }): Zone | null {
  if (!(Number.isFinite(facteur) && facteur > 0)) return null;
  const borne = (valeur: number, max: number) => Math.min(Math.max(Math.round(valeur * facteur), 0), max);
  const x0 = borne(zone.x, image.largeur);
  const y0 = borne(zone.y, image.hauteur);
  const x1 = borne(zone.x + zone.largeur, image.largeur);
  const y1 = borne(zone.y + zone.hauteur, image.hauteur);
  if (!(x1 > x0 && y1 > y0)) return null;
  return { x: x0, y: y0, largeur: x1 - x0, hauteur: y1 - y0 };
}

// --- La fenêtre au clic (ADR-008) ---------------------------------------------------

/** Une zone en pixels physiques de l'écran, en pixels CSS de la sélection. */
export function versCss(zone: Zone, facteur: number): Zone {
  return { x: zone.x / facteur, y: zone.y / facteur, largeur: zone.largeur / facteur, hauteur: zone.hauteur / facteur };
}

/** Rang de la fenêtre sous le point : la première de la liste, rangée de la plus haute à la plus basse. */
export function fenetreSous(fenetres: Zone[], point: { x: number; y: number }): number | null {
  const rang = fenetres.findIndex((f) => point.x >= f.x && point.x < f.x + f.largeur && point.y >= f.y && point.y < f.y + f.hauteur);
  return rang < 0 ? null : rang;
}

// --- Le recadrage (ADR-007)------------------------------------------------------

type Image = { largeur: number; hauteur: number };

/**
 * Un recadrage en pixels entiers de l'original : normalisé, bords arrondis,
 * borné à l'image. `null` s'il a moins de `min` pixels de côté. Rust le
 * contrôle encore (`stockage::valider_recadrage`).
 */
export function recadrageNet(zone: Zone, image: Image, min: number): Zone | null {
  const borne = (v: number, max: number) => Math.min(Math.max(Math.round(v), 0), max);
  const x0 = borne(Math.min(zone.x, zone.x + zone.largeur), image.largeur);
  const y0 = borne(Math.min(zone.y, zone.y + zone.hauteur), image.hauteur);
  const x1 = borne(Math.max(zone.x, zone.x + zone.largeur), image.largeur);
  const y1 = borne(Math.max(zone.y, zone.y + zone.hauteur), image.hauteur);
  if (!(x1 - x0 >= min && y1 - y0 >= min)) return null;
  return { x: x0, y: y0, largeur: x1 - x0, hauteur: y1 - y0 };
}

/** Le recadrage déplacé de `dx`, `dy`, de la même taille, arrêté aux bords de l'image. */
export function glisserRecadrage(zone: Zone, dx: number, dy: number, image: Image): Zone {
  const borne = (v: number, max: number) => Math.min(Math.max(Math.round(v), 0), max);
  return { ...zone, x: borne(zone.x + dx, image.largeur - zone.largeur), y: borne(zone.y + dy, image.hauteur - zone.hauteur) };
}

/** La zone en pixels entiers, bornée au canevas, ou `null` si elle est vide. */
function zoneEntiere(ctx: CanvasRenderingContext2D, zone: Zone): Zone | null {
  const x0 = Math.max(0, Math.floor(Math.min(zone.x, zone.x + zone.largeur)));
  const y0 = Math.max(0, Math.floor(Math.min(zone.y, zone.y + zone.hauteur)));
  const x1 = Math.min(ctx.canvas.width, Math.ceil(Math.max(zone.x, zone.x + zone.largeur)));
  const y1 = Math.min(ctx.canvas.height, Math.ceil(Math.max(zone.y, zone.y + zone.hauteur)));
  return x1 - x0 >= 1 && y1 - y0 >= 1 ? { x: x0, y: y0, largeur: x1 - x0, hauteur: y1 - y0 } : null;
}

/** Voile du projecteur, posé hors de ses rectangles. */
export const VOILE_PROJECTEUR = 0.55;

/**
 * Assombrit tout le canevas sauf les projecteurs : leurs pixels sont mis de côté
 * avant le voile, puis reposés. Deux projecteurs qui se chevauchent restent nets.
 */
function projeter(ctx: CanvasRenderingContext2D, zones: readonly Zone[]) {
  const nets = zones.flatMap((zone) => {
    const z = zoneEntiere(ctx, zone);
    return z ? [{ z, pixels: ctx.getImageData(z.x, z.y, z.largeur, z.hauteur) }] : [];
  });
  ctx.fillStyle = `rgba(0, 0, 0, ${VOILE_PROJECTEUR})`;
  ctx.fillRect(0, 0, ctx.canvas.width, ctx.canvas.height);
  for (const { z, pixels } of nets) ctx.putImageData(pixels, z.x, z.y);
}

/**
 * Pixellise une partie du canevas : chaque bloc prend la moyenne exacte de ses
 * pixels. Ce qui est déjà dessiné dessous, image et formes, est pixellisé.
 */
function pixelliser(ctx: CanvasRenderingContext2D, zone: Zone, bloc: number) {
  const z = zoneEntiere(ctx, zone);
  if (!z) return;
  const { x: x0, y: y0, largeur: l, hauteur: h } = z;
  const image = ctx.getImageData(x0, y0, l, h);
  const p = image.data;
  for (let by = 0; by < h; by += bloc) {
    for (let bx = 0; bx < l; bx += bloc) {
      const somme = [0, 0, 0, 0];
      const fx = Math.min(bx + bloc, l);
      const fy = Math.min(by + bloc, h);
      for (let y = by; y < fy; y++) for (let x = bx; x < fx; x++) for (let c = 0; c < 4; c++) somme[c] += p[(y * l + x) * 4 + c];
      const n = (fx - bx) * (fy - by);
      for (let y = by; y < fy; y++) for (let x = bx; x < fx; x++) for (let c = 0; c < 4; c++) p[(y * l + x) * 4 + c] = Math.round(somme[c] / n);
    }
  }
  ctx.putImageData(image, x0, y0);
}

/**
 * Dessine les formes par-dessus ce que le canevas porte déjà, dans l'ordre :
 * une pixellisation brouille aussi les formes posées avant elle. Les
 * projecteurs passent avant tout le reste. Traits
 * épais : une annotation est faite pour être lue d'un coup d'œil, et par un agent.
 */
export function dessinerFormes(ctx: CanvasRenderingContext2D, formes: readonly Forme[], echelle: number) {
  const e = echelle > 0 ? echelle : 1;
  ctx.save();
  ctx.lineJoin = "round";
  // Le projecteur assombrit l'image d'abord : les autres formes restent nettes par-dessus.
  const projecteurs = formes.filter((f): f is Extract<Forme, { type: "projecteur" }> => f.type === "projecteur");
  if (projecteurs.length > 0) projeter(ctx, projecteurs);
  for (const forme of formes) {
    const { couleur, taille } = styleDe(forme);
    const t = TAILLES[taille].facteur * e;
    ctx.globalAlpha = 1;
    ctx.lineCap = "round";
    ctx.strokeStyle = couleur;
    ctx.fillStyle = couleur;
    if (forme.type === "rectangle") {
      ctx.lineWidth = 4 * t;
      ctx.strokeRect(forme.x, forme.y, forme.largeur, forme.hauteur);
    } else if (forme.type === "ellipse") {
      ctx.lineWidth = 4 * t;
      ctx.beginPath();
      ctx.ellipse(forme.x + forme.largeur / 2, forme.y + forme.hauteur / 2, Math.abs(forme.largeur) / 2, Math.abs(forme.hauteur) / 2, 0, 0, 2 * Math.PI);
      ctx.stroke();
    } else if (forme.type === "pixellisation") {
      pixelliser(ctx, forme, Math.max(10, Math.round(12 * t)));
    } else if (forme.type === "loupe") {
      // ponytail: facteur 2 fixe, la moitié centrale remplit la zone ; un facteur réglable se poserait ici.
      const { x, y, largeur: l, hauteur: h } = forme;
      ctx.drawImage(ctx.canvas, x + l / 4, y + h / 4, l / 2, h / 2, x, y, l, h);
      ctx.lineWidth = 4 * t;
      ctx.strokeRect(x, y, l, h);
    } else if (forme.type === "tampon") {
      // L'émoji porte ses couleurs : la couleur choisie ne s'y applique pas (ADR-014).
      ctx.font = `${Math.round(36 * t * 0.85)}px "Segoe UI Emoji", sans-serif`;
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      ctx.fillText(forme.symbole, forme.x, forme.y);
      ctx.textAlign = "start";
    } else if (forme.type === "surligneur" || forme.type === "ligne") {
      if (forme.type === "surligneur") {
        ctx.globalAlpha = 0.35;
        ctx.lineCap = "butt";
      }
      ctx.lineWidth = (forme.type === "surligneur" ? 18 : 5) * t;
      ctx.beginPath();
      ctx.moveTo(forme.x1, forme.y1);
      ctx.lineTo(forme.x2, forme.y2);
      ctx.stroke();
    } else if (forme.type === "crayon") {
      ctx.lineWidth = 4 * t;
      ctx.beginPath();
      ctx.moveTo(forme.points[0], forme.points[1]);
      for (let i = 2; i + 1 < forme.points.length; i += 2) ctx.lineTo(forme.points[i], forme.points[i + 1]);
      // Un seul point : un trait nul, que le bout rond rend en point.
      if (forme.points.length === 2) ctx.lineTo(forme.points[0], forme.points[1]);
      ctx.stroke();
    } else if (forme.type === "fleche") {
      const angle = Math.atan2(forme.y2 - forme.y1, forme.x2 - forme.x1);
      const pointe = 18 * t;
      // Le trait s'arrête dans la pointe, pour qu'elle reste aiguë.
      const fin = { x: forme.x2 - Math.cos(angle) * pointe * 0.6, y: forme.y2 - Math.sin(angle) * pointe * 0.6 };
      ctx.lineWidth = 5 * t;
      ctx.beginPath();
      ctx.moveTo(forme.x1, forme.y1);
      ctx.lineTo(fin.x, fin.y);
      ctx.stroke();
      ctx.beginPath();
      ctx.moveTo(forme.x2, forme.y2);
      for (const cote of [-1, 1]) {
        ctx.lineTo(forme.x2 - Math.cos(angle + cote * 0.45) * pointe, forme.y2 - Math.sin(angle + cote * 0.45) * pointe);
      }
      ctx.closePath();
      ctx.fill();
    } else if (forme.type === "etape") {
      const rayon = 13 * t;
      ctx.beginPath();
      ctx.arc(forme.x, forme.y, rayon, 0, 2 * Math.PI);
      ctx.fill();
      ctx.font = police(rayon * (forme.numero > 9 ? 1 : 1.25));
      ctx.textAlign = "center";
      ctx.textBaseline = "middle";
      // Le chiffre se lit sur la couleur du disque : noir sur clair, blanc sur sombre.
      ctx.fillStyle = luminance(couleur) > 0.5 ? "#1c1c1e" : "#ffffff";
      ctx.fillText(String(forme.numero), forme.x, forme.y + rayon * 0.05);
      ctx.textAlign = "start";
    } else if (forme.type === "texte") {
      ctx.font = police(TAILLE_TEXTE * t);
      ctx.textBaseline = "top";
      // Liseré sombre : le texte reste lisible sur un fond de sa couleur ou clair.
      // Le texte noir prend un liseré clair.
      ctx.lineWidth = 3 * t;
      ctx.strokeStyle = luminance(couleur) < 0.1 ? "rgba(255, 255, 255, 0.7)" : "rgba(0, 0, 0, 0.55)";
      ctx.strokeText(forme.texte, forme.x, forme.y);
      ctx.fillText(forme.texte, forme.x, forme.y);
    }
  }
  ctx.restore();
}

// --- L'orientation (ADR-018) -------------------------------------------------------

/** Les trois gestes de la barre de réédition : un quart de tour horaire, deux miroirs. */
export type Bouton = "pivoter" | "horizontal" | "vertical";

/** Les six nombres de `setTransform` : x' = a x + c y + e, y' = b x + d y + f. */
export type Matrice = [number, number, number, number, number, number];

/**
 * L'état après un geste sur l'affiché. État `(miroir ? 4 : 0) + quarts`, affiché =
 * R^quarts(M^miroir(original)) ; un miroir renverse le sens des quarts de tour.
 * Prouvé par calcul sur une grille 3 x 2 (test Node).
 */
export function composer(orientation: number, bouton: Bouton): number {
  const q = orientation % 4;
  const m = orientation >= 4 ? 1 : 0;
  switch (bouton) {
    case "pivoter":
      return 4 * m + ((q + 1) % 4);
    case "horizontal":
      return 4 * (1 - m) + ((4 - q) % 4);
    case "vertical":
      return 4 * (1 - m) + ((6 - q) % 4);
  }
}

/** Largeur et hauteur une fois orientée : un quart de tour les échange. Miroir de `stockage::taille_orientee`. */
export function tailleOrientee(taille: Image, orientation: number): Image {
  return orientation % 2 === 1 ? { largeur: taille.hauteur, hauteur: taille.largeur } : { largeur: taille.largeur, hauteur: taille.hauteur };
}

/** `A` après `B`. */
function multiplier(A: Matrice, B: Matrice): Matrice {
  return [
    A[0] * B[0] + A[2] * B[1],
    A[1] * B[0] + A[3] * B[1],
    A[0] * B[2] + A[2] * B[3],
    A[1] * B[2] + A[3] * B[3],
    A[0] * B[4] + A[2] * B[5] + A[4],
    A[1] * B[4] + A[3] * B[5] + A[5],
  ];
}

/** La matrice qui envoie un pixel de l'original (`largeur` x `hauteur`) à sa place dans l'affiché. */
export function matriceOrientation(orientation: number, largeur: number, hauteur: number): Matrice {
  const quarts = orientation % 4;
  let m: Matrice = orientation >= 4 ? [-1, 0, 0, 1, largeur, 0] : [1, 0, 0, 1, 0, 0];
  let h = hauteur;
  for (let i = 0; i < quarts; i++) {
    // Un quart de tour horaire d'une image de hauteur h : (x, y) devient (h - y, x).
    m = multiplier([0, 1, -1, 0, h, 0], m);
    h = i % 2 === 0 ? largeur : hauteur;
  }
  return m;
}

/** Un point de l'espace affiché (`taille` avant le geste) après le geste. */
function transformerPoint(x: number, y: number, bouton: Bouton, taille: Image): [number, number] {
  switch (bouton) {
    case "pivoter":
      return [taille.hauteur - y, x];
    case "horizontal":
      return [taille.largeur - x, y];
    case "vertical":
      return [x, taille.hauteur - y];
  }
}

/** Une boîte (le recadrage, un rectangle) : ses deux coins opposés transformés, refaite par `entre`. */
export function orienterZone(zone: Zone, bouton: Bouton, taille: Image): Zone {
  const [x0, y0] = transformerPoint(zone.x, zone.y, bouton, taille);
  const [x1, y1] = transformerPoint(zone.x + zone.largeur, zone.y + zone.hauteur, bouton, taille);
  return entre(x0, y0, x1, y1);
}

/**
 * Une forme après le geste. Les traits et les boîtes suivent leurs points ; l'étape, le
 * tampon et le texte suivent leur point d'ancrage et gardent leur contenu droit : le
 * texte par le centre de sa boîte mesurée, dont `l` et `px` ne s'échangent jamais.
 */
export function orienterForme(forme: Forme, bouton: Bouton, taille: Image, echelle: number, mesure: Mesure = MESURE_ESTIMEE): Forme {
  const point = (x: number, y: number) => transformerPoint(x, y, bouton, taille);
  switch (forme.type) {
    case "fleche":
    case "ligne":
    case "surligneur": {
      const [x1, y1] = point(forme.x1, forme.y1);
      const [x2, y2] = point(forme.x2, forme.y2);
      return { ...forme, x1, y1, x2, y2 };
    }
    case "crayon": {
      const points: number[] = [];
      for (let i = 0; i + 1 < forme.points.length; i += 2) points.push(...point(forme.points[i], forme.points[i + 1]));
      return { ...forme, points };
    }
    case "texte": {
      const b = boite(forme, echelle, mesure);
      const [cx, cy] = point(b.x + b.largeur / 2, b.y + b.hauteur / 2);
      return { ...forme, x: cx - b.largeur / 2, y: cy - b.hauteur / 2 };
    }
    case "etape":
    case "tampon": {
      const [x, y] = point(forme.x, forme.y);
      return { ...forme, x, y };
    }
    default:
      return { ...forme, ...orienterZone(forme, bouton, taille) };
  }
}

export function orienterFormes(formes: readonly Forme[], bouton: Bouton, taille: Image, echelle: number, mesure?: Mesure): Forme[] {
  return formes.map((f) => orienterForme(f, bouton, taille, echelle, mesure));
}

/** La même fonction sur le passé, le présent et le futur, sans engager de pas. */
export function remanierHistorique(h: Historique, f: (formes: Forme[]) => Forme[]): Historique {
  return { passe: h.passe.map(f), present: f(h.present), futur: h.futur.map(f) };
}

/** Tout l'historique orienté : Ctrl+Z après un pivot rend des formes à leur place pivotée. */
export function orienterHistorique(h: Historique, bouton: Bouton, taille: Image, echelle: number, mesure?: Mesure): Historique {
  return remanierHistorique(h, (formes) => orienterFormes(formes, bouton, taille, echelle, mesure));
}
