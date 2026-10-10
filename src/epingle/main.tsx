/**
 * Une épingle (ADR-011) : une capture posée sur l'écran, toujours devant. Tirer
 * déplace, double-clic ou Échap ferme, molette pour la taille, Ctrl + molette
 * pour l'opacité, clic du milieu remet les deux à 100 %, Ctrl + C copie, clic
 * droit ouvre le menu. Rust tient la fenêtre ; la page tient l'opacité.
 */
import { useEffect, useRef, useState } from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";

import "@fontsource/inter/500.css";

import { adresse, epingleCopier, epingleFermer, epingleInfos, epingleMenu, epingleTaille, errorText, surOrigineEpingle } from "@/lib/captures-api";
import "@/index.css";

/** Au-delà de ce déplacement, en pixels CSS, l'appui devient un déplacement de la fenêtre. */
const SEUIL = 3;
/** Pas de la molette, en points de pourcentage, pour la taille comme pour l'opacité. */
const PAS = 10;

function Epingle() {
  const [source, setSource] = useState("");
  const [opacite, setOpacite] = useState(100);
  const [indication, setIndication] = useState("");
  /** La taille obtenue de Rust, qui la borne. */
  const taille = useRef(100);
  /** Les demandes de taille passent une à une : deux en vol pouvaient se croiser et décaler le pourcentage d'un cran. */
  const file = useRef(Promise.resolve());
  const opaciteVoulue = useRef(100);
  const depart = useRef<{ x: number; y: number } | null>(null);
  const minuteur = useRef<number | undefined>(undefined);

  function dire(texte: string) {
    setIndication(texte);
    window.clearTimeout(minuteur.current);
    minuteur.current = window.setTimeout(() => setIndication(""), 1000);
  }

  /** `voulu` part de la taille obtenue par la demande d'avant, une fois son tour venu. */
  function regler(voulu: (actuelle: number) => number, ancreX = 0.5, ancreY = 0.5) {
    file.current = file.current
      .then(() => epingleTaille(voulu(taille.current), ancreX, ancreY))
      .then(
        (obtenu) => {
          taille.current = obtenu;
          dire(`${Math.round(obtenu)} %`);
        },
        (e) => dire(errorText(e)),
      );
  }

  function opaciter(voulue: number) {
    opaciteVoulue.current = Math.min(100, Math.max(10, voulue));
    setOpacite(opaciteVoulue.current);
    dire(`Opacité ${opaciteVoulue.current} %`);
  }

  function copier() {
    epingleCopier().then(() => dire("Copiée"), (e) => dire(errorText(e)));
  }

  useEffect(() => {
    let lien = "";
    async function charger() {
      const infos = await epingleInfos();
      taille.current = infos.pourcent;
      return fetch(adresse(infos.rendu, infos.version));
    }
    (async () => {
      // Une capture supprimée entre les infos et la lecture vient de partir à la corbeille :
      // les infos relues donnent son nouveau chemin.
      let reponse = await charger();
      if (!reponse.ok) reponse = await charger();
      if (!reponse.ok) throw new Error("Image de la capture illisible.");
      // L'image est gardée en mémoire : une capture supprimée pendant qu'elle est épinglée reste affichée.
      lien = URL.createObjectURL(await reponse.blob());
      setSource(lien);
    })().catch((e) => dire(errorText(e)));
    return () => URL.revokeObjectURL(lien);
  }, []);

  useEffect(() => {
    // Non passif : Ctrl + molette ne doit pas zoomer la page.
    function surMolette(e: WheelEvent) {
      e.preventDefault();
      // ponytail: un événement vaut un pas ; un pavé tactile en envoie plus qu'une molette.
      const sens = e.deltaY < 0 ? 1 : -1;
      if (e.ctrlKey) opaciter(opaciteVoulue.current + sens * PAS);
      else regler((actuelle) => actuelle + sens * PAS, e.clientX / window.innerWidth, e.clientY / window.innerHeight);
    }
    function surTouche(e: KeyboardEvent) {
      if (e.key === "Escape") {
        e.preventDefault();
        void epingleFermer();
      } else if (e.ctrlKey && e.key.toLowerCase() === "c") {
        e.preventDefault();
        copier();
      }
    }
    window.addEventListener("wheel", surMolette, { passive: false });
    window.addEventListener("keydown", surTouche);
    const arret = surOrigineEpingle(() => regler(() => 100));
    return () => {
      window.removeEventListener("wheel", surMolette);
      window.removeEventListener("keydown", surTouche);
      void arret.then((f) => f());
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  return (
    <div
      data-testid="epingle"
      className="fixed inset-0 cursor-move overflow-hidden select-none"
      onPointerDown={(e) => {
        if (e.button !== 0) return;
        depart.current = { x: e.screenX, y: e.screenY };
      }}
      onPointerMove={(e) => {
        const d = depart.current;
        if (!d || (e.buttons & 1) === 0) return;
        if (Math.hypot(e.screenX - d.x, e.screenY - d.y) <= SEUIL) return;
        depart.current = null;
        // ponytail: la fenêtre part du point atteint, les 3 premiers pixels sont perdus.
        getCurrentWindow().startDragging().catch((err) => dire(errorText(err)));
      }}
      onPointerUp={() => {
        depart.current = null;
      }}
      onMouseDown={(e) => {
        // Le clic du milieu : taille et opacité à 100 %, sans le défilement automatique.
        if (e.button !== 1) return;
        e.preventDefault();
        regler(() => 100, e.clientX / window.innerWidth, e.clientY / window.innerHeight);
        opaciter(100);
      }}
      onDoubleClick={() => void epingleFermer()}
      onContextMenu={(e) => {
        e.preventDefault();
        epingleMenu().catch((err) => dire(errorText(err)));
      }}
    >
      {source && (
        <img src={source} alt="" draggable={false} data-testid="epingle-image" className="pointer-events-none block size-full" style={{ opacity: opacite / 100 }} />
      )}
      {/* Liseré par-dessus le bord de l'image : une ombre portée rognerait les coins ou déborderait (ADR-011). */}
      <div className="pointer-events-none absolute inset-0 ring-1 ring-primary/60 ring-inset" />
      {indication && (
        <span
          data-testid="epingle-indication"
          className="pointer-events-none absolute top-1 right-1 rounded bg-surface/90 px-1.5 py-0.5 text-[11px] text-on-surface tabular-nums"
        >
          {indication}
        </span>
      )}
    </div>
  );
}

const racine = document.getElementById("root");
if (!racine) {
  throw new Error("Élément racine introuvable dans epingle.html.");
}
ReactDOM.createRoot(racine).render(<Epingle />);
