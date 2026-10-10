/** Comment la capture défilante avance (ADR-019) : la molette dans un sens, ou soi-même. */
export type Facon = "bas" | "haut" | "droite" | "manuel";

export const FACONS: readonly Facon[] = ["bas", "haut", "droite", "manuel"];

export const LIBELLES: Record<Facon, string> = {
  bas: "Vers le bas",
  haut: "Vers le haut",
  droite: "Vers la droite",
  manuel: "Je fais défiler moi-même",
};

/** La dernière façon choisie, gardée par la page de sélection. */
export const CLE_FACON = "equishot.defilante.facon";

/** La façon gardée ; vers le bas si elle manque ou ne se reconnaît pas. */
export function lireFacon(brut: string | null): Facon {
  return FACONS.find((f) => f === brut) ?? "bas";
}
