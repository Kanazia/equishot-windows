import { useRef, useState } from "react";

import { glisserRecadrage, recadrageNet, tirer, type Poignee, type Zone } from "@/modules/captures/formes";

/** Voile hors du cadre, celui du projecteur. */
const VOILE = "rgba(0, 0, 0, 0.55)";
/** Portée d'une poignée, en pixels CSS. */
const MARGE_POIGNEE = 10;
const COINS: Poignee[] = ["no", "ne", "so", "se"];
const CURSEUR: Partial<Record<Poignee, string>> = { no: "nwse-resize", se: "nwse-resize", ne: "nesw-resize", so: "nesw-resize" };

interface Point {
  x: number;
  y: number;
}

type Geste = { mode: "coin"; poignee: Poignee } | { mode: "deplacer"; depart: Point } | { mode: "nouveau"; depart: Point };

interface RecadrageProps {
  /** Le cadre posé, en pixels de l'original, ou `null` pour l'image entière. */
  recadrage: Zone | null;
  image: { largeur: number; hauteur: number };
  /** Mode recadrage : le cadre se tire. Sinon, seul le voile reste, sans gêner l'annotation. */
  actif: boolean;
  echelle: number;
  onChange: (recadrage: Zone | null) => void;
}

/**
 * Le cadre du recadrage (ADR-007), un calque HTML posé sur l'annotation : le
 * canevas ne le voit pas, l'aplatissement non plus. Un coin le redimensionne,
 * le milieu le déplace, un tracé hors de lui (ou n'importe où sans cadre) en
 * pose un neuf.
 */
export function Recadrage({ recadrage, image, actif, echelle, onChange }: RecadrageProps) {
  const calque = useRef<HTMLDivElement>(null);
  const geste = useRef<Geste | null>(null);
  const [apercu, setApercu] = useState<Zone | null>(null);
  const [curseur, setCurseur] = useState("crosshair");
  const entiere = { x: 0, y: 0, largeur: image.largeur, hauteur: image.hauteur };
  const cadre = apercu ?? recadrage ?? (actif ? entiere : null);
  const min = 8 * (echelle || 1);
  if (!cadre) return null;

  const ratio = () => (calque.current?.getBoundingClientRect().width ?? image.largeur) / image.largeur;
  function point(evenement: { clientX: number; clientY: number }): Point {
    const boite = calque.current?.getBoundingClientRect();
    if (!boite) return { x: 0, y: 0 };
    const r = ratio();
    return { x: (evenement.clientX - boite.left) / r, y: (evenement.clientY - boite.top) / r };
  }
  const coinSous = (p: Point) => {
    const portee = MARGE_POIGNEE / ratio();
    return COINS.find((id) => {
      const x = id === "no" || id === "so" ? cadre.x : cadre.x + cadre.largeur;
      const y = id === "no" || id === "ne" ? cadre.y : cadre.y + cadre.hauteur;
      return Math.hypot(x - p.x, y - p.y) <= portee;
    });
  };
  const dans = (p: Point) => p.x >= cadre.x && p.x <= cadre.x + cadre.largeur && p.y >= cadre.y && p.y <= cadre.y + cadre.hauteur;

  function suivre(g: Geste, p: Point): Zone | null {
    const base = recadrage ?? entiere;
    if (g.mode === "coin") return recadrageNet(tirer({ type: "rectangle", ...base }, g.poignee, p) as Zone, image, min);
    if (g.mode === "deplacer") return glisserRecadrage(base, p.x - g.depart.x, p.y - g.depart.y, image);
    return recadrageNet({ x: g.depart.x, y: g.depart.y, largeur: p.x - g.depart.x, hauteur: p.y - g.depart.y }, image, min);
  }

  const pourcent = (v: number, total: number) => `${(v / total) * 100}%`;
  return (
    <div
      ref={calque}
      data-testid="recadrage-calque"
      className="absolute inset-0 overflow-hidden"
      style={{ pointerEvents: actif ? "auto" : "none", cursor: actif ? curseur : undefined, touchAction: "none" }}
      onPointerDown={(evenement) => {
        if (!actif || evenement.button !== 0) return;
        const p = point(evenement);
        // Sans cadre posé, tout appui en trace un neuf.
        const coin = recadrage ? coinSous(p) : undefined;
        geste.current = coin
          ? { mode: "coin", poignee: coin }
          : recadrage && dans(p)
            ? { mode: "deplacer", depart: p }
            : { mode: "nouveau", depart: p };
        evenement.currentTarget.setPointerCapture(evenement.pointerId);
      }}
      onPointerMove={(evenement) => {
        const p = point(evenement);
        const g = geste.current;
        if (g) {
          const suivant = suivre(g, p);
          if (suivant) setApercu(suivant);
          return;
        }
        const coin = recadrage ? coinSous(p) : undefined;
        setCurseur(coin ? (CURSEUR[coin] ?? "crosshair") : recadrage && dans(p) ? "move" : "crosshair");
      }}
      onPointerUp={() => {
        geste.current = null;
        if (!apercu) return;
        setApercu(null);
        const toute = apercu.x === 0 && apercu.y === 0 && apercu.largeur === image.largeur && apercu.hauteur === image.hauteur;
        onChange(toute ? null : apercu);
      }}
    >
      <div
        data-testid="recadrage-cadre"
        data-recadrage={`${cadre.x},${cadre.y},${cadre.largeur},${cadre.hauteur}`}
        className={actif ? "absolute border border-dashed border-white" : "absolute"}
        style={{
          left: pourcent(cadre.x, image.largeur),
          top: pourcent(cadre.y, image.hauteur),
          width: pourcent(cadre.largeur, image.largeur),
          height: pourcent(cadre.hauteur, image.hauteur),
          // Le voile hors du cadre : une ombre sans flou, coupée par le calque.
          boxShadow: `0 0 0 100vmax ${VOILE}`,
        }}
      >
        {actif &&
          recadrage &&
          COINS.map((id) => (
            <span
              key={id}
              data-testid={`recadrage-poignee-${id}`}
              className="pointer-events-none absolute size-3 -translate-1/2 rounded-sm border border-primary bg-white"
              style={{ left: id === "no" || id === "so" ? 0 : "100%", top: id === "no" || id === "ne" ? 0 : "100%" }}
            />
          ))}
      </div>
    </div>
  );
}
