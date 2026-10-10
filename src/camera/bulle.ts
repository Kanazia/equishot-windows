/**
 * La logique pure de la page de la bulle (ADR-017, 0.18), testée sous Node
 * (`tests/unit/video-bulle.test.mjs`) : seul un `import type` vient d'ailleurs.
 */
import type { Camera } from "@/lib/captures-api";

export interface AppareilVideo {
  kind: string;
  deviceId: string;
  label: string;
}

/** Les entrées vidéo de `enumerateDevices`, nommées « Caméra n » quand l'étiquette manque. */
export function camerasDe(appareils: AppareilVideo[]): Camera[] {
  return appareils.filter((a) => a.kind === "videoinput" && a.deviceId !== "").map((a, i) => ({ id: a.deviceId, nom: a.label || `Caméra ${i + 1}` }));
}

/** La caméra choisie est exigée (`exact`) : absente, l'erreur le dit et l'on repasse au défaut. */
export function contraintes(cameraId: string | null): MediaStreamConstraints {
  return { audio: false, video: cameraId === null ? true : { deviceId: { exact: cameraId } } };
}

const nomDe = (e: unknown) => (typeof e === "object" && e !== null && "name" in e && typeof e.name === "string" ? e.name : "");

/** `OverconstrainedError` (deviceId exact introuvable) ou `NotFoundError` : la caméra n'est plus là. */
export const estAbsente = (e: unknown) => ["OverconstrainedError", "NotFoundError"].includes(nomDe(e));

export const messageDe = (e: unknown) => (nomDe(e) === "NotFoundError" ? "Aucune caméra branchée." : `Caméra indisponible : ${nomDe(e) || "erreur inconnue"}.`);
