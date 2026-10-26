/**
 * Le panneau de mise en scène (ADR-017) : la même fenêtre que la barre, en grand
 * et focalisable, avant le décompte. Le micro et la sortie son branchés, leur
 * volume et leur jauge, la caméra, les effets (0.22) ; « Démarrer » ou Entrée
 * lance, Échap abandonne. Chaque
 * choix s'écrit aussitôt : il vaut pour cette vidéo et les suivantes.
 */
import { useEffect, useRef, useState } from "react";

import { Icon } from "@/components/Icon";
import {
  errorText,
  surScenes,
  videoAbandonner,
  videoLancer,
  videoPeripheriques,
  videoReglages,
  videoRegler,
  videoSceneAppliquer,
  videoSceneEnregistrer,
  videoScenes,
  type EtatBarre,
  type Peripheriques,
  type ReglagesVideo,
  type Scene,
  type ModeTouches,
  type TailleCamera,
} from "@/lib/captures-api";
import { cn } from "@/lib/utils";

import { choixAffiche, choixLu, fileDeUn, libelleCible, nomDeScene, pourcent, REPLI_CAMERA, REPLI_SORTIE, toucheDuPanneau } from "./mise-en-scene";

/** Les listes sont relues toutes les 2 s : un casque branché apparaît sans rouvrir. */
const RELECTURE_MS = 2000;

/** Les grossissements du curseur (0.22) : 1x, Windows le dessine ; au-delà, le moteur. */
const CURSEURS: [number, string][] = [
  [1, "1x"],
  [1.5, "1,5x"],
  [2, "2x"],
  [3, "3x"],
];

/** Les touches montrées dans la vidéo (0.23). */
const TOUCHES: [ModeTouches, string, string][] = [
  ["aucune", "Aucune", "Aucune touche dans la vidéo."],
  ["raccourcis", "Raccourcis", "Les touches tenues avec Ctrl, Alt ou Win, et F1 à F24."],
  ["toutes", "Toutes", "Tout ce qui est tapé, mots de passe compris."],
];

/** Les trois diamètres de la bulle de caméra (0.18). */
const TAILLES: [TailleCamera, string][] = [
  ["petite", "Petite"],
  ["moyenne", "Moyenne"],
  ["grande", "Grande"],
];

function Source(props: {
  titre: string;
  testid: string;
  liste: Peripheriques["micros"];
  defaut: string | null;
  valeur: string;
  volume: number;
  niveau: number;
  onChoix: (valeur: string) => void;
  onVolume: (volume: number) => void;
}) {
  const nomDefaut = props.liste.find((p) => p.id === props.defaut)?.nom;
  const coupe = props.valeur === "aucun";
  return (
    <div className="flex flex-col gap-1.5">
      <label className="text-[11px] font-medium text-muted" htmlFor={props.testid}>
        {props.titre}
      </label>
      <select
        id={props.testid}
        data-testid={props.testid}
        value={props.valeur}
        onChange={(e) => props.onChoix(e.target.value)}
        className="h-8 w-full truncate rounded-md border border-outline bg-surface-high px-2 text-[13px] text-on-surface"
      >
        <option value="aucun">Aucun</option>
        <option value="defaut">{nomDefaut ? `Par défaut (${nomDefaut})` : "Par défaut"}</option>
        {props.liste.map((p) => (
          <option key={p.id} value={p.id}>
            {p.nom}
          </option>
        ))}
      </select>
      <div className={cn("flex items-center gap-2", coupe && "opacity-40")}>
        <input
          type="range"
          min={0}
          max={200}
          step={5}
          disabled={coupe}
          aria-label={`Volume, ${props.titre.toLowerCase()}`}
          data-testid={`${props.testid.replace("prep-", "prep-volume-")}`}
          value={Math.round(props.volume * 100)}
          onChange={(e) => props.onVolume(Number(e.target.value) / 100)}
          className="min-w-0 flex-1 accent-primary"
        />
        <span className="w-12 shrink-0 text-right text-[11px] text-muted tabular-nums">{pourcent(props.volume)}</span>
      </div>
      <div className="h-1.5 overflow-hidden rounded-full bg-surface-high" aria-hidden="true">
        <div
          data-testid={`${props.testid.replace("prep-", "prep-jauge-")}`}
          data-niveau={props.niveau.toFixed(3)}
          className="h-full rounded-full bg-primary transition-[width] duration-75"
          style={{ width: `${Math.round(props.niveau * 100)}%` }}
        />
      </div>
    </div>
  );
}

