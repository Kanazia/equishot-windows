import { keepPreviousData, useQuery, useQueryClient } from "@tanstack/react-query";
import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";

import { Icon } from "@/components/Icon";
import { EmptyState, ViewShell } from "@/components/ViewShell";
import { Button } from "@/components/ui/button";
import { Dialog, DialogContent, DialogDescription, DialogHeader, DialogTitle } from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  adresse,
  capturesCopier,
  capturesCopierTexte,
  capturesEtatRaccourci,
  capturesLister,
  capturesModifier,
  capturesNoter,
  capturesRenommer,
  capturesMenu,
  capturesMontrer,
  capturesSupprimer,
  duree,
  epingleOuvrir,
  errorText,
  isTauri,
  surCapturesChangees,
  videoGif,
  type Capture,
} from "@/lib/captures-api";
import { confirmer } from "@/lib/tauri";
import { cn } from "@/lib/utils";
import { Annotation, BarreAnnotation, useAnnotation, type AnnotationPoignee } from "@/modules/captures/Annotation";
import { dernierFond, PanneauFond } from "@/modules/captures/Fond";
import { composer, orienterFormes, orienterZone, tailleOrientee, tailleRendu, type Bouton } from "@/modules/captures/formes";
import { Recadrage } from "@/modules/captures/Recadrage";
import { Touches } from "@/modules/captures/Touches";

/** Clé de cache de l'état du raccourci, partagée avec la rangée des Réglages. */
export const CLE_RACCOURCI = ["captures-raccourci"] as const;

const FORMAT_DATE = new Intl.DateTimeFormat("fr-FR", {
  day: "numeric",
  month: "short",
  year: "numeric",
  hour: "2-digit",
  minute: "2-digit",
});

/** `#modifier-<id>` (crayon d'une vignette) ou `#capture-<id>` (menu d'une épingle) : la fiche à ouvrir. */
function lireFiche(): { id: number; edition: boolean } | null {
  const trouve = /^#(modifier|capture)-(\d+)$/.exec(window.location.hash);
  return trouve ? { id: Number(trouve[2]), edition: trouve[1] === "modifier" } : null;
}

function dateLisible(iso: string): string {
  const date = new Date(iso);
  return Number.isNaN(date.getTime()) ? iso : FORMAT_DATE.format(date);
}

/**
 * Module Captures (ADR-075) : la galerie des captures prises par le raccourci
 * système, la plus récente en premier, avec sa recherche et la fiche de chacune.
 */
