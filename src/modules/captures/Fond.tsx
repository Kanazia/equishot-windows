/**
 * Le fond décoratif (ADR-012) : la rangée de la réédition et son aperçu. Rust fait
 * le vrai rendu (`stockage::habiller`) ; l'aperçu en reprend les règles, en petit.
 */
import { useEffect, useRef } from "react";

import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import { margePx, type Fond, type Zone } from "@/modules/captures/formes";

/** Les fonds prêts : six dégradés, deux unis. */
export const FONDS: [string, string][] = [
  ["#6366f1", "#ec4899"],
  ["#0ea5e9", "#22d3ee"],
  ["#f97316", "#facc15"],
  ["#10b981", "#84cc16"],
  ["#7c3aed", "#2563eb"],
  ["#1e293b", "#475569"],
  ["#f1f5f9", "#f1f5f9"],
  ["#0f172a", "#0f172a"],
];
const MARGES = [
  { valeur: 4, nom: "Petite" },
  { valeur: 8, nom: "Moyenne" },
  { valeur: 14, nom: "Grande" },
];
const ARRONDIS = [
  { valeur: 0, nom: "Droits" },
  { valeur: 2, nom: "Légers" },
  { valeur: 5, nom: "Ronds" },
];
const FOND_DEFAUT: Fond = { couleurs: FONDS[0], marge: 8, arrondi: 2, ombre: true };
const CLE_FOND = "equishot.fond";
const APERCU = { largeur: 280, hauteur: 160 };

/** Le dernier fond choisi, ou celui par défaut. Rust revalide tout. */
export function dernierFond(): Fond {
  try {
    const lu = JSON.parse(localStorage.getItem(CLE_FOND) ?? "null") as Partial<Fond> | null;
    if (lu && Array.isArray(lu.couleurs) && lu.couleurs.length === 2 && typeof lu.marge === "number" && typeof lu.arrondi === "number" && typeof lu.ombre === "boolean") {
      return lu as Fond;
    }
  } catch {
    // Stockage illisible : le défaut.
  }
  return FOND_DEFAUT;
}

function retenir(fond: Fond) {
  try {
    localStorage.setItem(CLE_FOND, JSON.stringify(fond));
  } catch {
    // Sans stockage, le choix vaut pour cette fois.
  }
}

/** L'aperçu : le cadre de `source` posé sur son fond, aux règles de Rust. */
function dessiner(toile: HTMLCanvasElement, source: CanvasImageSource, cadre: Zone, fond: Fond | null) {
  const m = fond ? margePx(cadre, fond) : 0;
  const [l, h] = [cadre.largeur + 2 * m, cadre.hauteur + 2 * m];
  const s = Math.min(APERCU.largeur / l, APERCU.hauteur / h);
  toile.width = Math.max(1, Math.round(l * s));
  toile.height = Math.max(1, Math.round(h * s));
  const ctx = toile.getContext("2d");
  if (!ctx) return;
  ctx.scale(s, s);
  const rayon = fond ? Math.round((Math.min(cadre.largeur, cadre.hauteur) * fond.arrondi) / 100) : 0;
  if (fond) {
    // Mêmes lignes de niveau que Rust : x + y constant.
    const degrade = ctx.createLinearGradient(0, 0, (l + h) / 2, (l + h) / 2);
    degrade.addColorStop(0, fond.couleurs[0]);
    degrade.addColorStop(1, fond.couleurs[1]);
    ctx.fillStyle = degrade;
    ctx.fillRect(0, 0, l, h);
    if (fond.ombre && m > 0) {
      ctx.save();
      // Le flou et le décalage de l'ombre ignorent l'échelle du contexte.
      ctx.shadowColor = "rgba(0, 0, 0, 0.45)";
      ctx.shadowBlur = ((2 * m) / 3) * s;
      ctx.shadowOffsetY = Math.floor(m / 4) * s;
      ctx.beginPath();
      ctx.roundRect(m, m, cadre.largeur, cadre.hauteur, rayon);
      ctx.fillStyle = "#000";
      ctx.fill();
      ctx.restore();
    }
  }
  ctx.save();
  ctx.beginPath();
  ctx.roundRect(m, m, cadre.largeur, cadre.hauteur, rayon);
  ctx.clip();
  ctx.drawImage(source, cadre.x, cadre.y, cadre.largeur, cadre.hauteur, m, m, cadre.largeur, cadre.hauteur);
  ctx.restore();
}

