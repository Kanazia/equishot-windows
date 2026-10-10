/**
 * Fenêtre de sélection : transparente et protégée, par-dessus l'écran vivant.
 * On trace la zone ; au relâchement, l'écran est pris et la zone s'affiche
 * figée dans son cadre pour être annotée, le reste continue de vivre. La zone
 * figée est écrite d'office (ADR-004) ; Entrée y pose les formes et ferme
 * (l'écran entier si rien n'est tracé), Échap ferme en gardant la zone brute,
 * Suppr la jette.
 */
import { useEffect, useLayoutEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";

import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "material-symbols/outlined.css";

import { Icon } from "@/components/Icon";
import { Button } from "@/components/ui/button";
import {
  capturesSelectionAnnuler,
  capturesSelectionArreter,
  capturesSelectionDefiler,
  capturesSelectionEcranEntier,
  capturesCopierCouleur,
  capturesSelectionCurseur,
  capturesSelectionEnregistrer,
  capturesSelectionFenetres,
  capturesSelectionGarder,
  capturesSelectionTexte,
  capturesSelectionInfos,
  capturesSelectionJeter,
  capturesSelectionPrendre,
  capturesSelectionPrete,
  errorText,
  surDefile,
  surModeSelection,
  videoDemarrer,
  type FenetreVue,
  type ModeSelection,
} from "@/lib/captures-api";
import { Annotation, BarreAnnotation, useAnnotation, type AnnotationPoignee } from "@/modules/captures/Annotation";
import { CLE_FACON, FACONS, LIBELLES, lireFacon, type Facon } from "@/modules/captures/facon";
import { fenetreSous, versCss, zonePhysique, type Zone } from "@/modules/captures/formes";
import "@/index.css";

import { Loupe } from "./Loupe";

/** Hauteur de la barre d'outils et son écart à la zone, en pixels CSS. */
const BARRE = 46;
/** Hauteur de la palette des couleurs et des tailles, ouverte sous la barre ou au-dessus. */
const PALETTE = 90;
const ECART = 8;

/** Le pas de chaque flèche, en pixels physiques. */
const FLECHES: Record<string, [number, number]> = { ArrowLeft: [-1, 0], ArrowRight: [1, 0], ArrowUp: [0, -1], ArrowDown: [0, 1] };

function entre(a: { x: number; y: number }, b: { x: number; y: number }): Zone {
  return { x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), largeur: Math.abs(b.x - a.x), hauteur: Math.abs(b.y - a.y) };
}

