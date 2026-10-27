/**
 * La page du dessin à l'écran pendant la prise (0.23, ADR-023), comme ZoomIt. Rust
 * la pose sur la zone filmée, protégée : elle n'est jamais filmée. Chaque trait est
 * envoyé à Rust en pixels physiques de la zone, et le moteur le redessine dans
 * l'image, sous le zoom. Elle garde le clic : rien n'est sélectionné dessous.
 * 0.24 (ADR-024) : deux outils, le pointeur (encre jaune qui s'efface) et le crayon
 * (encre qui reste), et le stylo au pinceau d'encre de Chine (`pinceau.ts`).
 * Glisser : stylo ; Maj : flèche ; Ctrl : rectangle ; R, J, B, V : couleur ;
 * E : tout effacer ; Ctrl+Z : annuler ; Échap ou clic droit : revenir à la souris
 * (les traits s'effacent).
 */
import { surOutilVideo, videoDessin, videoEtat, videoOutil, type OutilSouris, type TraitDessin } from "@/lib/captures-api";

import { pinceau } from "./pinceau";

type Rgb = [number, number, number];

const COULEURS: Record<string, Rgb> = {
  r: [0.93, 0.16, 0.16],
  j: [1, 0.77, 0],
  b: [0.24, 0.61, 1],
  v: [0.2, 0.78, 0.35],
};
/** Le pointeur écrit en jaune, comme la traînée ; le crayon en rouge. */
const COULEUR_DE: Record<"pointeur" | "crayon", Rgb> = { pointeur: COULEURS.j, crayon: COULEURS.r };
/** L'épaisseur de base du pinceau, en pixels CSS : celle de la traînée pour le pointeur. */
const BASE: Record<"pointeur" | "crayon", number> = { pointeur: 6, crayon: 5 };
/** Le trait du pointeur reste 0,4 s après le lâcher, puis s'efface en 0,6 s. */
const TENU_MS = 400;
const FONDU_MS = 600;
const AIDE: Record<"pointeur" | "crayon", string> = {
  pointeur: "Pointeur : l'encre s'efface seule. Maj : flèche. Ctrl : rectangle. Échap : souris.",
  crayon: "Crayon : glisser pour écrire. Maj : flèche. Ctrl : rectangle. R J B V : couleur. E : effacer. Ctrl+Z : annuler. Échap : souris.",
};

/** Un trait de la page : celui envoyé à Rust, plus l'instant de chaque point et, au pointeur, celui du lâcher. */
type Trait = TraitDessin & { temps: number[]; ephemere: boolean; lache?: number };

const toile = document.querySelector("canvas");
const contexte = toile?.getContext("2d");
const ligne = document.getElementById("aide");
if (!toile || !contexte || !ligne) {
  throw new Error("Page du dessin incomplète.");
}
const canvas: HTMLCanvasElement = toile;
const ctx: CanvasRenderingContext2D = contexte;
const aide: HTMLElement = ligne;

let outil: "pointeur" | "crayon" = "crayon";
let traits: Trait[] = [];
let enCours: Trait | null = null;
let couleur: Rgb = COULEUR_DE.crayon;
let prevu = false;
let fini = false;

const css = (c: Rgb, a = 1) => `rgba(${c.map((v) => Math.round(v * 255)).join(",")},${a})`;
const pointDe = (e: PointerEvent): [number, number] => [e.clientX * devicePixelRatio, e.clientY * devicePixelRatio];

function prendre(nouveau: OutilSouris) {
  if (nouveau === "souris") return;
  outil = nouveau;
  couleur = COULEUR_DE[nouveau];
  document.documentElement.style.setProperty("--couleur", css(couleur));
  aide.textContent = AIDE[nouveau];
}

function retailler() {
  canvas.width = Math.round(innerWidth * devicePixelRatio);
  canvas.height = Math.round(innerHeight * devicePixelRatio);
  prevoir();
}

/** Les deux branches de la tête d'une flèche, comme `tete_de_fleche` côté Rust. */
function tete(de: [number, number], a: [number, number], longueur: number): [number, number][] {
  const angle = Math.atan2(a[1] - de[1], a[0] - de[0]);
  const ouverture = (28 * Math.PI) / 180;
  return [angle + Math.PI - ouverture, angle + Math.PI + ouverture].map((t) => [a[0] + longueur * Math.cos(t), a[1] + longueur * Math.sin(t)]);
}

/** La toile où un trait qui s'efface est dessiné opaque, avant d'être posé à son opacité. */
const calque = document.createElement("canvas");
const cctx = calque.getContext("2d")!;

/**
 * Pose un trait à son opacité. Dessiné à travers, chaque segment repeindrait la jointure
 * du précédent : des perles plus foncées le long du trait pendant le fondu.
 */
function poser(t: TraitDessin) {
  const opacite = t.opacite ?? 1;
  if (opacite >= 1) return tracer(ctx, t);
  if (calque.width !== canvas.width || calque.height !== canvas.height) {
    calque.width = canvas.width;
    calque.height = canvas.height;
  }
  cctx.clearRect(0, 0, calque.width, calque.height);
  tracer(cctx, t);
  ctx.globalAlpha = opacite;
  ctx.drawImage(calque, 0, 0);
  ctx.globalAlpha = 1;
}