function Choix({
  nom,
  testid,
  valeurs,
  actuelle,
  onChoisir,
}: {
  nom: string;
  testid: string;
  valeurs: { valeur: number; nom: string }[];
  actuelle: number | undefined;
  onChoisir: (valeur: number) => void;
}) {
  return (
    <div className="flex items-center gap-1.5 text-[11px]" role="radiogroup" aria-label={nom}>
      <span className="w-12 text-muted">{nom}</span>
      {valeurs.map((v) => (
        <button
          key={v.valeur}
          type="button"
          role="radio"
          aria-checked={actuelle === v.valeur}
          data-testid={`${testid}-${v.valeur}`}
          className={cn(
            "rounded border border-outline px-2 py-0.5",
            actuelle === v.valeur ? "bg-surface-highest text-primary" : "text-on-surface-variant hover:text-on-surface",
          )}
          onClick={() => onChoisir(v.valeur)}
        >
          {v.nom}
        </button>
      ))}
    </div>
  );
}

/** La rangée du fond, sous la barre de la réédition. `source` : l'image annotée. */
export function PanneauFond({
  fond,
  onChange,
  source,
  cadre,
}: {
  fond: Fond | null;
  onChange: (fond: Fond | null) => void;
  source: CanvasImageSource | null;
  cadre: Zone;
}) {
  const apercu = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    if (source && apercu.current) dessiner(apercu.current, source, cadre, fond);
  }, [source, cadre, fond]);

  // Après « Sans fond », un choix repart du dernier fond retenu.
  const base = fond ?? dernierFond();
  function changer(modif: Partial<Fond>) {
    const suivant = { ...base, ...modif };
    retenir(suivant);
    onChange(suivant);
  }

  return (
    <div data-testid="fond-panneau" className="flex flex-wrap items-center gap-4 rounded-lg border border-outline bg-surface-low px-3 py-2">
      <canvas ref={apercu} data-testid="fond-apercu" aria-label="Aperçu du fond" className="rounded border border-outline" />
      <div className="flex flex-col gap-2">
        <div className="flex flex-wrap gap-1.5" role="radiogroup" aria-label="Fond">
          {FONDS.map((couleurs, i) => {
            const choisi = fond !== null && fond.couleurs[0] === couleurs[0] && fond.couleurs[1] === couleurs[1];
            return (
              <button
                key={couleurs.join()}
                type="button"
                role="radio"
                aria-checked={choisi}
                aria-label={`Fond ${i + 1}`}
                data-testid={`fond-prereglage-${i}`}
                className={cn("size-6 rounded-full border border-outline", choisi && "ring-2 ring-primary ring-offset-1 ring-offset-surface")}
                style={{ background: `linear-gradient(135deg, ${couleurs[0]}, ${couleurs[1]})` }}
                onClick={() => changer({ couleurs })}
              />
            );
          })}
        </div>
        <Choix nom="Marge" testid="fond-marge" valeurs={MARGES} actuelle={fond?.marge} onChoisir={(marge) => changer({ marge })} />
        <Choix nom="Coins" testid="fond-arrondi" valeurs={ARRONDIS} actuelle={fond?.arrondi} onChoisir={(arrondi) => changer({ arrondi })} />
        <div className="flex gap-2">
          <Button variant="outline" size="sm" aria-pressed={fond?.ombre ?? false} data-testid="fond-ombre" className={cn(fond?.ombre && "text-primary")} onClick={() => changer({ ombre: !base.ombre })}>
            Ombre
          </Button>
          <Button variant="ghost" size="sm" data-testid="fond-retirer" disabled={fond === null} onClick={() => onChange(null)}>
            Sans fond
          </Button>
        </div>
      </div>
    </div>
  );
}
