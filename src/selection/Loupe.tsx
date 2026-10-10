/**
 * La loupe de la sélection (ADR-008) : les 15 x 15 pixels de l'écran vivant
 * autour du curseur, huit fois plus grands, le pixel central cerné. Rust les lit
 * à chaque demande, une à la fois, et ne garde rien.
 */
import { useEffect, useRef, useState } from "react";

import { capturesSelectionLoupe } from "@/lib/captures-api";
import { couleurHex } from "@/modules/captures/formes";

/** Côté du carré, en pixels physiques (`stockage::COTE_LOUPE`). */
const COTE = 15;
const AGRANDI = 8;
/** Au moins 33 ms entre deux lectures : trente images par seconde au plus. */
const PAUSE = 33;
/** Écart entre le curseur et la loupe, en pixels CSS. */
const ECART = 24;

type Point = { x: number; y: number };

/** Le pixel central, en octets après les 8 de la position : la couleur de la pipette (ADR-013). */
const CENTRE = 8 + ((COTE >> 1) * COTE + (COTE >> 1)) * 4;

export function Loupe({
  point,
  onCurseur,
  onCouleur,
  copiee,
}: {
  point: Point | null;
  onCurseur: (physique: Point) => void;
  onCouleur: (couleur: string) => void;
  /** La couleur que la touche C vient de copier, dite un instant sous la loupe. */
  copiee: string | null;
}) {
  const toile = useRef<HTMLCanvasElement>(null);
  const [curseur, setCurseur] = useState<Point | null>(null);
  const [couleur, setCouleur] = useState<string | null>(null);
  const rappel = useRef({ onCurseur, onCouleur });
  rappel.current = { onCurseur, onCouleur };

  useEffect(() => {
    let fini = false;
    void (async () => {
      while (!fini) {
        const debut = performance.now();
        try {
          const octets = new Uint8Array(await capturesSelectionLoupe());
          const ctx = toile.current?.getContext("2d");
          if (fini || !ctx) return;
          const vue = new DataView(octets.buffer, octets.byteOffset, 8);
          const lu = { x: vue.getInt32(0, true), y: vue.getInt32(4, true) };
          ctx.putImageData(new ImageData(new Uint8ClampedArray(octets.buffer, octets.byteOffset + 8, COTE * COTE * 4), COTE, COTE), 0, 0);
          const teinte = couleurHex(octets.subarray(CENTRE, CENTRE + 3));
          setCurseur(lu);
          setCouleur(teinte);
          rappel.current.onCurseur(lu);
          rappel.current.onCouleur(teinte);
        } catch {
          // ponytail: curseur sur un autre écran, la loupe garde sa dernière image.
        }
        await new Promise((r) => window.setTimeout(r, Math.max(0, PAUSE - (performance.now() - debut))));
      }
    })();
    return () => {
      fini = true;
    };
  }, []);

  const taille = COTE * AGRANDI;
  // En bas à droite du curseur, de l'autre côté près d'un bord.
  const gauche = point ? (point.x + ECART + taille > window.innerWidth ? point.x - ECART - taille : point.x + ECART) : -9999;
  // Les deux légendes, position et couleur, font 44 px sous le carré.
  const haut = point ? (point.y + ECART + taille + 44 > window.innerHeight ? point.y - ECART - taille - 44 : point.y + ECART) : -9999;

  return (
    <div data-testid="selection-loupe" className="pointer-events-none absolute" style={{ left: gauche, top: haut }}>
      <div className="relative overflow-hidden rounded-md border border-outline bg-black shadow-lg" style={{ width: taille, height: taille }}>
        <canvas
          ref={toile}
          data-testid="selection-loupe-toile"
          width={COTE}
          height={COTE}
          className="block"
          style={{ width: taille, height: taille, imageRendering: "pixelated" }}
        />
        <div
          className="absolute outline outline-2 outline-white mix-blend-difference"
          style={{ left: (COTE >> 1) * AGRANDI, top: (COTE >> 1) * AGRANDI, width: AGRANDI, height: AGRANDI }}
        />
      </div>
      <p
        data-testid="selection-loupe-curseur"
        className="mt-1 rounded bg-surface px-1.5 py-0.5 text-center font-mono text-[11px] text-on-surface tabular-nums"
      >
        {curseur ? `${curseur.x}, ${curseur.y}` : "…"}
      </p>
      <p
        data-testid="selection-loupe-couleur"
        title="C copie la couleur"
        className="mt-1 flex items-center justify-center gap-1.5 rounded bg-surface px-1.5 py-0.5 font-mono text-[11px] text-on-surface"
      >
        {couleur && <span className="size-2.5 rounded-sm border border-outline" style={{ background: couleur }} />}
        {copiee ? `${copiee} copiée` : (couleur ?? "…")}
      </p>
    </div>
  );
}