export function CapturesView({ onReglages }: { onReglages: () => void }) {
  const queryClient = useQueryClient();
  const [recherche, setRecherche] = useState("");
  // La capture ouverte est gardée entière, pas par son rang dans la liste : une
  // note qui cesse de correspondre à la recherche ne ferme pas sa fiche.
  const [ouverte, setOuverte] = useState<{ capture: Capture; edition: boolean } | null>(null);
  const [selection, setSelection] = useState<ReadonlySet<number>>(new Set());
  const [erreurAction, setErreurAction] = useState("");

  const terme = recherche.trim();
  // Jamais tenue pour fraîche : une capture prise par le raccourci pendant
  // qu'un autre module est affiché doit être là au retour.
  const captures = useQuery({
    queryKey: ["captures", terme],
    queryFn: () => capturesLister(terme),
    enabled: isTauri(),
    staleTime: 0,
    placeholderData: keepPreviousData,
  });
  const raccourci = useQuery({ queryKey: CLE_RACCOURCI, queryFn: capturesEtatRaccourci, enabled: isTauri(), staleTime: 0 });

  useEffect(() => {
    if (!isTauri()) return;
    const ecoute = surCapturesChangees(() => void queryClient.invalidateQueries({ queryKey: ["captures"] }));
    return () => void ecoute.then((arreter) => arreter());
  }, [queryClient]);

  const liste = captures.data ?? [];

  // Le crayon d'une vignette ouvre la fenêtre sur `#modifier-<id>`, le menu d'une
  // épingle sur `#capture-<id>` : la fiche de cette capture s'ouvre, en réédition
  // pour le crayon. Lue dans la liste entière, pas celle de la recherche, qui peut la cacher.
  const [aOuvrir, setAOuvrir] = useState(lireFiche);
  useEffect(() => {
    const suivre = () => setAOuvrir(lireFiche());
    window.addEventListener("hashchange", suivre);
    return () => window.removeEventListener("hashchange", suivre);
  }, []);
  useEffect(() => {
    if (aOuvrir === null || !isTauri()) return;
    setAOuvrir(null);
    window.history.replaceState(null, "", "#galerie");
    capturesLister("").then(
      (toutes) => {
        const capture = toutes.find((c) => c.id === aOuvrir.id);
        if (capture) setOuverte({ capture, edition: aOuvrir.edition });
      },
      (e) => setErreurAction(errorText(e)),
    );
  }, [aOuvrir]);

  // La sélection ne garde que les cartes affichées : une recherche qui en cache
  // une la retire, on ne supprime jamais ce qu'on ne voit pas.
  useEffect(() => {
    setSelection((avant) => {
      const visibles = new Set(liste.map((c) => c.id));
      const apres = new Set([...avant].filter((id) => visibles.has(id)));
      return apres.size === avant.size ? avant : apres;
    });
  }, [liste]);

  const enSelection = selection.size > 0;
  const basculer = (id: number) =>
    setSelection((avant) => {
      const apres = new Set(avant);
      if (!apres.delete(id)) apres.add(id);
      return apres;
    });

  useEffect(() => {
    if (!enSelection || ouverte) return;
    const touche = (evenement: KeyboardEvent) => evenement.key === "Escape" && setSelection(new Set());
    window.addEventListener("keydown", touche);
    return () => window.removeEventListener("keydown", touche);
  }, [enSelection, ouverte]);

  /** Supprime `cibles` après une seule confirmation ; s'arrête à la première qui résiste. */
  async function supprimer(cibles: Capture[]) {
    const n = cibles.length;
    const question =
      n === 1
        ? "Supprimer cette capture ? Ses fichiers passent dans la corbeille du dossier des captures."
        : `Supprimer ces ${n} captures ? Leurs fichiers passent dans la corbeille du dossier des captures.`;
    if (!(await confirmer(question))) return;
    setErreurAction("");
    let faites = 0;
    try {
      for (const capture of cibles) {
        await capturesSupprimer(capture.id).catch((e) => {
          throw new Error(`« ${capture.fenetre || "Fenêtre sans titre"} » n'a pas pu l'être : ${errorText(e)}`);
        });
        faites++;
      }
    } catch (e) {
      setErreurAction(`${faites} supprimée${faites > 1 ? "s" : ""} sur ${n}. ${errorText(e)}`);
    } finally {
      // La liste relue retire d'elle-même les supprimées de la sélection.
      void queryClient.invalidateQueries({ queryKey: ["captures"] });
    }
  }

  const pastille =
    "grid size-7 place-items-center rounded-full border border-outline bg-surface/90 text-on-surface-variant shadow-sm transition-[opacity,color] duration-150 hover:text-on-surface";

  return (
    <ViewShell
      title="Captures"
      actions={
        <>
          <div className="relative mr-auto w-72">
            <span className="pointer-events-none absolute top-1/2 left-2 -translate-y-1/2">
              <Icon name="search" size={15} className="text-muted" />
            </span>
            <Input
              value={recherche}
              onChange={(evenement) => setRecherche(evenement.target.value)}
              placeholder="Note, fenêtre, application, date, texte"
              aria-label="Chercher dans les captures"
              data-testid="captures-recherche"
              className="h-8 pl-7 text-[12px]"
            />
          </div>
          <span className="text-[11px] text-muted tabular-nums" data-testid="captures-compteur">
            {liste.length} capture{liste.length > 1 ? "s" : ""}
          </span>
          {raccourci.data && (
            <button
              type="button"
              data-testid="captures-raccourci"
              data-etat={raccourci.data.principal.etat}
              title={raccourci.data.principal.motif ?? raccourci.data.secondaire.motif ?? "Changer le raccourci dans les Réglages"}
              className="eq-hover flex items-center gap-1.5 rounded-md px-1.5 py-1 text-[11px]"
              onClick={onReglages}
            >
              <Touches combinaison={raccourci.data.principal.combinaison} />
              {raccourci.data.principal.etat === "indisponible" && <span className="text-tertiary">indisponible</span>}
              {raccourci.data.secondaire.type !== "aucun" && (
                <>
                  <span className="text-muted">ou</span>
                  <span data-testid="captures-raccourci-secondaire" className="contents">
                    <Touches combinaison={raccourci.data.secondaire.combinaison} />
                  </span>
                  {raccourci.data.secondaire.etat === "indisponible" && <span className="text-tertiary">indisponible</span>}
                </>
              )}
            </button>
          )}
        </>
      }
    >
      {captures.isError && (
        <p role="alert" data-testid="captures-erreur" className="mb-4 rounded-lg border border-error/40 bg-error/5 px-3 py-2 text-[12px] text-error">
          {errorText(captures.error)}
        </p>
      )}

      {erreurAction && (
        <p role="alert" data-testid="galerie-erreur" className="mb-4 rounded-lg border border-error/40 bg-error/5 px-3 py-2 text-[12px] text-error">
          {erreurAction}
        </p>
      )}

      {enSelection && (
        <div
          data-testid="selection-barre"
          className="sticky top-0 z-10 mb-3 flex items-center gap-2 rounded-lg border border-outline bg-surface px-3 py-1.5 text-[12px] shadow-sm"
        >
          <span data-testid="selection-compte" className="font-medium tabular-nums">
            {selection.size} sélectionnée{selection.size > 1 ? "s" : ""}
          </span>
          <Button
            variant="ghost"
            size="sm"
            data-testid="selection-tout"
            disabled={selection.size === liste.length}
            onClick={() => setSelection(new Set(liste.map((c) => c.id)))}
          >
            Tout sélectionner
          </Button>
          <Button
            variant="ghost"
            size="sm"
            data-testid="selection-supprimer"
            className="ml-auto text-on-surface-variant hover:text-error"
            onClick={() => void supprimer(liste.filter((c) => selection.has(c.id)))}
          >
            <Icon name="delete" size={14} />
            Supprimer
          </Button>
          <Button variant="outline" size="sm" data-testid="selection-annuler" onClick={() => setSelection(new Set())}>
            Annuler
          </Button>
        </div>
      )}

      {liste.length === 0 && !captures.isLoading ? (
        terme ? (
          <EmptyState icon="search_off" title="Aucune capture ne correspond" hint="La recherche porte sur la note, la fenêtre d'origine, l'application et la date." />
        ) : (
          <EmptyState icon="screenshot_region" title="Aucune capture" hint="Le raccourci ouvre la sélection : tracez la zone, annotez, Entrée." />
        )
      ) : (
        <ul className="grid grid-cols-[repeat(auto-fill,minmax(220px,1fr))] gap-3">
          {liste.map((capture) => {
            const cochee = selection.has(capture.id);
            const titre = capture.fenetre || "Fenêtre sans titre";
            return (
              <li key={capture.id} className="group relative">
                <button
                  type="button"
                  data-testid="capture-carte"
                  data-id={capture.id}
                  data-cochee={cochee}
                  aria-pressed={enSelection ? cochee : undefined}
                  className={cn(
                    "flex w-full flex-col overflow-hidden rounded-lg border border-outline bg-surface text-left transition-[border-color,transform] duration-150 ease-out hover:border-primary/60 active:scale-[0.97]",
                    cochee && "border-primary ring-2 ring-primary/40",
                  )}
                  onClick={() => (enSelection ? basculer(capture.id) : setOuverte({ capture, edition: false }))}
                  onContextMenu={(e) => {
                    e.preventDefault();
                    capturesMenu(capture.id).catch((err) => setErreurAction(errorText(err)));
                  }}
                >
                  <span className="relative">
                    <img
                      src={adresse(capture.vignette, capture.modifieLe)}
                      alt=""
                      loading="lazy"
                      className="aspect-[16/10] w-full bg-surface-low object-contain"
                    />
                    {capture.dureeMs !== null && (
                      <span
                        data-testid="carte-duree"
                        className="absolute right-1.5 bottom-1.5 flex items-center gap-0.5 rounded bg-black/70 px-1.5 py-0.5 text-[11px] font-medium text-white tabular-nums"
                      >
                        <Icon name="videocam" size={13} />
                        {duree(capture.dureeMs)}
                      </span>
                    )}
                  </span>
                  <span className="flex flex-col gap-0.5 px-2.5 py-2">
                    <span className="truncate text-[12px] font-medium text-on-surface" title={capture.fenetre}>
                      {capture.fenetre || "Fenêtre sans titre"}
                    </span>
                    <span className="truncate text-[11px] text-muted">
                      {dateLisible(capture.creeLe)}
                      {capture.note && ` · ${capture.note}`}
                    </span>
                  </span>
                </button>
                {/* Les pastilles sont posées sur la carte, pas dedans : un bouton ne se met pas dans un bouton. */}
                <button
                  type="button"
                  role="checkbox"
                  aria-checked={cochee}
                  aria-label={`Sélectionner « ${titre} »`}
                  title={cochee ? "Retirer de la sélection" : "Sélectionner"}
                  data-testid="carte-cocher"
                  onClick={() => basculer(capture.id)}
                  className={cn(
                    pastille,
                    "absolute top-2 left-2",
                    cochee && "border-primary bg-primary text-primary-foreground hover:text-primary-foreground",
                    enSelection ? "opacity-100" : "opacity-0 group-focus-within:opacity-100 group-hover:opacity-100",
                  )}
                >
                  <Icon name="check" size={16} className={cn(!cochee && "opacity-40")} />
                </button>
                {!enSelection && (
                  <span className="absolute top-2 right-2 flex gap-1 opacity-0 transition-opacity duration-150 group-focus-within:opacity-100 group-hover:opacity-100">
                    {capture.dureeMs === null && (
                    <button
                      type="button"
                      data-testid="carte-modifier"
                      title="Modifier l'annotation"
                      aria-label={`Modifier l'annotation de « ${titre} »`}
                      className={pastille}
                      onClick={() => setOuverte({ capture, edition: true })}
                    >
                      <Icon name="draw" size={15} />
                    </button>
                    )}
                    <button
                      type="button"
                      data-testid="carte-supprimer"
                      title="Supprimer"
                      aria-label={`Supprimer « ${titre} »`}
                      className={cn(pastille, "hover:text-error")}
                      onClick={() => void supprimer([capture])}
                    >
                      <Icon name="delete" size={15} />
                    </button>
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}

      <Dialog open={ouverte !== null} onOpenChange={(ouvert) => !ouvert && setOuverte(null)}>
        {ouverte && (
          <Fiche key={ouverte.capture.id} initiale={ouverte.capture} editionInitiale={ouverte.edition} onFermer={() => setOuverte(null)} />
        )}
      </Dialog>
    </ViewShell>
  );
}

/** La fiche d'une capture : image annotée, note, origine, et ses quatre gestes. */
function Fiche({ initiale, editionInitiale = false, onFermer }: { initiale: Capture; editionInitiale?: boolean; onFermer: () => void }) {
  const queryClient = useQueryClient();
  // Remplacée par ce que rend la réédition : c'est elle qui date l'image à recharger.
  const [capture, setCapture] = useState(initiale);
  const [note, setNote] = useState(capture.note);
  const [retour, setRetour] = useState("");
  const [erreur, setErreur] = useState("");
  // Une vidéo ne s'annote pas (ADR-016) : le crayon d'une carte vidéo n'ouvre que la fiche.
  const [edition, setEdition] = useState(editionInitiale && initiale.dureeMs === null);
  const video = capture.dureeMs !== null;
  const [gifEnCours, setGifEnCours] = useState(false);
  const minuteur = useRef<number | undefined>(undefined);
  const derniere = useRef(capture.note);
  // Le titre : celui de la fenêtre d'origine, changé sur place. Entrée ou un clic
  // ailleurs l'enregistre, Échap le remet tel qu'il était.
  const [titre, setTitre] = useState(capture.fenetre);
  const titreEnregistre = useRef(capture.fenetre);
  const saisieTitre = useRef(titre);
  saisieTitre.current = titre;

  const rafraichir = () => void queryClient.invalidateQueries({ queryKey: ["captures"] });

  const dire = (message: string) => {
    setRetour(message);
    window.setTimeout(() => setRetour((courant) => (courant === message ? "" : courant)), 2000);
  };

  async function tenter(geste: () => Promise<unknown>, fait?: string) {
    setErreur("");
    try {
      await geste();
      if (fait) dire(fait);
    } catch (e) {
      setErreur(errorText(e));
    }
  }

  // La note s'enregistre à la saisie, sans bouton : une demi-seconde après la
  // dernière frappe, ou tout de suite quand le champ perd la main.
  function enregistrerNote(texte: string) {
    window.clearTimeout(minuteur.current);
    if (texte === derniere.current) return;
    derniere.current = texte;
    void tenter(() => capturesNoter(capture.id, texte).then(rafraichir), "Note enregistrée");
  }

  function enregistrerTitre() {
    const propre = saisieTitre.current.trim();
    setTitre(propre);
    if (propre === titreEnregistre.current) return;
    titreEnregistre.current = propre;
    void tenter(() => capturesRenommer(capture.id, propre).then(rafraichir), "Titre enregistré");
  }

  // Une fiche fermée dans la demi-seconde qui suit la dernière frappe
  // enregistre quand même sa note : rien de tapé ne se perd.
  const saisie = useRef(note);
  saisie.current = note;
  useEffect(
    () => () => {
      window.clearTimeout(minuteur.current);
      if (saisie.current !== derniere.current) {
        void capturesNoter(initiale.id, saisie.current).then(rafraichir, () => undefined);
      }
    },
    // eslint-disable-next-line react-hooks/exhaustive-deps
    [],
  );

  return (
    <DialogContent data-testid="capture-fiche" className="flex max-h-[92vh] max-w-5xl flex-col gap-3 sm:max-w-5xl">
      <DialogHeader>
        <DialogTitle className="pr-8 text-[14px]">
          <input
            value={titre}
            onChange={(evenement) => setTitre(evenement.target.value)}
            onBlur={enregistrerTitre}
            onKeyDown={(evenement) => {
              if (evenement.key === "Enter") {
                evenement.preventDefault();
                evenement.currentTarget.blur();
              } else if (evenement.key === "Escape") {
                saisieTitre.current = titreEnregistre.current;
                setTitre(titreEnregistre.current);
              }
            }}
            placeholder="Fenêtre sans titre"
            aria-label="Titre de la capture"
            title="Cliquer pour renommer"
            data-testid="capture-titre"
            className="-mx-1.5 w-full truncate rounded-md border border-transparent bg-transparent px-1.5 py-0.5 font-medium text-on-surface outline-none transition-colors placeholder:text-muted-foreground hover:border-input focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50"
          />
        </DialogTitle>
        <DialogDescription className="text-[11px] text-muted">
          {/* Windows nomme ses écrans `\\.\DISPLAY1` : le préfixe ne dit rien. */}
          {/* La taille du rendu : le cadre (ADR-007) et les marges du fond (ADR-012). */}
          {[
            capture.application,
            dateLisible(capture.creeLe),
            capture.dureeMs !== null && `Vidéo de ${duree(capture.dureeMs)}`,
            `${tailleRendu(capture).largeur} × ${tailleRendu(capture).hauteur} px`,
            capture.ecran.replace(/^\\\\\.\\/, ""),
          ]
            .filter(Boolean)
            .join(" · ")}
        </DialogDescription>
      </DialogHeader>

      {edition ? (
        <Reedition
          capture={capture}
          onFini={(modifiee) => {
            setEdition(false);
            if (modifiee) {
              setCapture(modifiee);
              rafraichir();
              dire("Annotation enregistrée");
            }
          }}
        />
      ) : (
        <>
          <div className="flex min-h-0 justify-center overflow-auto rounded-lg border border-outline bg-surface-low">
            {video ? (
              <video
                src={adresse(capture.origine, capture.modifieLe)}
                poster={adresse(capture.rendu, capture.modifieLe)}
                controls
                preload="metadata"
                data-testid="capture-video"
                className="max-h-[58vh] max-w-full"
              />
            ) : (
              <img
                src={adresse(capture.rendu, capture.modifieLe)}
                alt={`Capture de ${capture.fenetre || "l'écran"}`}
                data-testid="capture-image"
                className="max-h-[58vh] max-w-full object-contain"
              />
            )}
          </div>
          <Textarea
            value={note}
            onChange={(evenement) => {
              const texte = evenement.target.value;
              setNote(texte);
              window.clearTimeout(minuteur.current);
              minuteur.current = window.setTimeout(() => enregistrerNote(texte), 500);
            }}
            onBlur={() => enregistrerNote(note)}
            placeholder="Note"
            aria-label="Note de la capture"
            data-testid="capture-note"
            className="min-h-16 text-[12px]"
          />
          <div className="flex flex-wrap items-center gap-2">
            <Button
              size="sm"
              data-testid="capture-copier"
              title={video ? "Copier le fichier : Ctrl+V le dépose dans une messagerie ou un dossier" : undefined}
              onClick={() => void tenter(() => capturesCopier(capture.id), video ? "Fichier copié" : "Copiée")}
            >
              <Icon name="content_copy" size={14} />
              {video ? "Copier le fichier" : "Copier"}
            </Button>
            {video && (
              <Button
                variant="outline"
                size="sm"
                data-testid="capture-gif"
                disabled={gifEnCours}
                title="Les 30 premières secondes au plus, 800 px de large : le GIF est copié comme fichier"
                onClick={async () => {
                  setGifEnCours(true);
                  await tenter(() => videoGif(capture.id), "GIF copié comme fichier");
                  setGifEnCours(false);
                }}
              >
                <Icon name={gifEnCours ? "progress_activity" : "gif_box"} size={14} className={gifEnCours ? "animate-spin" : undefined} />
                {gifEnCours ? "GIF en cours" : "Enregistrer en GIF"}
              </Button>
            )}
            {!video && (
              <>
            {/* La fiche garde la capture de son ouverture : sur `null`, le texte a pu être lu
                depuis, Rust répond au clic avec la base. Grisé seulement quand on sait qu'il n'y a rien (ADR-010). */}
            <Button
              variant="outline"
              size="sm"
              data-testid="copier-texte"
              disabled={capture.texte === ""}
              title={capture.texte === "" ? "Aucun texte reconnu" : capture.texte === null ? "Le texte est peut-être encore en lecture" : undefined}
              onClick={() => void tenter(() => capturesCopierTexte(capture.id), "Texte copié")}
            >
              <Icon name="text_fields" size={14} />
              Copier le texte
            </Button>
            <Button variant="outline" size="sm" data-testid="capture-epingler" onClick={() => void tenter(() => epingleOuvrir(capture.id))}>
              <Icon name="push_pin" size={14} />
              Épingler
            </Button>
              </>
            )}
            <Button variant="outline" size="sm" data-testid="capture-dossier" onClick={() => void tenter(() => capturesMontrer(capture.id))}>
              <Icon name="folder_open" size={14} />
              Ouvrir le dossier
            </Button>
            {!video && (
              <Button variant="outline" size="sm" data-testid="capture-modifier" onClick={() => setEdition(true)}>
                <Icon name="draw" size={14} />
                Modifier l'annotation
              </Button>
            )}
            <span className="text-[11px] text-primary" data-testid="capture-retour" aria-live="polite">
              {retour}
            </span>
            <Button
              variant="ghost"
              size="sm"
              data-testid="capture-supprimer"
              className="ml-auto text-on-surface-variant hover:text-error"
              onClick={async () => {
                const quoi = video ? "cette vidéo ? Ses fichiers passent dans la corbeille du dossier des vidéos." : "cette capture ? Ses fichiers passent dans la corbeille du dossier des captures.";
                if (!(await confirmer(`Supprimer ${quoi}`))) return;
                await tenter(() => capturesSupprimer(capture.id).then(onFermer));
              }}
            >
              <Icon name="delete" size={14} />
              Supprimer
            </Button>
          </div>
        </>
      )}
      {erreur && (
        <p role="alert" data-testid="capture-erreur" className="text-[11px] text-error">
          {erreur}
        </p>
      )}
    </DialogContent>
  );
}

/** Les trois gestes d'orientation (ADR-018), dans l'ordre de la barre. Un seul pivot, horaire : trois clics pour l'autre sens. */
const ORIENTATIONS: { bouton: Bouton; libelle: string; titre: string; icone: string; testid: string; classe?: string }[] = [
  { bouton: "pivoter", libelle: "Pivoter", titre: "Pivoter d'un quart de tour (sens horaire)", icone: "rotate_90_degrees_cw", testid: "reedition-pivoter" },
  { bouton: "horizontal", libelle: "Retourner horizontalement", titre: "Retourner horizontalement", icone: "flip", testid: "reedition-retourner-h" },
  // ponytail: le même glyphe tourné d'un quart, pas d'icône propre au miroir vertical.
  { bouton: "vertical", libelle: "Retourner verticalement", titre: "Retourner verticalement", icone: "flip", testid: "reedition-retourner-v", classe: "rotate-90" },
];

/**
 * Réédition des formes et du recadrage sur l'original entier, jamais sur le
 * rendu : rien ne s'empile, et un cadre se desserre (ADR-007).
 */
function Reedition({ capture, onFini }: { capture: Capture; onFini: (modifiee: Capture | null) => void }) {
  const [image, setImage] = useState<HTMLImageElement | null>(null);
  const edition = useAnnotation(capture.formes);
  const { formes } = edition;
  const [recadrage, setRecadrage] = useState(capture.recadrage);
  const [recadrer, setRecadrer] = useState(false);
  const [fond, setFond] = useState(capture.fond);
  const [panneauFond, setPanneauFond] = useState(false);
  // L'orientation affichée (ADR-018) : un état, comme le recadrage et le fond ; rien n'est écrit avant « Enregistrer ».
  const [orientation, setOrientation] = useState(capture.orientation);
  // La taille affichée remplace celle de l'original partout dans la réédition : un pivot échange les côtés.
  const taille = tailleOrientee(capture, orientation);
  const echelle = capture.echelle || 1;
  const [sourceApercu, setSourceApercu] = useState<CanvasImageSource | null>(null);
  const [erreur, setErreur] = useState("");
  const [envoi, setEnvoi] = useState(false);
  const poignee = useRef<AnnotationPoignee>(null);

  const orienter = (bouton: Bouton) => {
    // Dans cet ordre : les formes et le cadre dans l'espace affiché d'avant le clic, puis l'état.
    // La saisie de texte en cours a déjà posé son texte (onBlur du champ, avant le clic).
    edition.remanier((formes) => orienterFormes(formes, bouton, taille, echelle, poignee.current?.mesurer));
    setRecadrage((r) => r && orienterZone(r, bouton, taille));
    setOrientation((o) => composer(o, bouton));
    // Le mode recadrage se ferme ; le cadre reste, transformé. Le panneau du fond ne bouge pas.
    setRecadrer(false);
  };

  // L'aperçu du fond montre les formes : l'image annotée, aplatie quand elles changent.
  useEffect(() => {
    if (!panneauFond || !image) return;
    // Sans forme mais orientée, l'original non orienté ne ferait pas l'affaire : l'aplati, toujours.
    const png = formes.length > 0 || orientation !== 0 ? poignee.current?.aplatir() : undefined;
    if (!png) {
      setSourceApercu(image);
      return;
    }
    let vivant = true;
    const annotee = new Image();
    annotee.src = `data:image/png;base64,${png}`;
    annotee.decode().then(
      () => vivant && setSourceApercu(annotee),
      () => vivant && setSourceApercu(image),
    );
    return () => {
      vivant = false;
    };
  }, [panneauFond, image, formes, orientation]);

  useEffect(() => {
    let vivant = true;
    const origine = new Image();
    // Sans quoi le canevas, sali par une image d'une autre origine, ne s'aplatit plus.
    origine.crossOrigin = "anonymous";
    origine.src = convertFileSrc(capture.origine);
    origine.decode().then(
      () => vivant && setImage(origine),
      () => vivant && setErreur("L'image d'origine est illisible."),
    );
    return () => {
      vivant = false;
    };
  }, [capture.origine]);

  // En mode recadrage, Entrée et Échap quittent le mode : ni la fiche ne se ferme,
  // ni le bouton qui a la main ne se rappuie. En phase de capture, avant la boîte de dialogue.
  useEffect(() => {
    if (!recadrer) return;
    function surTouche(evenement: KeyboardEvent) {
      if (evenement.key !== "Enter" && evenement.key !== "Escape") return;
      evenement.preventDefault();
      evenement.stopImmediatePropagation();
      setRecadrer(false);
    }
    window.addEventListener("keydown", surTouche, true);
    return () => window.removeEventListener("keydown", surTouche, true);
  }, [recadrer]);

  async function enregistrer() {
    setEnvoi(true);
    setErreur("");
    try {
      // Le rendu aplati est à la taille orientée (ADR-018) : Rust le contrôle, le rogne, puis le pose sur son fond.
      onFini(await capturesModifier(capture.id, formes, recadrage, fond, orientation, formes.length > 0 ? poignee.current?.aplatir() : undefined));
    } catch (e) {
      setErreur(errorText(e));
      setEnvoi(false);
    }
  }

  const ratio = taille.largeur / taille.hauteur;
  return (
    <div className="flex min-h-0 flex-col items-center gap-3" data-testid="capture-reedition">
      <div className="flex min-h-0 w-full justify-center overflow-auto rounded-lg border border-outline bg-surface-low">
        {image && (
          <div
            className="relative"
            style={{
              aspectRatio: `${taille.largeur} / ${taille.hauteur}`,
              width: `min(100%, calc(58vh * ${ratio}), ${taille.largeur / echelle}px)`,
            }}
          >
            <Annotation
              ref={poignee}
              image={image}
              zone={{ x: 0, y: 0, largeur: taille.largeur, hauteur: taille.hauteur }}
              echelle={echelle}
              edition={edition}
              orientation={orientation}
              className="size-full"
            />
            <Recadrage recadrage={recadrage} image={taille} actif={recadrer} echelle={echelle} onChange={setRecadrage} />
          </div>
        )}
      </div>
      <BarreAnnotation edition={edition}>
        <Button
          variant="ghost"
          size="icon"
          title={recadrer ? "Finir le recadrage (Entrée)" : "Recadrer"}
          aria-label="Recadrer"
          aria-pressed={recadrer}
          data-testid="reedition-recadrer"
          className={recadrer ? "bg-surface-highest text-primary" : "text-on-surface-variant"}
          onClick={() => {
            edition.choisirForme(null);
            setRecadrer((r) => !r);
          }}
        >
          <Icon name="crop" size={16} />
        </Button>
        {recadrer && recadrage && (
          <Button
            variant="ghost"
            size="icon"
            title="Image entière"
            aria-label="Image entière"
            data-testid="reedition-image-entiere"
            className="text-on-surface-variant"
            onClick={() => setRecadrage(null)}
          >
            <Icon name="crop_free" size={16} />
          </Button>
        )}
        {ORIENTATIONS.map((o) => (
          <Button
            key={o.bouton}
            variant="ghost"
            size="icon"
            title={o.titre}
            aria-label={o.libelle}
            data-testid={o.testid}
            className="text-on-surface-variant"
            onClick={() => orienter(o.bouton)}
          >
            <Icon name={o.icone} size={16} className={o.classe} />
          </Button>
        ))}
        <Button
          variant="ghost"
          size="icon"
          title="Fond"
          aria-label="Fond"
          aria-pressed={panneauFond}
          data-testid="reedition-fond"
          className={panneauFond || fond ? "bg-surface-highest text-primary" : "text-on-surface-variant"}
          onClick={() => {
            // Un clic sans fond pose le dernier fond choisi : le résultat se voit tout de suite.
            if (!panneauFond && fond === null) setFond(dernierFond());
            setRecadrer(false);
            setPanneauFond((p) => !p);
          }}
        >
          <Icon name="wallpaper" size={16} />
        </Button>
        <span className="mx-0.5 h-4 w-px bg-outline" />
        <Button variant="ghost" size="sm" data-testid="reedition-annuler" onClick={() => onFini(null)}>
          Annuler
        </Button>
        <Button size="sm" data-testid="reedition-enregistrer" disabled={envoi || !image} onClick={() => void enregistrer()}>
          Enregistrer
        </Button>
      </BarreAnnotation>
      {panneauFond && (
        <PanneauFond
          fond={fond}
          onChange={setFond}
          source={sourceApercu}
          cadre={recadrage ?? { x: 0, y: 0, largeur: taille.largeur, hauteur: taille.hauteur }}
        />
      )}
      {erreur && (
        <p role="alert" className="text-[11px] text-error">
          {erreur}
        </p>
      )}
    </div>
  );
}
