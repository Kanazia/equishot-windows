/**
 * La page de la bulle de caméra (ADR-017, 0.18). Elle n'allume la caméra qu'après
 * `video_bulle_config` (l'autorisation est alors posée par Rust), énumère les
 * caméras pour le panneau, suit `video://bulle` (caméra, miroir), se glisse à la
 * souris (deltas d'écran cumulés, envoyés par une file de un), et dit sa perte.
 * `data-etat` sur <html> : `attente`, `filme`, `perdue` (lu par le parcours).
 */
import { fileDeGlisser } from "@/enregistrement/mise-en-scene";
import { surBulle, videoBulleCameras, videoBulleConfig, videoBulleDeplacer, videoBullePerdue, type ConfigBulle } from "@/lib/captures-api";

import { camerasDe, contraintes, estAbsente, messageDe } from "./bulle";

const racine = document.documentElement;
const disque = document.getElementById("disque");
const trouvee = document.querySelector("video");
if (!disque || !trouvee) {
  throw new Error("Page de la bulle incomplète.");
}
// Les fonctions déclarées plus bas sont hissées : le rétrécissement de `trouvee` ne les suit pas.
const video: HTMLVideoElement = trouvee;

let flux: MediaStream | null = null;
// La caméra demandée par la dernière config, retenue avant l'allumage : un réglage du
// panneau qui ne la change pas ne relance rien, même quand elle a cédé au défaut.
let voulue: string | null | undefined;
// Chaque extinction périme l'allumage en cours : son flux arrivé tard est arrêté, son échec ignoré.
let generation = 0;

const etat = (e: "attente" | "filme" | "perdue") => {
  racine.dataset.etat = e;
};

function eteindre() {
  generation++;
  flux?.getTracks().forEach((piste) => piste.stop());
  flux = null;
  video.srcObject = null;
}

async function perdre(motif: string) {
  eteindre();
  etat("perdue");
  await videoBullePerdue(motif).catch(() => {});
}

/** Ouvre `cameraId` ; absente, repasse au défaut (le panneau le dit par la liste). */
async function allumer(cameraId: string | null): Promise<void> {
  eteindre();
  const n = generation;
  let neuf: MediaStream;
  try {
    neuf = await navigator.mediaDevices.getUserMedia(contraintes(cameraId));
  } catch (e) {
    if (n !== generation) return;
    if (cameraId !== null && estAbsente(e)) return allumer(null);
    return perdre(messageDe(e));
  }
  if (n !== generation) {
    neuf.getTracks().forEach((piste) => piste.stop());
    return;
  }
  flux = neuf;
  video.srcObject = neuf;
  neuf.getVideoTracks()[0]?.addEventListener("ended", () => void perdre("Caméra débranchée"));
  etat("filme");
  const appareils = await navigator.mediaDevices.enumerateDevices();
  await videoBulleCameras(camerasDe(appareils)).catch(() => {});
}

function appliquer(config: ConfigBulle) {
  video.classList.toggle("miroir", config.miroir);
  if (config.cameraId === voulue) return;
  voulue = config.cameraId;
  void allumer(config.cameraId);
}

// --- Le glisser : deltas d'écran cumulés depuis l'appui, la dernière valeur gagne.
let appui: { x: number; y: number } | null = null;
// Un « fini » perdu laisserait à Rust l'ancien départ : la bulle sauterait au glisser suivant.
const envoi = fileDeGlisser((m: { dx: number; dy: number; fini: boolean }) => videoBulleDeplacer(m.dx, m.dy, m.fini));
const pousser = envoi.pousser;
disque.addEventListener("pointerdown", (e) => {
  if (e.button !== 0) return;
  envoi.appui();
  appui = { x: e.screenX, y: e.screenY };
  disque.setPointerCapture(e.pointerId);
});
disque.addEventListener("pointermove", (e) => {
  if (appui) pousser({ dx: e.screenX - appui.x, dy: e.screenY - appui.y, fini: false });
});
const lacher = (e: PointerEvent) => {
  if (!appui) return;
  pousser({ dx: e.screenX - appui.x, dy: e.screenY - appui.y, fini: true });
  appui = null;
};
disque.addEventListener("pointerup", lacher);
disque.addEventListener("pointercancel", lacher);

window.addEventListener("pagehide", eteindre);
void surBulle(appliquer);
videoBulleConfig().then(appliquer, (e) => void perdre(e instanceof Error ? e.message : String(e)));
