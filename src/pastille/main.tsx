/**
 * La pastille dépliée montre tout, tout le temps : « Zone », « Écran entier », le
 * texte d'une zone (ADR-010), la vidéo (ADR-016), le différé, la galerie, les
 * réglages, « contracter » et « ranger » (le 9 octobre 2026 : montrés au
 * survol seulement, ils laissaient un trou). Contractée, il reste la poignée, les
 * deux icônes de capture, « déplier » et « ranger ». Elle n'est pas
 * focalisable (Rust) : un clic ici laisse le premier plan à l'application où l'on
 * travaille.
 */
import { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";

import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "material-symbols/outlined.css";

import { Icon } from "@/components/Icon";
import {
  capturesDiffereAnnuler,
  capturesEcranEntier,
  capturesEtatRaccourci,
  capturesOuvrirSelection,
  capturesOuvrirSelectionTexte,
  errorText,
  pastilleContractee,
  pastilleContracter,
  pastilleFilmer,
  pastilleMenuDiffere,
  pastilleRanger,
  principaleOuvrir,
  surDiffere,
  surErreurCapture,
  surInfoCapture,
  surRaccourciChange,
  type EtatRaccourci,
} from "@/lib/captures-api";
import { cn } from "@/lib/utils";
import "@/index.css";

/** Durée d'affichage d'une erreur ou d'un « Enregistrée », en ms. */
const DUREE_MESSAGE = 5000;
/** Durée d'affichage d'un « Texte copié », en ms. */
const DUREE_INFO = 3000;

function Pastille() {
  const [raccourci, setRaccourci] = useState<EtatRaccourci | null>(null);
  const [erreur, setErreur] = useState("");
  /** Un message neutre de Rust (« Texte copié »), effacé par une erreur. */
  const [info, setInfo] = useState("");
  const [prise, setPrise] = useState(false);
  /** Seul « Écran entier » attend son enregistrement : « Zone » reste libre. */
  const [ecranEnCours, setEcranEnCours] = useState(false);
  const [contractee, setContractee] = useState(false);
  /** Secondes avant l'écran entier différé (ADR-009), `null` hors différé. */
  // ponytail: la pastille vit autant qu'EquiShot, elle voit chaque différé depuis son début.
  const [reste, setReste] = useState<number | null>(null);

  useEffect(() => {
    capturesEtatRaccourci().then(setRaccourci, (e) => setErreur(errorText(e)));
    pastilleContractee().then(setContractee, (e) => setErreur(errorText(e)));
    const ecoutes = [surRaccourciChange(setRaccourci), surErreurCapture(setErreur), surInfoCapture(setInfo), surDiffere(setReste)];
    return () => ecoutes.forEach((ecoute) => void ecoute.then((arreter) => arreter()));
  }, []);

  useEffect(() => {
    if (!erreur) return;
    const minuterie = window.setTimeout(() => setErreur(""), DUREE_MESSAGE);
    return () => window.clearTimeout(minuterie);
  }, [erreur]);

  useEffect(() => {
    if (!info) return;
    const minuterie = window.setTimeout(() => setInfo(""), DUREE_INFO);
    return () => window.clearTimeout(minuterie);
  }, [info]);

  function zone() {
    setErreur("");
    capturesOuvrirSelection().catch((e) => setErreur(errorText(e)));
  }

  function texte() {
    setErreur("");
    setInfo("");
    capturesOuvrirSelectionTexte().catch((e) => setErreur(errorText(e)));
  }

  function video() {
    setErreur("");
    setInfo("");
    pastilleFilmer().catch((e) => setErreur(errorText(e)));
  }

  async function ecranEntier() {
    if (ecranEnCours) return;
    setErreur("");
    setEcranEnCours(true);
    try {
      await capturesEcranEntier();
      setPrise(true);
      window.setTimeout(() => setPrise(false), 1500);
    } catch (e) {
      setErreur(errorText(e));
    } finally {
      setEcranEnCours(false);
    }
  }

  function contracter(valeur: boolean) {
    pastilleContracter(valeur).then(() => setContractee(valeur), (e) => setErreur(errorText(e)));
  }

  const indisponible = raccourci?.principal.etat === "indisponible";
  const titreZone = raccourci ? `Zone (${raccourci.principal.combinaison})` : "Zone";
  const titreTexte = raccourci?.texte.type === "clavier" ? `Texte d'une zone (${raccourci.texte.combinaison})` : "Texte d'une zone";
  const titreVideo = raccourci?.video?.type === "clavier" ? `Vidéo (${raccourci.video.combinaison})` : "Vidéo";
  const titreEcran = raccourci?.ecran?.type === "clavier" ? `Écran entier (${raccourci.ecran.combinaison})` : "Écran entier";
  const bouton = "flex h-9 items-center gap-2 rounded-full text-[13px] font-medium whitespace-nowrap transition-[transform,background-color] duration-150 active:scale-[0.97]";
  const outil = "eq-hover grid size-8 place-items-center rounded-full text-muted";
  const ranger = (
    <button type="button" data-testid="pastille-ranger" title="Ranger dans la zone de notification" aria-label="Ranger" className={outil} onClick={() => void pastilleRanger().catch((e) => setErreur(errorText(e)))}>
      <Icon name="remove" size={18} />
    </button>
  );
  // Contractée, l'erreur n'a pas la place d'un texte : l'icône « Écran entier » vire au rouge.
  const erreurEnTexte = Boolean(erreur) && !contractee;
  const infoEnTexte = Boolean(info) && !erreur && !contractee;

  return (
    <div className="flex h-screen w-screen items-center p-0.5">
      <div
        data-testid="pastille-corps"
        data-contractee={contractee}
        className="flex h-full w-full items-center gap-1 rounded-full border border-outline bg-surface/95 pr-1.5 pl-1 shadow-sm"
      >
        <span
          data-tauri-drag-region
          data-testid="pastille-poignee"
          title="Déplacer"
          className="grid h-9 w-6 shrink-0 cursor-grab place-items-center text-muted"
        >
          <Icon name="drag_indicator" size={18} className="pointer-events-none" />
        </span>

        {/* « Zone » reste là même pendant un message : il ne masque que « Écran entier ». */}
        <button
          type="button"
          data-testid="pastille-zone"
          title={titreZone}
          aria-label={contractee ? titreZone : undefined}
          onClick={zone}
          className={cn(bouton, "relative shrink-0 bg-primary/15 text-primary hover:bg-primary/25", contractee ? "w-9 justify-center" : "px-4")}
        >
          <Icon name="screenshot_region" size={17} />
          {!contractee && "Zone"}
          <span
            data-testid="pastille-raccourci"
            data-etat={raccourci?.principal.etat ?? "inconnu"}
            title={indisponible ? (raccourci?.principal.motif ?? "Raccourci indisponible") : undefined}
            className={cn("size-1.5 rounded-full bg-tertiary", contractee && "absolute top-1 right-1", !indisponible && "hidden")}
          />
        </button>
        {/* Le compte à rebours passe avant un message : il porte l'annulation. */}
        {erreurEnTexte && reste === null ? (
          <p role="alert" data-testid="pastille-erreur" title={erreur} className="min-w-0 flex-1 truncate px-1 text-[11px] text-error">
            {erreur}
          </p>
        ) : infoEnTexte && reste === null ? (
          <p role="status" data-testid="pastille-info" title={info} className="min-w-0 flex-1 truncate px-1 text-[11px] text-on-surface">
            {info}
          </p>
        ) : reste !== null ? (
          <button
            type="button"
            data-testid="pastille-ecran"
            data-etat="differe"
            data-reste={reste}
            title="Annuler la capture différée"
            aria-label={`Capture dans ${reste} s, cliquer pour l'annuler`}
            onClick={() => void capturesDiffereAnnuler().catch((e) => setErreur(errorText(e)))}
            className={cn(bouton, "eq-hover shrink-0 text-primary tabular-nums", contractee ? "w-9 justify-center" : "px-4")}
          >
            {contractee ? reste : <Icon name="timer" size={17} />}
            {!contractee && `Dans ${reste} s`}
          </button>
        ) : (
          <button
            type="button"
            data-testid="pastille-ecran"
            data-etat={erreur ? "erreur" : ecranEnCours ? "en-cours" : prise ? "prise" : "pret"}
            aria-busy={ecranEnCours}
            title={erreur || titreEcran}
            aria-label={contractee ? erreur || titreEcran : undefined}
            onClick={() => void ecranEntier()}
            className={cn(bouton, "eq-hover shrink-0 text-on-surface", contractee ? "w-9 justify-center" : "px-4", ecranEnCours && "text-muted", erreur && "text-error")}
          >
            <Icon
              name={erreur ? "error" : ecranEnCours ? "progress_activity" : prise ? "check" : "screenshot_monitor"}
              size={17}
              className={cn(ecranEnCours && "animate-spin", prise && "text-primary")}
            />
            {!contractee && (ecranEnCours ? "En cours" : prise ? "Enregistrée" : "Écran entier")}
          </button>
        )}

        {contractee ? (
          <span className="ml-auto flex shrink-0 items-center">
            <button type="button" data-testid="pastille-deplier" title="Déplier" aria-label="Déplier" className={outil} onClick={() => contracter(false)}>
              <Icon name="keyboard_arrow_right" size={18} />
            </button>
            {ranger}
          </span>
        ) : (
          <span className="ml-auto flex shrink-0 items-center">
            <button type="button" data-testid="pastille-texte" title={titreTexte} aria-label={titreTexte} className={outil} onClick={texte}>
              <Icon name="text_fields" size={18} />
            </button>
            <button type="button" data-testid="pastille-video" title={titreVideo} aria-label={titreVideo} className={outil} onClick={video}>
              <Icon name="videocam" size={18} />
            </button>
            <button type="button" data-testid="pastille-differe" title="Écran entier différé" aria-label="Écran entier différé" className={outil} onClick={() => void pastilleMenuDiffere().catch((e) => setErreur(errorText(e)))}>
              <Icon name="timer" size={18} />
            </button>
            <button type="button" data-testid="pastille-galerie" title="Galerie" aria-label="Galerie" className={outil} onClick={() => void principaleOuvrir("galerie").catch((e) => setErreur(errorText(e)))}>
              <Icon name="photo_library" size={18} />
            </button>
            <button type="button" data-testid="pastille-reglages" title="Réglages" aria-label="Réglages" className={outil} onClick={() => void principaleOuvrir("reglages").catch((e) => setErreur(errorText(e)))}>
              <Icon name="settings" size={18} />
            </button>
            <button type="button" data-testid="pastille-contracter" title="Contracter" aria-label="Contracter" className={outil} onClick={() => contracter(true)}>
              <Icon name="keyboard_arrow_left" size={18} />
            </button>
            {ranger}
          </span>
        )}
      </div>
    </div>
  );
}

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(<Pastille />);
