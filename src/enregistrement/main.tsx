/**
 * La barre d'enregistrement vidéo (ADR-016, ADR-017). Elle s'ouvre en panneau
 * de mise en scène (`Preparation`), puis devient la barre : le décompte, puis la
 * durée filmée, « Pause » / « Reprendre », « Arrêter », « Abandonner ». Rust la
 * rend invisible aux captures (`content_protected`) et, une fois lancée, non
 * focalisable : un clic ici laisse le premier plan à l'application filmée.
 * Survolée, elle s'efface et laisse passer les clics, sauf sa poignée (le point
 * et la durée) : Rust suit le curseur et l'annonce (`video://survol`).
 */
import { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";

import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "material-symbols/outlined.css";

import { Icon } from "@/components/Icon";
import {
  duree,
  errorText,
  surEtatVideo,
  surSurvolVideo,
  videoAbandonner,
  videoArreter,
  videoEtat,
  videoPauser,
  type EtatBarre,
  type Survol,
} from "@/lib/captures-api";
import { cn } from "@/lib/utils";
import "@/index.css";

import { Preparation } from "./Preparation";

/** Une fine jauge verticale par source, à côté de la durée. */
function Jauge({ niveau, titre }: { niveau: number; titre: string }) {
  return (
    <span title={titre} className="flex h-5 w-1 shrink-0 items-end overflow-hidden rounded-full bg-surface-high" aria-hidden="true">
      <span className="w-full rounded-full bg-primary" style={{ height: `${Math.round(niveau * 100)}%` }} />
    </span>
  );
}

function Barre() {
  const [etat, setEtat] = useState<EtatBarre>({ phase: "prepare", reste: 3, actifMs: 0, avertissement: null, niveaux: { micro: 0, son: 0 }, notes: [], cameras: [], cible: null });
  const [survol, setSurvol] = useState<Survol>("normale");
  const [erreur, setErreur] = useState("");
  const [envoi, setEnvoi] = useState(false);

  useEffect(() => {
    videoEtat().then(setEtat, (e) => setErreur(errorText(e)));
    const ecoutes = [surEtatVideo(setEtat), surSurvolVideo(setSurvol)];
    return () => ecoutes.forEach((ecoute) => void ecoute.then((arreter) => arreter()));
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

  if (etat.phase === "prepare") return <Preparation etat={etat} />;

  const enPause = etat.phase === "pause";
  const effacee = survol === "effacee";
  const bouton =
    "flex h-9 shrink-0 items-center gap-1.5 rounded-full px-3 text-[13px] font-medium whitespace-nowrap transition-[transform,background-color] duration-150 active:scale-[0.97] disabled:opacity-50";
  const message = erreur || etat.avertissement;
  // Effacée : tout sauf la poignée passe à 10 %.
  const reste = cn("transition-opacity duration-150", effacee && "opacity-10");

  return (
    <div className="flex h-screen w-screen items-center p-0.5">
      <div
        data-testid="barre-video"
        data-phase={etat.phase}
        data-survol={survol}
        className={cn(
          "flex h-full w-full items-center gap-1 rounded-full border pr-1.5 shadow-sm transition-colors duration-150",
          effacee ? "border-transparent bg-transparent shadow-none" : "border-outline bg-surface/95",
        )}
      >
        {/* La poignée : 96 px CSS à gauche (`POIGNEE_CSS` de Rust), jamais effacée. */}
        <div data-testid="barre-poignee" className={cn("flex h-full w-[94px] shrink-0 items-center gap-2 rounded-full pl-3", effacee && "bg-surface/95")}>
          {etat.phase === "decompte" ? (
            <span data-testid="barre-decompte" className="shrink-0 text-[13px] font-medium whitespace-nowrap text-on-surface tabular-nums">
              Départ dans {etat.reste}
            </span>
          ) : (
            <>
              <span className={cn("size-2.5 shrink-0 rounded-full", enPause ? "bg-muted" : "animate-pulse bg-error")} aria-hidden="true" />
              <span data-testid="barre-duree" data-actif-ms={etat.actifMs} className="w-14 shrink-0 text-[13px] font-medium text-on-surface tabular-nums">
                {duree(etat.actifMs)}
              </span>
            </>
          )}
        </div>
        <div className={cn("flex min-w-0 flex-1 items-center gap-1", reste)}>
          {etat.phase !== "decompte" && (
            <span data-testid="barre-jauges" className="flex shrink-0 items-center gap-0.5">
              <Jauge niveau={etat.niveaux.micro} titre="Micro" />
              <Jauge niveau={etat.niveaux.son} titre="Son de l'ordinateur" />
            </span>
          )}
          <p role={erreur ? "alert" : "status"} title={message ?? undefined} className={cn("min-w-0 flex-1 truncate px-1 text-[11px]", erreur ? "text-error" : "text-muted")}>
            {message}
          </p>
          {etat.phase !== "decompte" && (
            <button
              type="button"
              data-testid="barre-pause"
              disabled={envoi}
              onClick={() => void videoPauser(!enPause).catch((e) => setErreur(errorText(e)))}
              className={cn(bouton, "eq-hover text-on-surface")}
            >
              <Icon name={enPause ? "play_arrow" : "pause"} size={17} />
              {enPause ? "Reprendre" : "Pause"}
            </button>
          )}
          <button
            type="button"
            data-testid="barre-abandonner"
            title="Abandonner : rien n'est gardé"
            aria-label="Abandonner"
            disabled={envoi}
            onClick={() => void geste(videoAbandonner)}
            className={cn(bouton, "eq-hover w-9 justify-center px-0 text-on-surface")}
          >
            <Icon name="delete" size={17} />
          </button>
          {etat.phase !== "decompte" && (
            <button type="button" data-testid="barre-arreter" disabled={envoi} onClick={() => void geste(videoArreter)} className={cn(bouton, "bg-primary/15 text-primary hover:bg-primary/25")}>
              <Icon name="stop" size={17} filled />
              Arrêter
            </button>
          )}
        </div>
      </div>
    </div>
  );
}

const racine = document.getElementById("root");
if (!racine) {
  throw new Error("Élément racine introuvable dans enregistrement.html.");
}
ReactDOM.createRoot(racine).render(<Barre />);
