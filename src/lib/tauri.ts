/**
 * Pont avec Tauri : détection de la fenêtre, confirmation, texte d'erreur.
 * Toute erreur remontée par Rust arrive sous forme de chaîne lisible.
 */
import { confirm } from "@tauri-apps/plugin-dialog";

/** Vrai seulement dans une fenêtre Tauri, faux dans un navigateur nu. */
export function isTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/**
 * Boîte de confirmation. Dans la fenêtre Tauri, `window.confirm` est remplacé
 * par le greffon dialog et rend une promesse : un `if (window.confirm(...))`
 * y est toujours vrai. Tout le front passe par ici et
 * attend. Les parcours de bout en bout répondent « Confirmer ».
 */
export function confirmer(message: string): Promise<boolean> {
  if (!isTauri()) return Promise.resolve(window.confirm(message));
  return confirm(message, { title: "EquiShot", kind: "warning", okLabel: "Confirmer", cancelLabel: "Annuler" });
}

/** Message d'erreur affichable, quelle que soit la forme de l'erreur. */
export function errorText(error: unknown): string {
  if (typeof error === "string") return error;
  if (error instanceof Error) return error.message;
  return "Erreur inattendue.";
}