/** Dessine un trait opaque comme le moteur : le stylo segment par segment, à l'épaisseur moyenne de ses deux bouts. */
function tracer(ctx: CanvasRenderingContext2D, t: TraitDessin) {
  ctx.strokeStyle = css(t.couleur);
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  const [de] = t.points;
  const a = t.points[t.points.length - 1];
  if (!de) return;
  if (t.outil === "stylo") {
    if (t.points.length === 1) {
      ctx.lineWidth = t.epaisseur;
      ctx.beginPath();
      ctx.moveTo(de[0], de[1]);
      ctx.lineTo(de[0] + 0.01, de[1]);
      ctx.stroke();
    }
    for (let i = 1; i < t.points.length; i++) {
      ctx.lineWidth = t.largeurs ? (t.largeurs[i - 1] + t.largeurs[i]) / 2 : t.epaisseur;
      ctx.beginPath();
      ctx.moveTo(t.points[i - 1][0], t.points[i - 1][1]);
      ctx.lineTo(t.points[i][0], t.points[i][1]);
      ctx.stroke();
    }
    return;
  }
  ctx.lineWidth = t.epaisseur;
  ctx.beginPath();
  if (t.outil === "rectangle") {
    ctx.rect(Math.min(de[0], a[0]), Math.min(de[1], a[1]), Math.abs(a[0] - de[0]), Math.abs(a[1] - de[1]));
  } else {
    ctx.moveTo(de[0], de[1]);
    ctx.lineTo(a[0], a[1]);
    for (const bout of tete(de, a, t.epaisseur * 4)) {
      ctx.moveTo(a[0], a[1]);
      ctx.lineTo(bout[0], bout[1]);
    }
  }
  ctx.stroke();
}

/** L'opacité d'un trait du pointeur lâché à `lache`, à l'instant `maintenant`. */
const opaciteA = (t: Trait, maintenant: number) => (t.lache === undefined ? 1 : Math.min(1, 1 - (maintenant - t.lache - TENU_MS) / FONDU_MS));

/** Redessine et envoie les traits, une fois par image au plus ; tant qu'un trait s'efface, à chaque image. */
function prevoir() {
  if (prevu) return;
  prevu = true;
  requestAnimationFrame((maintenant) => {
    prevu = false;
    traits = traits.filter((t) => !t.ephemere || opaciteA(t, maintenant) > 0);
    const tous = (enCours ? [...traits, enCours] : traits).map(
      (t): TraitDessin => ({
        outil: t.outil,
        couleur: t.couleur,
        epaisseur: t.epaisseur,
        points: t.points,
        largeurs: t.largeurs,
        opacite: t.ephemere ? opaciteA(t, maintenant) : 1,
      }),
    );
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    tous.forEach(poser);
    void videoDessin(tous).catch(() => {});
    if (traits.some((t) => t.ephemere && t.lache !== undefined)) prevoir();
  });
}

function finir() {
  if (fini) return;
  fini = true;
  void videoOutil("souris").catch(() => {});
}

canvas.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  canvas.setPointerCapture(e.pointerId);
  const p = pointDe(e);
  const forme = e.ctrlKey ? "rectangle" : e.shiftKey ? "fleche" : "stylo";
  const epaisseur = BASE[outil] * devicePixelRatio;
  enCours = { outil: forme, couleur, epaisseur, points: forme === "stylo" ? [p] : [p, p], temps: [e.timeStamp / 1000], ephemere: outil === "pointeur" };
  prevoir();
});

canvas.addEventListener("pointermove", (e) => {
  if (!enCours) return;
  const p = pointDe(e);
  if (enCours.outil === "stylo") {
    const d = enCours.points[enCours.points.length - 1];
    if (Math.hypot(p[0] - d[0], p[1] - d[1]) < 1.5) return;
    enCours.points.push(p);
    enCours.temps.push(e.timeStamp / 1000);
    enCours.largeurs = pinceau(
      enCours.points.map((q, i) => [q[0], q[1], enCours!.temps[i]]),
      enCours.epaisseur,
    );
  } else {
    enCours.points[1] = p;
  }
  prevoir();
});

const lacher = () => {
  if (!enCours) return;
  if (enCours.ephemere) enCours.lache = performance.now();
  traits.push(enCours);
  enCours = null;
  prevoir();
};
canvas.addEventListener("pointerup", lacher);
canvas.addEventListener("pointercancel", lacher);

window.addEventListener("contextmenu", (e) => {
  e.preventDefault();
  finir();
});

window.addEventListener("keydown", (e) => {
  const touche = e.key.toLowerCase();
  if (e.key === "Escape") {
    finir();
  } else if (touche === "z" && e.ctrlKey) {
    traits.pop();
    prevoir();
  } else if (touche === "e" && !e.ctrlKey && !e.altKey) {
    traits = [];
    enCours = null;
    prevoir();
  } else if (COULEURS[touche] && !e.ctrlKey && !e.altKey) {
    couleur = COULEURS[touche];
    document.documentElement.style.setProperty("--couleur", css(couleur));
  } else {
    return;
  }
  e.preventDefault();
});

window.addEventListener("resize", retailler);
void surOutilVideo(prendre);
void videoEtat()
  .then((etat) => prendre(etat.outil))
  .catch(() => {});
prendre("crayon");
retailler();
window.focus();