export function Preparation({ etat }: { etat: EtatBarre }) {
  const [reglages, setReglages] = useState<ReglagesVideo | null>(null);
  const [liste, setListe] = useState<Peripheriques>({ micros: [], sorties: [], microDefaut: null, sortieDefaut: null });
  const [erreur, setErreur] = useState("");
  const [envoi, setEnvoi] = useState(false);
  const [ecriture] = useState(() => fileDeUn(videoRegler, (e) => setErreur(errorText(e))));
  // Le lancement relit les réglages en base : il attend la dernière écriture.
  const lancer = () => ecriture.fini().then(videoLancer);

  useEffect(() => {
    videoReglages().then(setReglages, (e) => setErreur(errorText(e)));
    const relire = () => void videoPeripheriques().then(setListe, (e) => setErreur(errorText(e)));
    relire();
    const minuteur = window.setInterval(relire, RELECTURE_MS);
    return () => window.clearInterval(minuteur);
  }, []);

  const [scenes, setScenes] = useState<Scene[]>([]);
  /** « Enregistrer comme scène » ouvert : le nom, la cible gardée, le nom déjà pris. */
  const [formulaire, setFormulaire] = useState<{ nom: string; aTracer: boolean; pris: boolean } | null>(null);

  useEffect(() => {
    // Deux relectures rapprochées : seule la dernière demandée s'affiche.
    let derniere = 0;
    const relire = () => {
      const numero = ++derniere;
      void videoScenes().then((l) => numero === derniere && setScenes(l.scenes), (e) => setErreur(errorText(e)));
    };
    relire();
    const ecoute = surScenes(relire);
    return () => void ecoute.then((arreter) => arreter());
  }, []);

  async function geste(action: () => Promise<void>) {
    if (envoi) return;
    setEnvoi(true);
    setErreur("");
    try {
      await action();
    } catch (e) {
      setErreur(errorText(e));
      setEnvoi(false);
    }
  }

  async function scene(action: () => Promise<void>) {
    setErreur("");
    try {
      await action();
    } catch (e) {
      setErreur(errorText(e));
    }
  }

  /** Vrai pendant qu'une scène s'applique : un geste partirait avec les anciens réglages. */
  const applique = useRef(false);

  /** Une écriture en vol écraserait la scène : on l'attend. */
  async function appliquerScene(id: number) {
    applique.current = true;
    try {
      await ecriture.fini();
      setReglages(await videoSceneAppliquer(id));
    } finally {
      applique.current = false;
    }
  }

  async function enregistrerScene(remplacer: boolean) {
    if (!formulaire) return;
    const lu = nomDeScene(formulaire.nom);
    if ("erreur" in lu) throw new Error(lu.erreur);
    const ecrite = await ecriture.fini().then(() => videoSceneEnregistrer(lu.nom, formulaire.aTracer, remplacer));
    setFormulaire(ecrite.etat === "nomPris" ? { ...formulaire, pris: true } : null);
  }

  useEffect(() => {
    function touche(e: KeyboardEvent) {
      const action = toucheDuPanneau(e.key, formulaire !== null);
      if (!action) return;
      e.preventDefault();
      if (action === "lancer") void geste(lancer);
      else if (action === "abandonner") void geste(videoAbandonner);
      // Entrée n'est jamais « Remplacer » : celui-là se clique.
      else if (action === "valider") void scene(() => enregistrerScene(false));
      else setFormulaire(null);
    }
    window.addEventListener("keydown", touche);
    return () => window.removeEventListener("keydown", touche);
  });

  /**
   * Application immédiate. La réponse n'est pas reprise : pendant qu'on glisse
   * un curseur, une réponse en retard le ferait sauter en arrière.
   */
  function changer(partiel: Partial<ReglagesVideo>) {
    if (!reglages || applique.current) return;
    const voulus = { ...reglages, ...partiel };
    setReglages(voulus);
    ecriture.pousser(voulus);
  }

  const micro = reglages ? choixAffiche(reglages.micro, reglages.microId, liste.micros) : null;
  const son = reglages ? choixAffiche(reglages.sonSysteme, reglages.sortieId, liste.sorties, REPLI_SORTIE) : null;
  const camera = reglages ? choixAffiche(reglages.camera, reglages.cameraId, etat.cameras, REPLI_CAMERA) : null;
  // Une liste encore vide (première lecture en cours) ne crie pas au débranchement.
  // ponytail: la liste des caméras vient de la bulle ; caméra à « Aucune », pas de bulle,
  // donc pas de liste : le panneau ne propose alors que « Aucune » et « Caméra par défaut ».
  const notes = [liste.micros.length ? micro?.note : null, liste.sorties.length ? son?.note : null, etat.cameras.length ? camera?.note : null, ...etat.notes].filter(Boolean);
  const message = erreur || notes.join(" ");

  return (
    <div className="flex h-screen w-screen p-0.5">
      <div data-testid="barre-video" data-phase="prepare" className="flex h-full w-full flex-col gap-3 rounded-2xl border border-outline bg-surface/95 p-4 shadow-sm">
        <p className="text-[13px] font-medium text-on-surface">Mise en scène</p>
        <div className="flex flex-col gap-1.5">
          <label className="text-[11px] font-medium text-muted" htmlFor={formulaire ? "prep-scene-nom" : "prep-scene"}>
            Scène
          </label>
          {formulaire ? (
            <>
              <div className="flex items-center gap-1.5">
                <input
                  id="prep-scene-nom"
                  data-testid="prep-scene-nom"
                  autoFocus
                  maxLength={40}
                  placeholder="Nom de la scène"
                  value={formulaire.nom}
                  onChange={(e) => setFormulaire({ ...formulaire, nom: e.target.value, pris: false })}
                  className="h-8 min-w-0 flex-1 rounded-md border border-outline bg-surface-high px-2 text-[13px] text-on-surface"
                />
                <select
                  data-testid="prep-scene-cible"
                  aria-label="Ce que la scène filme"
                  value={formulaire.aTracer ? "aTracer" : "actuelle"}
                  onChange={(e) => setFormulaire({ ...formulaire, aTracer: e.target.value === "aTracer" })}
                  className="h-8 rounded-md border border-outline bg-surface-high px-2 text-[13px] text-on-surface"
                >
                  <option value="actuelle">{libelleCible(etat.cible)}</option>
                  <option value="aTracer">À tracer à chaque fois</option>
                </select>
              </div>
              <div className="flex items-center justify-end gap-1.5">
                {formulaire.pris && (
                  <span data-testid="prep-scene-pris" className="mr-auto truncate text-[11px] text-on-surface">
                    « {formulaire.nom.trim()} » existe déjà.
                  </span>
                )}
                <button type="button" data-testid="prep-scene-annuler" onClick={() => setFormulaire(null)} className="eq-hover h-7 rounded-full px-2.5 text-[11px] font-medium text-on-surface">
                  Annuler
                </button>
                {formulaire.pris ? (
                  <button type="button" data-testid="prep-scene-remplacer" onClick={() => void scene(() => enregistrerScene(true))} className="h-7 rounded-full bg-primary px-2.5 text-[11px] font-medium text-primary-foreground">
                    Remplacer
                  </button>
                ) : (
                  <button type="button" data-testid="prep-scene-valider" onClick={() => void scene(() => enregistrerScene(false))} className="h-7 rounded-full bg-primary px-2.5 text-[11px] font-medium text-primary-foreground">
                    Enregistrer
                  </button>
                )}
              </div>
            </>
          ) : (
            <div className="flex items-center gap-1.5">
              <select
                id="prep-scene"
                data-testid="prep-scene"
                value=""
                disabled={scenes.length === 0}
                onChange={(e) => {
                  const id = Number(e.target.value);
                  if (id) void scene(() => appliquerScene(id));
                }}
                className="h-8 min-w-0 flex-1 truncate rounded-md border border-outline bg-surface-high px-2 text-[13px] text-on-surface disabled:opacity-50"
              >
                <option value="">{scenes.length ? "Appliquer une scène…" : "Aucune scène enregistrée"}</option>
                {scenes.map((s) => (
                  <option key={s.id} value={s.id}>
                    {s.nom}
                  </option>
                ))}
              </select>
              <button
                type="button"
                data-testid="prep-scene-enregistrer"
                onClick={() => setFormulaire({ nom: "", aTracer: false, pris: false })}
                className="eq-hover h-8 shrink-0 rounded-full px-2.5 text-[11px] font-medium text-on-surface"
              >
                Enregistrer comme scène
              </button>
            </div>
          )}
        </div>
        {reglages && micro && son && camera ? (
          <>
            <Source
              titre="Micro"
              testid="prep-micro"
              liste={liste.micros}
              defaut={liste.microDefaut}
              valeur={micro.valeur}
              volume={reglages.volumeMicro}
              niveau={etat.niveaux.micro}
              onChoix={(v) => {
                const { actif, id } = choixLu(v);
                changer({ micro: actif, microId: id });
              }}
              onVolume={(v) => changer({ volumeMicro: v })}
            />
            <Source
              titre="Son de l'ordinateur"
              testid="prep-son"
              liste={liste.sorties}
              defaut={liste.sortieDefaut}
              valeur={son.valeur}
              volume={reglages.volumeSon}
              niveau={etat.niveaux.son}
              onChoix={(v) => {
                const { actif, id } = choixLu(v);
                changer({ sonSysteme: actif, sortieId: id });
              }}
              onVolume={(v) => changer({ volumeSon: v })}
            />
            <div className="flex flex-col gap-1.5">
              <label className="text-[11px] font-medium text-muted" htmlFor="prep-camera">
                Caméra
              </label>
              <select
                id="prep-camera"
                data-testid="prep-camera"
                value={camera.valeur}
                onChange={(e) => {
                  const { actif, id } = choixLu(e.target.value);
                  changer({ camera: actif, cameraId: id });
                }}
                className="h-8 w-full truncate rounded-md border border-outline bg-surface-high px-2 text-[13px] text-on-surface"
              >
                <option value="aucun">Aucune</option>
                <option value="defaut">Caméra par défaut</option>
                {etat.cameras.map((c) => (
                  <option key={c.id} value={c.id}>
                    {c.nom}
                  </option>
                ))}
              </select>
              <div className={cn("flex items-center gap-1.5", !reglages.camera && "opacity-40")}>
                {TAILLES.map(([taille, libelle]) => (
                  <button
                    type="button"
                    key={taille}
                    data-testid={`prep-camera-${taille}`}
                    aria-pressed={reglages.cameraTaille === taille}
                    disabled={!reglages.camera}
                    onClick={() => changer({ cameraTaille: taille })}
                    className={cn("h-7 rounded-full px-2.5 text-[11px] font-medium", reglages.cameraTaille === taille ? "bg-primary text-primary-foreground" : "eq-hover text-on-surface")}
                  >
                    {libelle}
                  </button>
                ))}
                <label className="ml-auto flex items-center gap-1.5 text-[11px] text-on-surface">
                  <input
                    type="checkbox"
                    data-testid="prep-camera-miroir"
                    disabled={!reglages.camera}
                    checked={reglages.cameraMiroir}
                    onChange={(e) => changer({ cameraMiroir: e.target.checked })}
                    className="accent-primary"
                  />
                  Miroir
                </label>
              </div>
            </div>
            <div className="flex flex-col gap-1.5" role="group" aria-labelledby="prep-effets-titre">
              <p id="prep-effets-titre" className="text-[11px] font-medium text-muted">
                Effets
              </p>
              <div className="flex items-center gap-3">
                <label className="flex items-center gap-1.5 text-[11px] text-on-surface">
                  <input type="checkbox" data-testid="prep-onde" checked={reglages.onde} onChange={(e) => changer({ onde: e.target.checked })} className="accent-primary" />
                  Onde au clic
                </label>
                <label className="flex items-center gap-1.5 text-[11px] text-on-surface">
                  <input type="checkbox" data-testid="prep-trainee" checked={reglages.trainee} onChange={(e) => changer({ trainee: e.target.checked })} className="accent-primary" />
                  Traînée
                </label>
                <div
                  className={cn("ml-auto flex items-center gap-1", !reglages.curseur && "opacity-40")}
                  title={reglages.curseur ? undefined : "Le curseur est caché dans les réglages."}
                >
                  <span className="mr-0.5 text-[11px] text-on-surface">Curseur</span>
                  {CURSEURS.map(([taille, libelle]) => (
                    <button
                      type="button"
                      key={taille}
                      data-testid={`prep-curseur-${libelle.replace(",", "-")}`}
                      aria-pressed={reglages.curseurTaille === taille}
                      disabled={!reglages.curseur}
                      onClick={() => changer({ curseurTaille: taille })}
                      className={cn("h-7 rounded-full px-2 text-[11px] font-medium tabular-nums", reglages.curseurTaille === taille ? "bg-primary text-primary-foreground" : "eq-hover text-on-surface")}
                    >
                      {libelle}
                    </button>
                  ))}
                </div>
              </div>
              <div className="flex flex-wrap items-center gap-x-3 gap-y-1.5">
                <div className="flex items-center gap-1" role="group" aria-label="Touches">
                  <span className="mr-0.5 text-[11px] text-on-surface">Touches</span>
                  {TOUCHES.map(([mode, libelle, aide]) => (
                    <button
                      type="button"
                      key={mode}
                      data-testid={`prep-touches-${mode}`}
                      aria-pressed={reglages.touches === mode}
                      title={aide}
                      onClick={() => changer({ touches: mode })}
                      className={cn("h-7 rounded-full px-2 text-[11px] font-medium", reglages.touches === mode ? "bg-primary text-primary-foreground" : "eq-hover text-on-surface")}
                    >
                      {libelle}
                    </button>
                  ))}
                </div>
                <label className="flex items-center gap-1.5 text-[11px] text-on-surface" title="Le zoom vient seul au clic et repart 2,5 s après le dernier clic.">
                  <input type="checkbox" data-testid="prep-zoom-auto" checked={reglages.zoomAuto} onChange={(e) => changer({ zoomAuto: e.target.checked })} className="accent-primary" />
                  Zoom au clic
                </label>
                <label
                  className={cn("flex items-center gap-1.5 text-[11px] text-on-surface", !reglages.curseur && "opacity-40")}
                  title={reglages.curseur ? "Le curseur suit la souris avec douceur dans la vidéo." : "Le curseur est caché dans les réglages."}
                >
                  <input
                    type="checkbox"
                    data-testid="prep-curseur-lisse"
                    disabled={!reglages.curseur}
                    checked={reglages.curseurLisse}
                    onChange={(e) => changer({ curseurLisse: e.target.checked })}
                    className="accent-primary"
                  />
                  Curseur lissé
                </label>
              </div>
              {etat.raccourcisEffets[0] && (
                <p data-testid="prep-effets-raccourcis" className="text-[11px] text-muted">
                  Pendant la prise : zoom {etat.raccourcisEffets[0]}, projecteur {etat.raccourcisEffets[1]}, dessin {etat.raccourcisEffets[2]}.
                </p>
              )}
            </div>
          </>
        ) : (
          <p className="flex-1 text-[11px] text-muted">Lecture des périphériques…</p>
        )}
        <p data-testid="prep-note" role={erreur ? "alert" : "status"} className={cn("min-h-4 text-[11px]", erreur ? "text-error" : "text-muted")}>
          {message}
        </p>
        <div className="mt-auto flex items-center justify-end gap-2">
          <button
            type="button"
            data-testid="prep-abandonner"
            disabled={envoi}
            onClick={() => void geste(videoAbandonner)}
            className="eq-hover flex h-9 items-center rounded-full px-3 text-[13px] font-medium text-on-surface disabled:opacity-50"
          >
            Abandonner
          </button>
          <button
            type="button"
            data-testid="prep-demarrer"
            disabled={envoi}
            onClick={() => void geste(lancer)}
            className="flex h-9 items-center gap-1.5 rounded-full bg-primary px-4 text-[13px] font-medium text-primary-foreground transition-[transform] duration-150 active:scale-[0.97] disabled:opacity-50"
          >
            <Icon name="fiber_manual_record" size={17} filled />
            Démarrer
          </button>
        </div>
      </div>
    </div>
  );
}
