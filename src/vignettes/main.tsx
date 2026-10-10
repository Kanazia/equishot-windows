/**
 * La pile des vignettes, accrochée à la pastille. Un clic sur une carte remet
 * l'image annotée dans le presse-papiers ; la glisser emporte son fichier vers
 * une autre application. La fenêtre n'est pas focalisable (Rust) : un clic ici
 * laisse le premier plan là où il est.
 */
import { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";

import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "material-symbols/outlined.css";

import { Icon } from "@/components/Icon";
import {
  adresse,
  capturesCopier,
  duree,
  capturesSupprimer,
  errorText,
  principaleModifier,
  principaleOuvrir,
  surPile,
  vignetteFermer,
  vignetteGlisser,
  vignettesLister,
  vignettesToutFermer,
  type Capture,
  type Pile,
} from "@/lib/captures-api";
import { cn } from "@/lib/utils";
import "@/index.css";

// Géométrie de `src-tauri/src/vignettes.rs`, en pixels CSS : la fenêtre a la
// hauteur MARGE + n x (CARTE + ECART) + BANDE.
const CARTE = 132;
const ECART = 8;
const MARGE = 8;
const BANDE = 32;
/** Au-delà de ce déplacement, l'appui devient un glisser. */
const SEUIL_GLISSER = 4;

function Carte({ capture, dessus }: { capture: Capture; dessus: boolean }) {
  const [copie, setCopie] = useState(false);
  const [erreur, setErreur] = useState("");
  const depart = useRef<{ x: number; y: number } | null>(null);
  const glisse = useRef(false);

  function copier() {
    setErreur("");
    capturesCopier(capture.id).then(() => {
      setCopie(true);
      window.setTimeout(() => setCopie(false), 1500);
    }, (e) => setErreur(errorText(e)));
  }

  // Les boutons de la carte ne lancent ni copie ni glisser.
  const bouton = (e: React.PointerEvent) => e.stopPropagation();
  const outil = "grid size-7 place-items-center rounded-full bg-surface/90 text-on-surface shadow-sm hover:bg-surface-high";

  return (
    <div style={{ height: CARTE + ECART, padding: dessus ? `${ECART}px 4px 0` : `0 4px ${ECART}px` }}>
      <div
        data-testid="vignette-carte"
        data-id={capture.id}
        data-pressable
        title="Cliquer pour copier, glisser vers une application"
        className="group/carte eq-entree relative h-full cursor-grab overflow-hidden rounded-lg border border-outline bg-surface shadow-md select-none"
        onPointerDown={(e) => {
          if (e.button !== 0) return;
          depart.current = { x: e.screenX, y: e.screenY };
          glisse.current = false;
        }}
        onPointerMove={(e) => {
          const d = depart.current;
          if (!d || glisse.current || (e.buttons & 1) === 0) return;
          if (Math.hypot(e.screenX - d.x, e.screenY - d.y) <= SEUIL_GLISSER) return;
          glisse.current = true;
          setErreur("");
          vignetteGlisser(capture.id)
            .catch((err) => setErreur(errorText(err)))
            .finally(() => {
              depart.current = null;
            });
        }}
        onPointerUp={() => {
          if (depart.current && !glisse.current) copier();
          depart.current = null;
        }}
      >
        <img
          data-testid="vignette-image"
          src={adresse(capture.vignette, capture.modifieLe)}
          alt={capture.fenetre || "Capture"}
          draggable={false}
          className="pointer-events-none size-full object-contain"
        />
        {capture.dureeMs !== null && (
          <span
            data-testid="vignette-duree"
            className="pointer-events-none absolute top-1.5 left-1/2 flex -translate-x-1/2 items-center gap-0.5 rounded bg-black/70 px-1.5 py-0.5 text-[11px] font-medium text-white tabular-nums"
          >
            <Icon name="videocam" size={13} />
            {duree(capture.dureeMs)}
          </span>
        )}
        <button
          type="button"
          data-testid="vignette-fermer"
          title="Fermer"
          aria-label="Fermer la vignette"
          onPointerDown={bouton}
          onClick={() => void vignetteFermer(capture.id).catch((e) => setErreur(errorText(e)))}
          className={cn(outil, "absolute top-1.5 right-1.5 opacity-0 transition-opacity duration-150 group-hover/carte:opacity-100")}
        >
          <Icon name="close" size={16} />
        </button>
        {/* ponytail: sans confirmation, les fichiers vont à la corbeille du dossier des captures. */}
        <button
          type="button"
          data-testid="vignette-supprimer"
          title="Supprimer la capture"
          aria-label="Supprimer la capture"
          onPointerDown={bouton}
          onClick={() => void capturesSupprimer(capture.id).catch((e) => setErreur(errorText(e)))}
          className={cn(outil, "absolute top-1.5 left-1.5 opacity-0 transition-opacity duration-150 group-hover/carte:opacity-100 hover:text-error")}
        >
          <Icon name="delete" size={15} />
        </button>
        <div className="absolute inset-x-1.5 bottom-1.5 flex justify-center gap-1.5 opacity-0 transition-opacity duration-150 group-hover/carte:opacity-100">
          <button
            type="button"
            data-testid="vignette-copier"
            onPointerDown={bouton}
            onClick={copier}
            className="flex h-7 items-center gap-1 rounded-full bg-surface/90 px-2.5 text-[11px] font-medium text-on-surface shadow-sm hover:bg-surface-high"
          >
            <Icon name="content_copy" size={14} />
            Copier
          </button>
          {capture.dureeMs === null && (
          <button
            type="button"
            data-testid="vignette-modifier"
            title="Modifier l'annotation"
            aria-label="Modifier l'annotation"
            onPointerDown={bouton}
            onClick={() => void principaleModifier(capture.id).catch((e) => setErreur(errorText(e)))}
            className={outil}
          >
            <Icon name="draw" size={15} />
          </button>
          )}
          <button
            type="button"
            data-testid="vignette-galerie"
            title="Ouvrir dans la galerie"
            aria-label="Ouvrir dans la galerie"
            onPointerDown={bouton}
            onClick={() => void principaleOuvrir("galerie").catch((e) => setErreur(errorText(e)))}
            className={outil}
          >
            <Icon name="photo_library" size={15} />
          </button>
        </div>
        {copie && (
          <span
            data-testid="vignette-copie"
            role="status"
            className="eq-entree absolute inset-0 grid place-items-center bg-bg/85 text-[13px] font-semibold text-primary"
          >
            Copié
          </span>
        )}
        {erreur && (
          <p role="alert" title={erreur} className="absolute inset-x-0 top-0 truncate bg-bg/90 px-2 py-1 text-[11px] text-error">
            {erreur}
          </p>
        )}
      </div>
    </div>
  );
}

function Vignettes() {
  const [pile, setPile] = useState<Pile>({ cartes: [], dessus: false });
  const [erreur, setErreur] = useState("");

  useEffect(() => {
    vignettesLister().then(setPile, (e) => setErreur(errorText(e)));
    // Une pile neuve efface l'erreur : elle ne cache pas « Tout fermer » pour la session.
    const ecoute = surPile((neuve) => {
      setPile(neuve);
      setErreur("");
    });
    return () => void ecoute.then((arreter) => arreter());
  }, []);

  const bande = (
    <div style={{ height: BANDE }} className="flex items-center justify-center px-1">
      {erreur ? (
        <p role="alert" title={erreur} className="truncate text-[11px] text-error">
          {erreur}
        </p>
      ) : (
        <button
          type="button"
          data-testid="vignettes-tout-fermer"
          onClick={() => void vignettesToutFermer().catch((e) => setErreur(errorText(e)))}
          className="h-6 rounded-full bg-surface/95 px-3 text-[11px] font-medium text-on-surface opacity-0 shadow-sm transition-opacity duration-150 group-hover:opacity-100 hover:bg-surface-high focus-visible:opacity-100"
        >
          Tout fermer
        </button>
      )}
    </div>
  );

  return (
    <div data-testid="vignettes-pile" data-dessus={pile.dessus} className="group flex h-screen w-screen flex-col">
      {pile.dessus ? bande : <div style={{ height: MARGE }} />}
      {pile.cartes.map((capture) => (
        <Carte key={capture.id} capture={capture} dessus={pile.dessus} />
      ))}
      {pile.dessus ? <div style={{ height: MARGE }} /> : bande}
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<Vignettes />);
