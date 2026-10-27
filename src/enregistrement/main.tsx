/**
 * La barre d'enregistrement vidéo (ADR-016, ADR-017). Elle s'ouvre en panneau
 * de mise en scène (`Preparation`), puis devient la barre : le décompte, puis la
 * durée filmée, les outils de la souris (0.24 : souris, pointeur, crayon), le zoom et le
 * projecteur (0.22), « Pause » / « Reprendre », « Arrêter », « Abandonner ». Rust la
 * rend invisible aux captures (`content_protected`) et, une fois lancée, non
 * focalisable : un clic ici laisse le premier plan à l'application filmée.
 * Posée sur la zone filmée (l'écran entier, ou glissée dedans), elle s'efface au survol
 * et laisse passer les clics, sauf sa poignée (de quoi la déplacer, le point et la durée) :
 * Rust suit le curseur et l'annonce (`video://survol`). Hors de la zone, elle reste entière.
 */
import { getCurrentWindow } from "@tauri-apps/api/window";
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
  videoEffet,
  videoEtat,
  videoOutil,
  videoPauser,
  type EtatBarre,
  type OutilSouris,
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

/** Le zoom ou le projecteur (0.22) : allumé, il prend l'accent ; son titre rappelle le raccourci. */
function BasculeEffet(props: { testid: string; icone: string; nom: string; raccourci: string; allume: boolean; onClick: () => void; classe: string }) {
  const titre = props.raccourci ? `${props.nom} (${props.raccourci})` : props.nom;
  return (
    <button
      type="button"
      data-testid={props.testid}
      aria-pressed={props.allume}
      aria-label={props.nom}
      title={titre}
      onClick={props.onClick}
      className={cn(props.classe, "w-9 justify-center px-0", props.allume ? "bg-primary/15 text-primary hover:bg-primary/25" : "eq-hover text-on-surface")}
    >
      <Icon name={props.icone} size={17} filled={props.allume} />
    </button>
  );
}

/** Les outils de la souris (0.24) : un seul à la fois. Le crayon garde le raccourci du dessin. */
const OUTILS: { outil: OutilSouris; icone: string; nom: string; aide: string }[] = [
  { outil: "souris", icone: "arrow_selector_tool", nom: "Souris", aide: "les clics vont à l'application filmée" },
  { outil: "pointeur", icone: "stylus_laser_pointer", nom: "Pointeur", aide: "une encre qui s'efface, sans rien cliquer dessous" },
  { outil: "crayon", icone: "draw", nom: "Crayon", aide: "une encre qui reste jusqu'à Échap" },
];

function Barre() {
  const [etat, setEtat] = useState<EtatBarre>({
    phase: "prepare",
    reste: 3,
    actifMs: 0,
    avertissement: null,
    niveaux: { micro: 0, son: 0 },
    notes: [],
    cameras: [],
    cible: null,
    zoom: false,
    projecteur: false,
    outil: "souris",
    raccourcisEffets: ["", "", ""],
  });
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
        {/* La poignée : 112 px CSS à gauche (`POIGNEE_CSS` de Rust), jamais effacée. */}
        <div data-testid="barre-poignee" className={cn("flex h-full w-[110px] shrink-0 items-center gap-1.5 rounded-full pl-1", effacee && "bg-surface/95")}>
          <span
            data-testid="barre-deplacer"
            role="button"
            aria-label="Déplacer la barre"
            title="Déplacer la barre"
            onPointerDown={(e) => {
              if (e.button === 0) getCurrentWindow().startDragging().catch((err) => setErreur(errorText(err)));
            }}
            className="flex h-9 w-5 shrink-0 cursor-grab items-center justify-center rounded-full text-muted transition-colors duration-150 hover:text-on-surface active:cursor-grabbing"
          >
            <Icon name="drag_indicator" size={16} />
          </span>
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
            <>
              <div role="radiogroup" aria-label="Outil de la souris" data-testid="barre-outils" className="flex h-9 shrink-0 items-center gap-0.5 rounded-full bg-surface-high/70 p-0.5">
                {OUTILS.map(({ outil, icone, nom, aide }) => {
                  const choisi = etat.outil === outil;
                  const raccourci = outil === "crayon" && etat.raccourcisEffets[2] ? ` (${etat.raccourcisEffets[2]})` : "";
                  return (
                    <button
                      key={outil}
                      type="button"
                      role="radio"
                      data-testid={`barre-outil-${outil}`}
                      aria-checked={choisi}
                      aria-label={nom}
                      title={`${nom}${raccourci} : ${aide}`}
                      onClick={() => void videoOutil(outil).catch((e) => setErreur(errorText(e)))}
                      className={cn(
                        "flex size-8 items-center justify-center rounded-full transition-[transform,background-color,color] duration-150 active:scale-[0.97]",
                        choisi ? "bg-primary/15 text-primary" : "eq-hover text-on-surface",
                      )}
                    >
                      <Icon name={icone} size={17} filled={choisi} />
                    </button>
                  );
                })}
              </div>
              <BasculeEffet
                testid="barre-zoom"
                icone="zoom_in"
                nom="Zoom"
                raccourci={etat.raccourcisEffets[0]}
                allume={etat.zoom}
                onClick={() => void videoEffet(true).catch((e) => setErreur(errorText(e)))}
                classe={bouton}
              />
              <BasculeEffet
                testid="barre-projecteur"
                icone="highlight"
                nom="Projecteur"
                raccourci={etat.raccourcisEffets[1]}
                allume={etat.projecteur}
                onClick={() => void videoEffet(false).catch((e) => setErreur(errorText(e)))}
                classe={bouton}
              />
            </>
          )}
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
