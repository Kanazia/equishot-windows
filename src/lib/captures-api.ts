/**
 * Pont typé entre le front et les captures (`src-tauri/src/captures/mod.rs`).
 * La commande rend sa valeur ou lève un message déjà rédigé pour l'écran.
 */
import { convertFileSrc, invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

import { errorText, isTauri } from "@/lib/tauri";
import type { Facon } from "@/modules/captures/facon";
import type { Fond, Forme, Zone } from "@/modules/captures/formes";

export { errorText, isTauri };

export interface Capture {
  id: number;
  creeLe: string;
  modifieLe: string;
  /** Chemins absolus des trois fichiers. L'original n'est jamais modifié. */
  origine: string;
  rendu: string;
  vignette: string;
  largeur: number;
  hauteur: number;
  ecran: string;
  echelle: number;
  fenetre: string;
  application: string;
  note: string;
  formes: Forme[];
  /** La partie de l'original que montre le rendu (ADR-007), `null` pour l'image entière. */
  recadrage: Zone | null;
  /** Le texte reconnu (ADR-010) : `null` pas encore lu, vide si rien n'est reconnu. */
  texte: string | null;
  /** Le fond autour du rendu (ADR-012), `null` sans fond. */
  fond: Fond | null;
  /** L'orientation (ADR-018), 0 à 7 : 0 telle que prise, 1 un quart de tour horaire, 4 le miroir horizontal. */
  orientation: number;
  /** La durée d'une vidéo (ADR-016), `null` pour une image. `origine` est alors le MP4. */
  dureeMs: number | null;
}

/** Image, texte (ADR-010) ou vidéo (ADR-016) : ce que la sélection fait de la zone. */
export type ModeSelection = "image" | "texte" | "video";

export type TypeDeclencheur = "clavier" | "souris" | "aucun";

/** Une case du raccourci : clavier (`Ctrl+Alt+S`), souris (`Alt+Bouton5`) ou rien. */
export interface EtatCase {
  type: TypeDeclencheur;
  /** Écriture canonique, celle que Rust relit. Vide pour « aucun ». */
  combinaison: string;
  defautType: TypeDeclencheur;
  defaut: string;
  etat: "enregistre" | "indisponible";
  motif: string | null;
}

/**
 * `derniere` reprend la dernière zone au lieu d'ouvrir la sélection (ADR-009) ;
 * `texte` ouvre la sélection en mode « Texte » (ADR-010), `video` en mode vidéo ou arrête la vidéo en cours (ADR-016).
 */
export type NomCase = "principal" | "secondaire" | "derniere" | "texte" | "video" | "ecran";

export type EtatRaccourci = Record<NomCase, EtatCase>;

export interface InfosSelection {
  /** Taille physique de l'écran couvert par la sélection. */
  largeur: number;
  hauteur: number;
  echelle: number;
  /** Délai entre le déclenchement et la sélection montrée, une fois montrée. */
  delaiMs: number | null;
  mode: ModeSelection;
}

export const capturesEtatRaccourci = () => invoke<EtatRaccourci>("captures_etat_raccourci");

/** L'enregistreur des réglages écoute (`true`) ou a renoncé (`false`). */
export const capturesSuspendreRaccourci = (suspendu: boolean) =>
  invoke<EtatRaccourci>("captures_suspendre_raccourci", { suspendu });

export const capturesReglerRaccourci = (casePrise: NomCase, type: TypeDeclencheur, combinaison: string) =>
  invoke<EtatRaccourci>("captures_regler_raccourci", { case: casePrise, type, combinaison });

export const capturesOuvrirSelection = () => invoke<void>("captures_ouvrir_selection");

/** Bouton « Texte » de la pastille (ADR-010). */
export const capturesOuvrirSelectionTexte = () => invoke<void>("captures_ouvrir_selection_texte");

/** Bouton « Vidéo » de la pastille (ADR-016). */
export const capturesOuvrirSelectionVideo = () => invoke<void>("captures_ouvrir_selection_video");

/** Mode « Texte » : Rust ferme la sélection, lit la zone du tracé `trace` et copie son texte. */
export const capturesSelectionTexte = (trace: number) => invoke<void>("captures_selection_texte", { trace });

/** « Copier le texte » de la galerie : refusé tant que le texte n'est pas lu, ou s'il est vide. */
export const capturesCopierTexte = (id: number) => invoke<void>("captures_copier_texte", { id });

export const capturesSelectionInfos = () => invoke<InfosSelection>("captures_selection_infos");

/**
 * Le tracé relâché : l'écran est pris maintenant, la zone revient en BMP (octets bruts).
 * `fenetre` : le rang d'une fenêtre cliquée, dont Rust prend le cadre exact (ADR-008).
 */
export const capturesSelectionPrendre = (zone: Zone, facteur: number, trace: number, fenetre?: number) =>
  invoke<ArrayBuffer>("captures_selection_prendre", { zone, facteur, trace, fenetre: fenetre ?? null });

/** Une fenêtre de l'écran de la sélection, en pixels physiques de cet écran. */
export type FenetreVue = Zone & { titre: string };

/** Les fenêtres de l'écran, de la plus haute à la plus basse. */
export const capturesSelectionFenetres = () => invoke<FenetreVue[]>("captures_selection_fenetres");

/** La loupe : x et y du curseur (i32), puis 15 x 15 pixels RGBA de l'écran vivant. */
export const capturesSelectionLoupe = () => invoke<ArrayBuffer>("captures_selection_loupe");

/** Une flèche : le curseur bouge de `dx`, `dy` pixels physiques. */
export const capturesSelectionCurseur = (dx: number, dy: number) => invoke<void>("captures_selection_curseur", { dx, dy });

export const capturesSelectionPrete = () => invoke<void>("captures_selection_prete");

/** La zone figée du tracé `trace` est écrite d'office ; faux si ce tracé est déjà remplacé (ADR-004). */
export const capturesSelectionGarder = (trace: number) => invoke<boolean>("captures_selection_garder", { trace });

/** Échap, « Fermer » : la zone déjà écrite reste, brute. */
export const capturesSelectionAnnuler = () => invoke<void>("captures_selection_annuler");

/** « Supprimer », Suppr : la zone déjà écrite part à la corbeille. */
export const capturesSelectionJeter = () => invoke<void>("captures_selection_jeter");
/** Capture défilante (ADR-015) : rend la main quand l'image recollée est écrite et la sélection fermée. */
export const capturesSelectionDefiler = (facon: Facon) => invoke<void>("captures_selection_defiler", { facon });
export const capturesSelectionArreter = () => invoke<void>("captures_selection_arreter");

/** `rendu` : le PNG annoté en base64, absent s'il n'y a aucune forme. `epingler` : une épingle se pose à la place de la zone (ADR-011). */
export const capturesSelectionEnregistrer = (formes: Forme[], rendu?: string, epingler = false) =>
  invoke<Capture>("captures_selection_enregistrer", { formes, rendu: rendu ?? null, epingler });

/** Entrée sans zone tracée : l'écran entier que la sélection couvre. */
export const capturesSelectionEcranEntier = () => invoke<Capture>("captures_selection_ecran_entier");

export const capturesEcranEntier = () => invoke<Capture>("captures_ecran_entier");

/** L'écran entier dans 3, 5 ou 10 s (ADR-009). Refusé si un autre différé court. */
export const capturesDifferer = (secondes: number) => invoke<void>("captures_differer", { secondes });
export const capturesDiffereAnnuler = () => invoke<void>("captures_differe_annuler");

export const capturesLister = (recherche: string) => invoke<Capture[]>("captures_lister", { recherche });

export const capturesNoter = (id: number, note: string) => invoke<void>("captures_noter", { id, note });

export const capturesRenommer = (id: number, titre: string) => invoke<void>("captures_renommer", { id, titre });

/** `orientation` (ADR-018) : 0 à 7, exigée. `rendu` : le PNG aplati à la taille orientée, absent sans forme. */
export const capturesModifier = (id: number, formes: Forme[], recadrage: Zone | null, fond: Fond | null, orientation: number, rendu?: string) =>
  invoke<Capture>("captures_modifier", { id, formes, recadrage, fond, orientation, rendu: rendu ?? null });

export const capturesSupprimer = (id: number) => invoke<void>("captures_supprimer", { id });

/** La pipette (ADR-013) : copie `#rrggbb` en texte, revalidé par Rust. */
export const capturesCopierCouleur = (couleur: string) => invoke<void>("captures_copier_couleur", { couleur });

export const capturesCopier = (id: number) => invoke<void>("captures_copier", { id });

/** « Copier » de la sélection : les fichiers, et leurs chemins en texte. */
export const capturesCopierPlusieurs = (ids: number[]) => invoke<void>("captures_copier_plusieurs", { ids });

export const capturesDossier = () => invoke<string>("captures_dossier");
export const capturesOuvrirDossier = () => invoke<string>("captures_ouvrir_dossier");
/** L'Explorateur, le fichier de la capture sélectionné (le MP4 d'une vidéo). */
export const capturesMontrer = (id: number) => invoke<void>("captures_montrer", { id });
/** Le clic droit sur une carte ou une vignette : le menu natif, sous le curseur. */
export const capturesMenu = (id: number) => invoke<void>("captures_menu", { id });

/** « Changer… » (ADR-020) : le dossier choisi et le nombre de fichiers qui partiraient, `null` si annulé. */
export interface Deplacement {
  cible: string;
  fichiers: number;
}
export const capturesChoisirDossier = () => invoke<Deplacement | null>("captures_choisir_dossier");
export const capturesDeplacerDossier = (cible: string) => invoke<string>("captures_deplacer_dossier", { cible });

/** La galerie a changé : une capture prise, modifiée ou supprimée. */
export function surCapturesChangees(handler: () => void): Promise<UnlistenFn> {
  return listen("captures://changee", handler);
}

/** Le raccourci a changé, depuis les réglages. */
export function surRaccourciChange(handler: (etat: EtatRaccourci) => void): Promise<UnlistenFn> {
  return listen<EtatRaccourci>("captures://raccourci", (evenement) => handler(evenement.payload));
}

/** Un échec sans page pour le dire : la pastille le montre. */
/** La sélection ouverte change de mode : l'autre raccourci a été frappé avant tout tracé. */
export function surModeSelection(handler: (mode: ModeSelection) => void): Promise<UnlistenFn> {
  return listen<ModeSelection>("captures://mode", (evenement) => handler(evenement.payload));
}

export function surErreurCapture(handler: (texte: string) => void): Promise<UnlistenFn> {
  return listen<string>("captures://erreur", (evenement) => handler(evenement.payload));
}

/** Une bonne nouvelle sans page pour la dire (« Texte copié ») : la pastille la montre au ton neutre. */
export function surInfoCapture(handler: (texte: string) => void): Promise<UnlistenFn> {
  return listen<string>("captures://info", (evenement) => handler(evenement.payload));
}

/** L'avancée de la capture défilante : `perdue` depuis une vue impossible à recoller (ADR-019). */
export type Avancee = { vues: number; longueur: number; perdue: boolean };

export function surDefile(handler: (avancee: Avancee) => void): Promise<UnlistenFn> {
  return listen<Avancee>("captures://defile", (evenement) => handler(evenement.payload));
}

/** Le compte à rebours du différé : les secondes restantes, `null` quand il s'arrête. */
export function surDiffere(handler: (reste: number | null) => void): Promise<UnlistenFn> {
  return listen<number | null>("captures://differe", (evenement) => handler(evenement.payload));
}

/** Adresse d'un fichier de capture ; `version` force le rechargement après une réédition. */
export function adresse(chemin: string, version: string): string {
  return `${convertFileSrc(chemin)}?v=${encodeURIComponent(version)}`;
}

/** La pile des vignettes (`src-tauri/src/vignettes.rs`) : les cartes de haut en bas. */
export interface Pile {
  cartes: Capture[];
  /** Vrai si la pile est au-dessus de la pastille : la plus récente en bas. */
  dessus: boolean;
}

export const vignettesLister = () => invoke<Pile>("vignettes_lister");
export const vignetteFermer = (id: number) => invoke<void>("vignette_fermer", { id });
export const vignettesToutFermer = () => invoke<void>("vignettes_tout_fermer");
/** Glisse le rendu de la capture vers une autre application ; rend la main au dépôt. */
export const vignetteGlisser = (id: number) => invoke<{ depose: boolean; dureeMs: number }>("vignette_glisser", { id });
export const vignettesActives = () => invoke<boolean>("vignettes_actives");
export const vignettesActiver = (actif: boolean) => invoke<boolean>("vignettes_activer", { actif });
export const vignettesAvecPastille = () => invoke<boolean>("vignettes_avec_pastille");
export const vignettesRangerAvecPastille = (actif: boolean) => invoke<boolean>("vignettes_ranger_avec_pastille", { actif });
/** Une épingle (`src-tauri/src/epingles.rs`), lue par l'étiquette de sa fenêtre. */
export interface InfosEpingle {
  id: number;
  rendu: string;
  version: string;
  pourcent: number;
}
export const epingleInfos = () => invoke<InfosEpingle>("epingle_infos");
/** Rend la taille obtenue, bornée par Rust à 20-500 %. `ancreX`, `ancreY` : le point qui reste sous le curseur, en fraction de la fenêtre. */
export const epingleTaille = (pourcent: number, ancreX: number, ancreY: number) => invoke<number>("epingle_taille", { pourcent, ancreX, ancreY });
export const epingleCopier = () => invoke<void>("epingle_copier");
export const epingleMenu = () => invoke<void>("epingle_menu");
export const epingleFermer = () => invoke<void>("epingle_fermer");
export const epingleOuvrir = (id: number) => invoke<void>("epingle_ouvrir", { id });
/** « Taille d'origine » du menu natif de l'épingle. */
export function surOrigineEpingle(handler: () => void): Promise<UnlistenFn> {
  return listen("epingle://origine", () => handler());
}

/** Le réglage des vignettes a changé, d'où que vienne le changement. */
export function surReglageVignettes(handler: (actives: boolean) => void): Promise<UnlistenFn> {
  return listen<boolean>("vignettes://reglage", (evenement) => handler(evenement.payload));
}

export function surPile(handler: (pile: Pile) => void): Promise<UnlistenFn> {
  return listen<Pile>("vignettes://pile", (evenement) => handler(evenement.payload));
}

export const pastilleRanger = () => invoke<void>("pastille_ranger");
/** Le choix du délai du différé, en menu surgissant sous le curseur. */
export const pastilleMenuDiffere = () => invoke<void>("pastille_menu_differe");
/** « Vidéo » de la pastille : la sélection, ou le menu « Filmer » s'il existe des scènes (0.19). */
export const pastilleFilmer = () => invoke<void>("pastille_filmer");
export const pastilleMontrer = () => invoke<void>("pastille_montrer");
export const pastilleContractee = () => invoke<boolean>("pastille_contractee");
export const pastilleContracter = (contractee: boolean) => invoke<void>("pastille_contracter", { contractee });
export const principaleOuvrir = (section: "galerie" | "reglages") => invoke<void>("principale_ouvrir", { section });

/** La galerie s'ouvre sur la fiche de la capture `id`, en réédition. */
export const principaleModifier = (id: number) => invoke<void>("principale_modifier", { id });
export const equishotQuitter = () => invoke<void>("equishot_quitter");

export {
  combinaisonCliquee,
  combinaisonFrappee,
  lireDisposition,
  MODIFICATEURS,
  touchesLisibles,
  type Disposition,
} from "@/modules/captures/frappe";

/** La vidéo (ADR-016, ADR-017). */
export interface EtatBarre {
  phase: "prepare" | "decompte" | "filme" | "pause" | "aucun";
  /** Secondes restantes du décompte. */
  reste: number;
  actifMs: number;
  avertissement: string | null;
  /** Crête de chaque source après volume, de 0 à 1 : les jauges. */
  niveaux: { micro: number; son: number };
  /** Mise en scène : une source qui n'a pas pu s'ouvrir. */
  notes: string[];
  /** Les caméras branchées, vues par la page de la bulle (0.18). */
  cameras: Camera[];
  /** Mise en scène : ce qui est filmé (0.19). */
  cible: CibleScene | null;
  /** Le zoom et le projecteur allumés (0.22), l'outil de la souris (0.24), et les raccourcis lisibles. */
  zoom: boolean;
  projecteur: boolean;
  outil: OutilSouris;
  raccourcisEffets: [string, string, string];
}

/** La bulle de caméra (0.18) : 160, 240 ou 360 pixels CSS de diamètre. */
export type TailleCamera = "petite" | "moyenne" | "grande";

/** Une caméra vue par la page de la bulle (`enumerateDevices`). */
export interface Camera {
  id: string;
  nom: string;
}

/** Ce que la page de la bulle reçoit : la caméra (`null` : celle par défaut), le miroir, le diamètre en pixels CSS. */
export interface ConfigBulle {
  cameraId: string | null;
  miroir: boolean;
  diametre: number;
}

export interface ReglagesVideo {
  micro: boolean;
  sonSysteme: boolean;
  curseur: boolean;
  /** `null` : le périphérique par défaut de Windows. */
  microId: string | null;
  sortieId: string | null;
  /** De 0 à 2 (200 %), appliqués au mélange. */
  volumeMicro: number;
  volumeSon: number;
  /** La bulle de caméra : allumée, quelle caméra (`null` : celle par défaut), sa taille, son miroir. */
  camera: boolean;
  cameraId: string | null;
  cameraTaille: TailleCamera;
  cameraMiroir: boolean;
  /** Proportion de l'espace libre de la zone (0 à 1) ; `null` : en bas à gauche. Écrite par le seul glisser. */
  cameraPlace: [number, number] | null;
  /** Les effets (0.22) : onde au clic, traînée bouton gauche tenu, curseur grossi de 1 à 3. */
  onde: boolean;
  trainee: boolean;
  curseurTaille: number;
  /** Le lot 2 et le lot 3 (0.23) : touches montrées, zoom qui vient seul au clic, curseur lissé. */
  touches: ModeTouches;
  zoomAuto: boolean;
  curseurLisse: boolean;
}

/** Les touches montrées dans la vidéo (0.23). */
export type ModeTouches = "aucune" | "raccourcis" | "toutes";

/**
 * L'outil de la souris pendant la prise (0.24) : la souris clique l'application filmée ;
 * le pointeur (encre qui s'efface) et le crayon (encre qui reste) gardent le clic.
 */
export type OutilSouris = "souris" | "pointeur" | "crayon";

/**
 * Un trait dessiné pendant la prise (0.23), en pixels physiques de la zone filmée.
 * 0.24 : l'épaisseur du pinceau à chaque point du stylo, et l'opacité du pointeur qui s'efface.
 */
export interface TraitDessin {
  outil: "stylo" | "fleche" | "rectangle";
  couleur: [number, number, number];
  epaisseur: number;
  points: [number, number][];
  largeurs?: number[];
  opacite?: number;
}

/** L'écran d'une scène (0.19) : nom Windows (`\\.\DISPLAY1`), coin et taille en pixels physiques. */
export interface EcranScene {
  nom: string;
  position: [number, number];
  taille: [number, number];
}

/** Ce que filme une scène : une zone à tracer, un écran entier, ou une zone fixe (pixels physiques, relative à l'écran). */
export type CibleScene = { genre: "aTracer" } | { genre: "ecran"; ecran: EcranScene } | { genre: "zone"; ecran: EcranScene; zone: [number, number, number, number] };

/** Ce qu'une scène remet dans les réglages : le son, la bulle et les effets, pas le curseur visible. */
export type MiseScene = Omit<ReglagesVideo, "curseur">;

export interface Scene {
  id: number;
  nom: string;
  maj: string;
  cible: CibleScene;
  mise: MiseScene;
}

/** Les scènes lisibles, et les illisibles à part (montrées dans les réglages pour être supprimées). */
export interface ListeScenes {
  scenes: Scene[];
  illisibles: { id: number; nom: string; motif: string }[];
}

export type SceneEcrite = { etat: "enregistree"; id: number } | { etat: "nomPris" };

export interface Peripherique {
  id: string;
  nom: string;
}

/** Les micros et sorties son branchés. */
export interface Peripheriques {
  micros: Peripherique[];
  sorties: Peripherique[];
  microDefaut: string | null;
  sortieDefaut: string | null;
}

/** La barre vue du curseur pendant l'enregistrement : effacée, elle laisse passer les clics. */
export type Survol = "normale" | "effacee" | "rendue";

/** Le tracé relâché, une fenêtre cliquée (`fenetre`) ou Entrée (rien) en mode vidéo : la mise en scène s'ouvre. */
export const videoDemarrer = (zone: Zone | null, facteur: number, fenetre?: number) =>
  invoke<void>("video_demarrer", { zone, facteur, fenetre: fenetre ?? null });
/** « Démarrer » ou Entrée dans la mise en scène : le décompte part. */
export const videoLancer = () => invoke<void>("video_lancer");
export const videoPeripheriques = () => invoke<Peripheriques>("video_peripheriques");
export const videoArreter = () => invoke<void>("video_arreter");
export const videoAbandonner = () => invoke<void>("video_abandonner");
export const videoPauser = (pause: boolean) => invoke<void>("video_pauser", { pause });
/** Les boutons de la barre (0.22) : bascule le zoom (`true`) ou le projecteur. */
export const videoEffet = (zoom: boolean) => invoke<void>("video_effet", { zoom });
/** L'outil de la souris (0.24) : `souris` ferme le dessin, `pointeur` et `crayon` l'ouvrent. */
export const videoOutil = (outil: OutilSouris) => invoke<void>("video_outil", { outil });
/** Le dessin à l'écran (0.23) : lui envoie ses traits. */
export const videoDessin = (traits: TraitDessin[]) => invoke<void>("video_dessin", { traits });
export const videoEtat = () => invoke<EtatBarre>("video_etat");
export const videoReglages = () => invoke<ReglagesVideo>("video_reglages");
export const videoRegler = (reglages: ReglagesVideo) => invoke<ReglagesVideo>("video_regler", { reglages });
export const videoDossier = () => invoke<string>("video_dossier");
export const videoOuvrirDossier = () => invoke<string>("video_ouvrir_dossier");
/** Le GIF d'une vidéo (0.16) : 30 s au plus, 10 i/s, 800 px de large ; écrit à côté du MP4 et copié comme fichier. */
export const videoGif = (id: number) => invoke<string>("video_gif", { id });

export function surEtatVideo(handler: (etat: EtatBarre) => void): Promise<UnlistenFn> {
  return listen<EtatBarre>("video://etat", (evenement) => handler(evenement.payload));
}

/** La page du dessin (0.24) : l'outil qui a pris la main, pointeur ou crayon. */
export function surOutilVideo(handler: (outil: OutilSouris) => void): Promise<UnlistenFn> {
  return listen<OutilSouris>("video://outil", (evenement) => handler(evenement.payload));
}

export function surSurvolVideo(handler: (survol: Survol) => void): Promise<UnlistenFn> {
  return listen<Survol>("video://survol", (evenement) => handler(evenement.payload));
}

export const videoBulleConfig = () => invoke<ConfigBulle>("video_bulle_config");
export const videoBulleCameras = (cameras: Camera[]) => invoke<void>("video_bulle_cameras", { cameras });
/** `dx`, `dy` : pixels CSS cumulés depuis l'appui ; `fini` écrit la place dans les réglages. */
export const videoBulleDeplacer = (dx: number, dy: number, fini: boolean) => invoke<void>("video_bulle_deplacer", { dx, dy, fini });
export const videoBullePerdue = (motif: string) => invoke<void>("video_bulle_perdue", { motif });

export function surBulle(handler: (config: ConfigBulle) => void): Promise<UnlistenFn> {
  return listen<ConfigBulle>("video://bulle", (evenement) => handler(evenement.payload));
}

export const videoScenes = () => invoke<ListeScenes>("video_scenes");
/** Un nom pris rend `nomPris` sans rien écrire, tant que `remplacer` est faux. */
export const videoSceneEnregistrer = (nom: string, aTracer: boolean, remplacer: boolean) => invoke<SceneEcrite>("video_scene_enregistrer", { nom, aTracer, remplacer });
export const videoSceneRenommer = (id: number, nom: string) => invoke<void>("video_scene_renommer", { id, nom });
export const videoSceneSupprimer = (id: number) => invoke<void>("video_scene_supprimer", { id });
/** Applique une scène en mise en scène ; une autre cible fixe rouvre le panneau. Rend les réglages écrits. */
export const videoSceneAppliquer = (id: number) => invoke<ReglagesVideo>("video_scene_appliquer", { id });

export function surScenes(handler: () => void): Promise<UnlistenFn> {
  return listen("video://scenes", () => handler());
}

/** « 1:42 », « 12:05 », « 1:02:03 ». */
export function duree(ms: number): string {
  const s = Math.floor(ms / 1000);
  const [h, m, sec] = [Math.floor(s / 3600), Math.floor((s % 3600) / 60), s % 60];
  const deux = (n: number) => String(n).padStart(2, "0");
  return h > 0 ? `${h}:${deux(m)}:${deux(sec)}` : `${m}:${deux(sec)}`;
}

/** Lancement avec Windows (ADR-020) : la case des Réglages, cochée d'office à l'installation. */
export const demarrageLire = () => invoke<boolean>("demarrage_lire");
export const demarrageRegler = (actif: boolean) => invoke<boolean>("demarrage_regler", { actif });

/** Mise à jour (ADR-020) : l'état de la dernière vérification, suivi par `maj://etat`. */
export interface EtatMaj {
  disponible: string | null;
  verifiee: string | null;
  erreur: string | null;
  dev: boolean;
}
export const majEtat = () => invoke<EtatMaj>("maj_etat");
export const majRechercher = () => invoke<EtatMaj>("maj_rechercher");
export const majInstaller = () => invoke<void>("maj_installer");
export function surMaj(handler: (etat: EtatMaj) => void): Promise<UnlistenFn> {
  return listen<EtatMaj>("maj://etat", (evenement) => handler(evenement.payload));
}
