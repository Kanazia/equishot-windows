/**
 * La page du dessin à l'écran pendant la prise (0.23, ADR-023), comme ZoomIt. Rust
 * la pose sur la zone filmée, protégée : elle n'est jamais filmée. Chaque trait est
 * envoyé à Rust en pixels physiques de la zone, et le moteur le redessine dans
 * l'image, sous le zoom. Glisser : stylo ; Maj : flèche ; Ctrl : rectangle ;
 * R, J, B, V : couleur ; E : tout effacer ; Ctrl+Z : annuler ; Échap ou clic droit :
 * finir (les traits s'effacent).
 */
import { videoDessin, videoDessinBasculer, type TraitDessin } from "@/lib/captures-api";

type Rgb = [number, number, number];

const COULEURS: Record<string, Rgb> = {
  r: [0.93, 0.16, 0.16],
  j: [1, 0.77, 0],
  b: [0.24, 0.61, 1],
  v: [0.2, 0.78, 0.35],
};

const toile = document.querySelector("canvas");
const contexte = toile?.getContext("2d");
if (!toile || !contexte) {
  throw new Error("Page du dessin incomplète.");
}
const canvas: HTMLCanvasElement = toile;
const ctx: CanvasRenderingContext2D = contexte;

let traits: TraitDessin[] = [];
let enCours: TraitDessin | null = null;
let couleur: Rgb = COULEURS.r;
let prevu = false;
let fini = false;

const css = (c: Rgb) => `rgb(${c.map((v) => Math.round(v * 255)).join(",")})`;
const pointDe = (e: PointerEvent): [number, number] => [e.clientX * devicePixelRatio, e.clientY * devicePixelRatio];

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

function tracer(t: TraitDessin) {
  ctx.strokeStyle = css(t.couleur);
  ctx.lineWidth = t.epaisseur;
  ctx.lineCap = "round";
  ctx.lineJoin = "round";
  const [de] = t.points;
  const a = t.points[t.points.length - 1];
  if (!de) return;
  ctx.beginPath();
  if (t.outil === "stylo") {
    ctx.moveTo(de[0], de[1]);
    for (const p of t.points) ctx.lineTo(p[0], p[1]);
    if (t.points.length === 1) ctx.lineTo(de[0] + 0.01, de[1]);
  } else if (t.outil === "rectangle") {
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

/** Redessine et envoie les traits, une fois par image au plus. */
function prevoir() {
  if (prevu) return;
  prevu = true;
  requestAnimationFrame(() => {
    prevu = false;
    const tous = enCours ? [...traits, enCours] : traits;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    tous.forEach(tracer);
    void videoDessin(tous).catch(() => {});
  });
}

function finir() {
  if (fini) return;
  fini = true;
  void videoDessinBasculer().catch(() => {});
}

canvas.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  canvas.setPointerCapture(e.pointerId);
  const p = pointDe(e);
  const outil = e.ctrlKey ? "rectangle" : e.shiftKey ? "fleche" : "stylo";
  enCours = { outil, couleur, epaisseur: 4 * devicePixelRatio, points: outil === "stylo" ? [p] : [p, p] };
  prevoir();
});

canvas.addEventListener("pointermove", (e) => {
  if (!enCours) return;
  const p = pointDe(e);
  if (enCours.outil === "stylo") {
    const d = enCours.points[enCours.points.length - 1];
    if (Math.hypot(p[0] - d[0], p[1] - d[1]) < 1.5) return;
    enCours.points.push(p);
  } else {
    enCours.points[1] = p;
  }
  prevoir();
});

const lacher = () => {
  if (!enCours) return;
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
retailler();
window.focus();