function Selection() {
  /** Taille physique de l'écran couvert, et son échelle. */
  const [ecran, setEcran] = useState<{ largeur: number; hauteur: number; echelle: number; mode: ModeSelection } | null>(null);
  const [erreur, setErreur] = useState("");
  const [zone, setZone] = useState<Zone | null>(null);
  /** La zone prise au relâchement, et le tracé qui l'a demandée. */
  const [prise, setPrise] = useState<HTMLImageElement | null>(null);
  const edition = useAnnotation();
  const { formes } = edition;
  /** Largeur mesurée de la barre, pour la garder dans l'écran. */
  const barre = useRef<HTMLDivElement>(null);
  const [largeurBarre, setLargeurBarre] = useState(0);
  const [envoi, setEnvoi] = useState(false);
  /** La zone figée est écrite (ADR-004). */
  const [gardee, setGardee] = useState(false);
  /** Les écritures d'office, l'une après l'autre ; fermer ou terminer les attend. */
  const garde = useRef<Promise<unknown>>(Promise.resolve());
  const depart = useRef<{ x: number; y: number } | null>(null);
  const annotation = useRef<AnnotationPoignee>(null);
  /** Numéro du tracé : une image prise pour un tracé déjà remplacé est jetée. */
  const trace = useRef(0);
  /** Les fenêtres de l'écran, de la plus haute à la plus basse (ADR-008). */
  const [fenetres, setFenetres] = useState<FenetreVue[]>([]);
  /** Le curseur, en pixels CSS : la souris, ou Rust par la loupe. */
  const [point, setPoint] = useState<{ x: number; y: number } | null>(null);
  /** Rang de la fenêtre prise au clic, dont la zone est le cadre exact. */
  const [cliquee, setCliquee] = useState<number | null>(null);
  /** L'appui est parti de l'écran vivant : un clic sans glisser y prend une fenêtre. */
  const depuisVivante = useRef(false);
  /** La couleur du pixel visé, lue par la loupe ; celle que C vient de copier (ADR-013). */
  const couleurVisee = useRef<string | null>(null);
  const [copiee, setCopiee] = useState<string | null>(null);
  /** La capture défilante en cours (ADR-015, ADR-019) : vues recollées, longueur atteinte, vue perdue. */
  const [defile, setDefile] = useState<{ vues: number; longueur: number; perdue: boolean; facon: Facon } | null>(null);
  /** La dernière façon de défiler choisie, celle du bouton (ADR-019). */
  const [facon, setFacon] = useState<Facon>(() => {
    try {
      return lireFacon(localStorage.getItem(CLE_FACON));
    } catch {
      return "bas";
    }
  });
  const [menuDefile, setMenuDefile] = useState(false);
  const menuRef = useRef<HTMLDivElement>(null);
  /** La zone est filmée (ADR-016) : il ne reste que son contour, traversé par la souris. */
  const [filme, setFilme] = useState(false);

  useEffect(() => {
    void (async () => {
      try {
        const infos = await capturesSelectionInfos();
        setEcran(infos);
        capturesSelectionFenetres().then(setFenetres, (e) => setErreur(errorText(e)));
      } catch (e) {
        setErreur(errorText(e));
      }
      // Montrée aussi en cas d'échec : l'erreur se lit, Échap ferme.
      capturesSelectionPrete().catch(() => void capturesSelectionAnnuler());
    })();
  }, []);

  useEffect(() => {
    const arret = surModeSelection((mode) => setEcran((e) => (e ? { ...e, mode } : e)));
    return () => void arret.then((f) => f());
  }, []);

  useEffect(() => {
    const arret = surDefile((avancee) => setDefile((d) => (d ? { ...d, ...avancee } : d)));
    return () => void arret.then((f) => f());
  }, []);

  // Le menu de « Défiler » se referme sur un appui hors de lui.
  useEffect(() => {
    if (!menuDefile) return;
    function surAppui(evenement: PointerEvent) {
      if (!menuRef.current?.contains(evenement.target as Node)) setMenuDefile(false);
    }
    window.addEventListener("pointerdown", surAppui, true);
    return () => window.removeEventListener("pointerdown", surAppui, true);
  }, [menuDefile]);

  // Pixels physiques par pixel CSS de cette fenêtre, qui couvre tout l'écran.
  const facteur = ecran ? ecran.largeur / window.innerWidth : window.devicePixelRatio;
  const physique =
    cliquee !== null ? (fenetres[cliquee] ?? null) : ecran && zone ? zonePhysique(zone, facteur, { largeur: ecran.largeur, hauteur: ecran.hauteur }) : null;
  const cadres = fenetres.map((f) => versCss(f, facteur));

  async function prendre(finale: Zone, fenetre?: number) {
    const numero = ++trace.current;
    try {
      const octets = await capturesSelectionPrendre(finale, facteur, numero, fenetre);
      // Mode « Texte » (ADR-010) : rien à figer ni à annoter, Rust ferme la
      // sélection, lit la zone et copie son texte. Il le dit par une réponse
      // vide, ce qui tient même si le mode change pendant la prise.
      if (octets.byteLength === 0) {
        if (numero !== trace.current) return;
        garde.current = garde.current.then(() => capturesSelectionTexte(numero)).catch((e) => {
          if (numero === trace.current) setErreur(errorText(e));
        });
        return;
      }
      const adresse = URL.createObjectURL(new Blob([octets], { type: "image/bmp" }));
      const image = new Image();
      image.src = adresse;
      await image.decode();
      if (numero !== trace.current) {
        URL.revokeObjectURL(adresse);
        return;
      }
      setPrise(image);
      garde.current = garde.current.then(() => capturesSelectionGarder(numero)).then(
        (ecrite) => {
          if (ecrite && numero === trace.current) setGardee(true);
        },
        (e) => {
          if (numero === trace.current) setErreur(errorText(e));
        },
      );
    } catch (e) {
      if (numero === trace.current) setErreur(errorText(e));
    }
  }

  /** Mode vidéo : la zone, la fenêtre cliquée ou l'écran entier (sans zone) est filmé ; Rust ouvre la barre. */
  async function filmer(finale: Zone | null, fenetre?: number) {
    if (envoi) return;
    setEnvoi(true);
    setErreur("");
    try {
      await videoDemarrer(finale, facteur, fenetre);
      setFilme(true);
    } catch (e) {
      setErreur(errorText(e));
      setZone(null);
      setCliquee(null);
      setEnvoi(false);
    }
  }

  async function enregistrer(epingler = false) {
    if (envoi) return;
    // Rien de tracé : l'écran entier. Un tracé dont l'image n'est pas encore là attend.
    if (!zone) {
      if (ecran?.mode === "video") return void filmer(null);
      // ponytail: en mode « Texte », l'écran entier lu n'est pas demandé.
      if (ecran?.mode === "texte") return;
      setEnvoi(true);
      setErreur("");
      try {
        await capturesSelectionEcranEntier();
      } catch (e) {
        setErreur(errorText(e));
        setEnvoi(false);
      }
      return;
    }
    if (!prise) return;
    setEnvoi(true);
    setErreur("");
    try {
      await garde.current;
      const rendu = formes.length > 0 ? annotation.current?.aplatir() : undefined;
      // Rust ferme la fenêtre une fois les fichiers écrits.
      await capturesSelectionEnregistrer(formes, rendu, epingler);
    } catch (e) {
      setErreur(errorText(e));
      setEnvoi(false);
    }
  }

  /** La page défile sous la zone et se recolle ; Rust ferme la sélection à la fin. */
  async function defiler(choisie: Facon) {
    setMenuDefile(false);
    if (envoi || !prise) return;
    setFacon(choisie);
    try {
      localStorage.setItem(CLE_FACON, choisie);
    } catch {
      // ponytail: sans stockage, le choix vaut pour cette sélection seulement.
    }
    setEnvoi(true);
    setErreur("");
    setDefile({ vues: 1, longueur: choisie === "droite" ? prise.naturalWidth : prise.naturalHeight, perdue: false, facon: choisie });
    try {
      await garde.current;
      await capturesSelectionDefiler(choisie);
    } catch (e) {
      setErreur(errorText(e));
      setDefile(null);
      setEnvoi(false);
    }
  }

  async function fermer() {
    await garde.current;
    await capturesSelectionAnnuler();
  }

  async function jeter() {
    if (envoi) return;
    setEnvoi(true);
    try {
      await garde.current;
      await capturesSelectionJeter();
    } catch (e) {
      setErreur(errorText(e));
      setEnvoi(false);
    }
  }

  // Lus par l'écouteur du clavier, posé une seule fois. Ctrl+Z, Ctrl+Y et les
  // touches d'une forme choisie passent d'abord par l'annotation (ADR-006).
  const gestes = useRef({ enregistrer, fermer, jeter, gardee, figee: false, defile: false, filme: false, menu: false });

  useEffect(() => {
    function surTouche(evenement: KeyboardEvent) {
      // Pendant une vidéo, la barre décide : la sélection n'est qu'un contour.
      if (gestes.current.filme) return;
      if (gestes.current.defile) {
        // Pendant le défilement, Entrée et Échap l'arrêtent ; le reste ne fait rien.
        evenement.preventDefault();
        if (evenement.key === "Escape" || evenement.key === "Enter") capturesSelectionArreter().catch((e) => setErreur(errorText(e)));
        return;
      }
      if (evenement.key === "Escape" && gestes.current.menu) {
        // Échap referme d'abord le menu de « Défiler », la sélection ensuite.
        evenement.preventDefault();
        setMenuDefile(false);
        return;
      }
      if (evenement.key === "Escape") {
        evenement.preventDefault();
        void gestes.current.fermer();
      } else if (evenement.key === "Delete" && gestes.current.gardee) {
        evenement.preventDefault();
        void gestes.current.jeter();
      } else if (evenement.key === "Enter") {
        evenement.preventDefault();
        void gestes.current.enregistrer();
      } else if (FLECHES[evenement.key] && !gestes.current.figee) {
        // Le vrai curseur bouge d'un pixel physique, de dix avec Maj : le tracé le suit (ADR-008).
        evenement.preventDefault();
        const [dx, dy] = FLECHES[evenement.key];
        const pas = evenement.shiftKey ? 10 : 1;
        capturesSelectionCurseur(dx * pas, dy * pas).catch((e) => setErreur(errorText(e)));
      } else if (evenement.key.toLowerCase() === "c" && !evenement.ctrlKey && !evenement.altKey && !evenement.metaKey && !gestes.current.figee) {
        // La pipette : la couleur du pixel sous le curseur, en texte.
        const couleur = couleurVisee.current;
        if (!couleur) return;
        evenement.preventDefault();
        capturesCopierCouleur(couleur).then(
          () => {
            setCopiee(couleur);
            window.setTimeout(() => setCopiee((c) => (c === couleur ? null : c)), 1500);
          },
          (e) => setErreur(errorText(e)),
        );
      }
    }
    window.addEventListener("keydown", surTouche);
    return () => window.removeEventListener("keydown", surTouche);
  }, []);

  // La barre se colle sous la zone, au-dessus si le bas de l'écran manque, dedans sinon.
  let hautBarre = 0;
  if (zone) {
    hautBarre = zone.y + zone.hauteur + ECART;
    if (hautBarre + BARRE > window.innerHeight) hautBarre = zone.y - ECART - BARRE;
    if (hautBarre < 0) hautBarre = zone.y + zone.hauteur - ECART - BARRE;
  }
  const figee = zone !== null && prise !== null && depart.current === null;
  gestes.current = { enregistrer, fermer, jeter, gardee, figee, defile: defile !== null, filme, menu: menuDefile };
  const vivante = zone === null && depart.current === null;
  const survol = vivante && point ? fenetreSous(cadres, point) : null;

  // La barre grandit avec ses outils : sa largeur se mesure, elle ne se suppose pas.
  useLayoutEffect(() => {
    const l = barre.current?.offsetWidth ?? 0;
    if (l !== largeurBarre) setLargeurBarre(l);
  });

  return (
    <div
      data-testid="selection"
      data-etat={figee ? "figee" : zone ? "trace" : "vivante"}
      className="fixed inset-0 cursor-crosshair overflow-hidden select-none"
      onPointerDown={(evenement) => {
        if (evenement.button !== 0 || !ecran || envoi) return;
        depuisVivante.current = vivante;
        trace.current += 1;
        depart.current = { x: evenement.clientX, y: evenement.clientY };
        setZone(null);
        setPrise(null);
        setGardee(false);
        setCliquee(null);
        edition.vider();
        evenement.currentTarget.setPointerCapture(evenement.pointerId);
      }}
      onPointerMove={(evenement) => {
        setPoint({ x: evenement.clientX, y: evenement.clientY });
        if (depart.current) setZone(entre(depart.current, { x: evenement.clientX, y: evenement.clientY }));
      }}
      onPointerUp={(evenement) => {
        if (!depart.current) return;
        const finale = entre(depart.current, { x: evenement.clientX, y: evenement.clientY });
        depart.current = null;
        // Un clic sans glisser prend la fenêtre sous lui, s'il part de l'écran vivant.
        const rang = depuisVivante.current ? fenetreSous(cadres, { x: evenement.clientX, y: evenement.clientY }) : null;
        const video = ecran?.mode === "video";
        if (finale.largeur >= 4 && finale.hauteur >= 4) {
          setZone(finale);
          void (video ? filmer(finale) : prendre(finale));
        } else if (rang !== null) {
          setCliquee(rang);
          setZone(cadres[rang]);
          void (video ? filmer(cadres[rang], rang) : prendre(cadres[rang], rang));
        } else {
          setZone(null);
        }
      }}
    >
      {/* Voile très léger, jamais vide : il garde la souris sur toute la fenêtre.
          Sur tout l'écran pendant le tracé, hors de la zone une fois l'image prise. */}
      {!figee && !filme && <div data-testid="selection-voile" className="pointer-events-none absolute inset-0 bg-black/15" />}
      {survol !== null && (
        <div
          data-testid="selection-fenetre"
          data-cadre={`${fenetres[survol].x},${fenetres[survol].y},${fenetres[survol].largeur},${fenetres[survol].hauteur}`}
          className="pointer-events-none absolute bg-primary/10 outline outline-2 -outline-offset-2 outline-primary"
          style={{ left: cadres[survol].x, top: cadres[survol].y, width: cadres[survol].largeur, height: cadres[survol].hauteur }}
        >
          <span className="absolute top-2 left-2 max-w-[calc(100%-16px)] truncate rounded bg-surface px-1.5 py-0.5 text-[12px] text-on-surface">
            {fenetres[survol].titre || "Fenêtre sans titre"}
          </span>
        </div>
      )}
      {zone && (
        <div
          data-testid={filme ? "selection-video" : "selection-zone"}
          className={`absolute outline ${filme || defile?.perdue ? "outline-2 outline-error" : "outline-1 outline-primary"} ${figee && !defile ? "shadow-[0_0_0_200vmax_rgba(0,0,0,0.15)]" : ""}`}
          style={{ left: zone.x, top: zone.y, width: zone.largeur, height: zone.hauteur }}
        >
          {/* Pendant le défilement, la page vit sous la zone : plus d'image figée. */}
          {defile && (
            <span
              data-testid="selection-defile"
              data-vues={defile.vues}
              data-longueur={defile.longueur}
              data-perdue={defile.perdue}
              data-facon={defile.facon}
              className={`pointer-events-none absolute left-0 rounded bg-surface px-1.5 py-0.5 text-[12px] whitespace-nowrap tabular-nums ${defile.perdue ? "text-error" : "text-on-surface"} ${zone.y >= 26 ? "-top-6" : "top-1"}`}
            >
              Défilement : {defile.longueur.toLocaleString("fr-FR")} px.{" "}
              {defile.perdue
                ? "Revenez un peu en arrière."
                : defile.facon === "manuel"
                  ? "Faites défiler, puis Entrée pour finir."
                  : "Entrée ou Échap pour arrêter."}
            </span>
          )}
          {figee && prise && physique && !defile && (
            <Annotation
              ref={annotation}
              image={prise}
              zone={{ x: 0, y: 0, largeur: prise.naturalWidth, hauteur: prise.naturalHeight }}
              echelle={ecran?.echelle ?? 1}
              edition={edition}
              className="size-full"
            />
          )}
          {physique && !defile && !filme && (
            <span
              data-testid="selection-dimensions"
              className={`pointer-events-none absolute left-0 rounded bg-surface px-1.5 py-0.5 font-mono text-[11px] text-on-surface tabular-nums ${zone.y >= 26 ? "-top-6" : "top-1"}`}
            >
              {physique.largeur} × {physique.hauteur}
            </span>
          )}
        </div>
      )}

      {zone && figee && !defile && (
        <BarreAnnotation
          ref={barre}
          edition={edition}
          palette={hautBarre + BARRE + PALETTE < window.innerHeight ? "bas" : "haut"}
          className="absolute cursor-default"
          style={{ top: hautBarre, left: Math.max(ECART, Math.min(zone.x, window.innerWidth - largeurBarre - ECART)) }}
        >
          <span data-testid="selection-gardee" data-gardee={gardee} className="px-1 text-[12px] whitespace-nowrap text-on-surface-variant">
            {gardee ? "Enregistrée" : "Enregistrement…"}
          </span>
          <Button variant="ghost" data-testid="selection-supprimer" className="h-9 px-3 text-[13px]" disabled={!gardee || envoi} onClick={() => void jeter()}>
            Supprimer
          </Button>
          <Button variant="ghost" data-testid="selection-annuler" className="h-9 px-3 text-[13px]" onClick={() => void fermer()}>
            Fermer
          </Button>
          <div ref={menuRef} className="relative flex items-center">
            <Button
              variant="ghost"
              data-testid="selection-defiler"
              className="h-9 rounded-r-none pr-2 pl-3 text-[13px]"
              title={formes.length > 0 ? "Retirez les formes pour faire défiler" : `Faire défiler la page sous la zone et recoller : ${LIBELLES[facon].toLowerCase()}`}
              disabled={!gardee || envoi || formes.length > 0}
              onClick={() => void defiler(facon)}
            >
              Défiler
            </Button>
            <Button
              variant="ghost"
              data-testid="selection-defiler-menu"
              className="h-9 rounded-l-none px-1 text-[13px]"
              aria-label="Choisir comment défiler"
              aria-haspopup="menu"
              aria-expanded={menuDefile}
              disabled={!gardee || envoi || formes.length > 0}
              onClick={() => setMenuDefile((m) => !m)}
            >
              <Icon name="expand_more" size={18} />
            </Button>
            {menuDefile && (
              <div
                role="menu"
                aria-label="Comment défiler"
                className={`absolute right-0 z-10 flex min-w-56 flex-col rounded-lg border border-outline bg-surface p-1 shadow-lg shadow-black/40 ${
                  hautBarre + BARRE + PALETTE < window.innerHeight ? "top-full mt-2" : "bottom-full mb-2"
                }`}
              >
                {FACONS.map((f) => (
                  <button
                    key={f}
                    type="button"
                    role="menuitemradio"
                    aria-checked={f === facon}
                    data-testid={`selection-defiler-${f}`}
                    className={`flex h-9 items-center gap-2 rounded px-2 text-left text-[13px] hover:bg-surface-highest ${f === facon ? "text-primary" : "text-on-surface"}`}
                    onClick={() => void defiler(f)}
                  >
                    <Icon name="check" size={16} className={f === facon ? "" : "invisible"} />
                    {LIBELLES[f]}
                  </button>
                ))}
              </div>
            )}
          </div>
          <Button
            variant="ghost"
            data-testid="selection-epingler"
            className="h-9 px-3 text-[13px]"
            title="Enregistrer et garder l'image devant les fenêtres"
            disabled={envoi}
            onClick={() => void enregistrer(true)}
          >
            Épingler
          </Button>
          <Button data-testid="selection-enregistrer" className="h-9 px-4 text-[13px]" disabled={envoi} onClick={() => void enregistrer()}>
            Terminer
          </Button>
        </BarreAnnotation>
      )}

      {ecran && !figee && !filme && (
        <Loupe
          point={point}
          onCurseur={(c) => setPoint({ x: c.x / facteur, y: c.y / facteur })}
          onCouleur={(c) => {
            couleurVisee.current = c;
          }}
          copiee={copiee}
        />
      )}

      {ecran?.mode === "video" && !zone && !erreur && !filme && (
        <p
          data-testid="selection-mode-video"
          className="pointer-events-none absolute top-4 left-1/2 -translate-x-1/2 rounded-lg bg-surface px-3 py-2 text-[12px] text-on-surface"
        >
          Vidéo : tracez la zone, cliquez une fenêtre, ou Entrée pour l'écran entier
        </p>
      )}

      {ecran?.mode === "texte" && !zone && !erreur && (
        <p
          data-testid="selection-mode-texte"
          className="pointer-events-none absolute top-4 left-1/2 -translate-x-1/2 rounded-lg bg-surface px-3 py-2 text-[12px] text-on-surface"
        >
          Texte : tracez la zone à lire
        </p>
      )}

      {erreur && (
        <p
          role="alert"
          data-testid="selection-erreur"
          className="absolute top-4 left-1/2 max-w-xl -translate-x-1/2 cursor-default rounded-lg border border-error/40 bg-surface px-3 py-2 text-[12px] text-error"
        >
          {erreur} Échap pour fermer.
        </p>
      )}
    </div>
  );
}

const racine = document.getElementById("root");
if (!racine) {
  throw new Error("Élément racine introuvable dans selection.html.");
}
// Sans StrictMode : son double montage demanderait deux fois la sélection.
ReactDOM.createRoot(racine).render(<Selection />);
