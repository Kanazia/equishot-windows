/**
 * Lecture d'une frappe pour l'enregistreur du raccourci de capture (ADR-075).
 * Fonctions pures, sans import : elles se testent sans la WebView
 * (`tests/unit/captures-frappe.test.mjs`).
 */

export const MODIFICATEURS = ["Control", "Alt", "Shift", "Meta", "AltGraph"];

/** Ce que chaque touche physique écrit sans modificateur, selon la disposition du clavier. */
export type Disposition = { get(code: string): string | undefined };

/** Ce que l'enregistreur lit d'un évènement clavier. */
export type Frappe = Pick<KeyboardEvent, "code" | "key" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">;

/**
 * La disposition du clavier, ou `null` si la WebView ne la donne pas. Sans elle,
 * l'enregistreur retiendrait la position physique de la touche : sur un AZERTY,
 * frapper Ctrl+Alt+A enregistrerait Ctrl+Alt+Q.
 */
export async function lireDisposition(): Promise<Disposition | null> {
  const clavier = (navigator as Navigator & { keyboard?: { getLayoutMap?: () => Promise<Disposition> } }).keyboard;
  try {
    return (await clavier?.getLayoutMap?.()) ?? null;
  } catch {
    return null;
  }
}

/** Touches dont le nom ne dépend pas de la disposition : chiffres, fonctions, flèches, pavé. */
const TOUCHE_STABLE = /^(Digit\d|F\d{1,2}|Arrow(Up|Down|Left|Right)|Numpad\w+|Space|Enter|Tab|Backspace|Delete|Insert|Home|End|PageUp|PageDown|PrintScreen|Pause)$/;

/**
 * Le nom, pour Windows, de la touche frappée. Une lettre est celle que la
 * touche écrit, pas celle de sa position. `null` pour une touche de ponctuation,
 * dont le nom change d'une disposition à l'autre.
 */
export function toucheFrappee(evenement: Pick<Frappe, "code" | "key">, disposition: Disposition | null): string | null {
  if (TOUCHE_STABLE.test(evenement.code)) return evenement.code;
  // Sans la disposition, la lettre lue dans l'évènement fait foi quand il en porte une.
  const ecrit = disposition?.get(evenement.code) ?? (disposition ? "" : evenement.key);
  return /^[a-z]$/i.test(ecrit) ? `Key${ecrit.toUpperCase()}` : null;
}

/**
 * La combinaison d'une frappe, dans l'écriture que Rust relit (`Ctrl+Alt+KeyS`).
 * `null` tant que seuls des modificateurs sont tenus, ou si la touche n'a pas de
 * nom sûr.
 */
export function combinaisonFrappee(evenement: Frappe, disposition: Disposition | null): string | null {
  if (MODIFICATEURS.includes(evenement.key)) return null;
  const touche = toucheFrappee(evenement, disposition);
  if (!touche) return null;
  const morceaux: string[] = [];
  if (evenement.ctrlKey) morceaux.push("Ctrl");
  if (evenement.altKey) morceaux.push("Alt");
  if (evenement.shiftKey) morceaux.push("Shift");
  if (evenement.metaKey) morceaux.push("Super");
  return [...morceaux, touche].join("+");
}

/** Boutons permis par `PointerEvent.button` : 1 molette, 3 bouton 4, 4 bouton 5. */
const BOUTONS: Record<number, string> = { 1: "Molette", 3: "Bouton4", 4: "Bouton5" };

/**
 * La combinaison d'un clic, dans l'écriture que Rust relit (`Alt+Bouton5`).
 * `null` pour le clic gauche, le droit ou un bouton inconnu. Les modificateurs
 * passent tels quels : Rust refuse Win et le bouton seul, avec la raison.
 */
export function combinaisonCliquee(evenement: Pick<MouseEvent, "button" | "ctrlKey" | "altKey" | "shiftKey" | "metaKey">): string | null {
  const bouton = BOUTONS[evenement.button];
  if (!bouton) return null;
  const morceaux: string[] = [];
  if (evenement.ctrlKey) morceaux.push("Ctrl");
  if (evenement.altKey) morceaux.push("Alt");
  if (evenement.shiftKey) morceaux.push("Shift");
  if (evenement.metaKey) morceaux.push("Super");
  return [...morceaux, bouton].join("+");
}

const NOMS_TOUCHES: Record<string, string> = { Shift: "Maj", Super: "Win", Bouton4: "Bouton 4", Bouton5: "Bouton 5", Molette: "Clic molette" };

/** Les touches d'une combinaison, telles qu'on les lit sur un clavier français. */
export function touchesLisibles(combinaison: string): string[] {
  return combinaison.split("+").map((touche) => NOMS_TOUCHES[touche] ?? touche);
}
