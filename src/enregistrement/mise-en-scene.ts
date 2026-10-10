/**
 * La logique pure du panneau de préparation (ADR-017), testée sous Node
 * (`tests/unit/video-preparation.test.mjs`) : seul un `import type` vient d'ailleurs.
 */
import type { CibleScene, MiseScene, Peripherique } from "@/lib/captures-api";

export const REPLI_MICRO = "Le micro choisi n'est plus branché : micro par défaut.";
export const REPLI_SORTIE = "La sortie son choisie n'est plus branchée : sortie par défaut.";
export const REPLI_CAMERA = "La caméra choisie n'est plus branchée : caméra par défaut.";

/** La valeur d'une liste du panneau : « aucun », « defaut » ou un identifiant branché. */
export function choixAffiche(actif: boolean, id: string | null, liste: Peripherique[], repli = REPLI_MICRO): { valeur: string; note: string | null } {
  if (!actif) return { valeur: "aucun", note: null };
  if (id === null) return { valeur: "defaut", note: null };
  if (liste.some((p) => p.id === id)) return { valeur: id, note: null };
  return { valeur: "defaut", note: repli };
}

/** L'inverse : ce qu'une valeur de liste écrit dans les réglages. */
export function choixLu(valeur: string): { actif: boolean; id: string | null } {
  if (valeur === "aucun") return { actif: false, id: null };
  return { actif: true, id: valeur === "defaut" ? null : valeur };
}

/**
 * Un seul envoi en vol, la dernière valeur voulue en attente : un curseur glissé
 * écrit dans l'ordre et la dernière gagne. `fini` attend la file vide (le lancement
 * relit les réglages écrits).
 */
export function fileDeUn<T>(envoyer: (v: T) => Promise<unknown>, echec: (e: unknown) => void) {
  let enVol: Promise<void> | null = null;
  let attente: { v: T } | null = null;
  async function vider() {
    while (attente) {
      const { v } = attente;
      attente = null;
      try {
        await envoyer(v);
      } catch (e) {
        echec(e);
      }
    }
    enVol = null;
  }
  return {
    pousser(v: T) {
      attente = { v };
      enVol ??= vider();
    },
    fini: () => enVol ?? Promise.resolve(),
  };
}

/**
 * La file d'un glisser : les messages d'un appui ne partent qu'une fois parti le
 * dernier message du glisser d'avant (son « fini »), que `fileDeUn` écraserait sinon.
 * Chaînée, pour tenir aussi trois glissers pendant un seul envoi en vol.
 */
export function fileDeGlisser<T>(envoyer: (v: T) => Promise<unknown>) {
  const file = fileDeUn(envoyer, () => {});
  let precedent: Promise<void> = Promise.resolve();
  return {
    appui() {
      precedent = precedent.then(() => file.fini());
    },
    pousser(v: T) {
      void precedent.then(() => file.pousser(v));
    },
  };
}

export const pourcent =(v: number) => `${Math.round(v * 100)} %`;

export const NOM_SCENE_MAX = 40;

/** Le nom d'une scène tel que Rust le garde : sans espaces autour, 1 à 40 caractères. */
export function nomDeScene(brut: string): { nom: string } | { erreur: string } {
  const nom = brut.trim();
  const n = [...nom].length;
  if (n === 0) return { erreur: "Donnez un nom à la scène." };
  if (n > NOM_SCENE_MAX) return { erreur: `${NOM_SCENE_MAX} caractères au plus pour le nom d'une scène.` };
  return { nom };
}

/** Ce que « Enregistrer comme scène » propose de garder. */
export function libelleCible(cible: CibleScene | null): string {
  return cible?.genre === "ecran" ? "Cet écran" : "Cette zone";
}

/**
 * Entrée et Échap du panneau. Le formulaire d'une scène ouvert, ils l'enregistrent
 * ou le ferment : jamais ils ne lancent ni n'abandonnent la vidéo.
 */
export function toucheDuPanneau(touche: string, formulaire: boolean): "lancer" | "abandonner" | "valider" | "fermer" | null {
  if (formulaire) {
    if (touche === "Enter") return "valider";
    if (touche === "Escape") return "fermer";
    return null;
  }
  if (touche === "Enter") return "lancer";
  if (touche === "Escape") return "abandonner";
  return null;
}

/** Une ligne pour la rangée d'une scène : la cible, le son, la bulle. */
export function resumeScene(s: { cible: CibleScene; mise: MiseScene }): string {
  const cible =
    s.cible.genre === "aTracer"
      ? "Zone à tracer"
      : s.cible.genre === "ecran"
        ? `Écran entier ${s.cible.ecran.taille[0]} x ${s.cible.ecran.taille[1]}`
        : `Zone ${s.cible.zone[2]} x ${s.cible.zone[3]}`;
  const micro = s.mise.micro ? `micro ${pourcent(s.mise.volumeMicro)}` : "sans micro";
  const son = s.mise.sonSysteme ? `son de l'ordinateur ${pourcent(s.mise.volumeSon)}` : "sans son de l'ordinateur";
  const bulle = s.mise.camera ? `caméra ${s.mise.cameraTaille}` : "sans caméra";
  return `${cible} · ${micro}, ${son} · ${bulle}`;
}
