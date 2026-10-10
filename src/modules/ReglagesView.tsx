/**
 * Réglages d'EquiShot : les raccourcis (capture, dernière zone, texte d'une zone,
 * vidéo, écran entier), les vignettes, le dossier des captures, l'application
 * (lancement avec Windows, mises à jour, ADR-020), puis la vidéo (curseur, dossier
 * des vidéos ; le micro et le son se règlent dans la mise en scène, ADR-017).
 * Treize réglages : pas de recherche (la checklist la demande au-delà d'une
 * quinzaine), pas de zone dangereuse (rien d'irréversible ici : le déplacement
 * du dossier se confirme et se refait dans l'autre sens).
 * La section Vidéo liste aussi les scènes (0.19) : ce sont des données, pas des
 * réglages ; elles ne comptent pas dans le seuil de la recherche.
 */
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { version } from "../../package.json";

import { Icon } from "@/components/Icon";
import { ViewShell } from "@/components/ViewShell";
import { Button } from "@/components/ui/button";
import {
  capturesChoisirDossier,
  capturesDeplacerDossier,
  capturesDossier,
  capturesEtatRaccourci,
  capturesOuvrirDossier,
  capturesReglerRaccourci,
  capturesSuspendreRaccourci,
  combinaisonCliquee,
  combinaisonFrappee,
  demarrageLire,
  demarrageRegler,
  errorText,
  isTauri,
  lireDisposition,
  majEtat,
  majInstaller,
  majRechercher,
  MODIFICATEURS,
  surMaj,
  surReglageVignettes,
  surScenes,
  vignettesActiver,
  vignettesActives,
  vignettesAvecPastille,
  vignettesRangerAvecPastille,
  videoDossier,
  videoOuvrirDossier,
  videoReglages,
  videoRegler,
  videoSceneRenommer,
  videoScenes,
  videoSceneSupprimer,
  type Deplacement,
  type Disposition,
  type EtatMaj,
  type EtatRaccourci,
  type NomCase,
  type TypeDeclencheur,
} from "@/lib/captures-api";
import { resumeScene } from "@/enregistrement/mise-en-scene";
import { cn } from "@/lib/utils";
import { CLE_RACCOURCI } from "@/modules/captures/CapturesView";
import { Touches } from "@/modules/captures/Touches";

function Section({ titre, note, children }: { titre: string; note?: string; children: React.ReactNode }) {
  return (
    <section className="rounded-lg border border-outline bg-surface">
      <header className="flex items-center justify-between border-b border-outline px-3 py-2">
        <h2 className="text-[10px] font-semibold tracking-[0.09em] text-muted uppercase">{titre}</h2>
        {note && <span className="text-[10px] text-muted">{note}</span>}
      </header>
      <div className="divide-y divide-outline/60">{children}</div>
    </section>
  );
}

function SettingLine({
  icon,
  label,
  description,
  modified = false,
  onRevert,
  children,
}: {
  icon: string;
  label: React.ReactNode;
  description: string;
  modified?: boolean;
  onRevert?: () => void;
  children: React.ReactNode;
}) {
  return (
    <div className="flex items-start justify-between gap-6 px-3 py-3">
      <div className="flex min-w-0 gap-2.5">
        <Icon name={icon} size={17} className="mt-0.5 text-muted" />
        <div className="min-w-0">
          <p className="flex items-center gap-1.5 text-[13px] font-medium text-on-surface">
            {modified && <span className="size-1.5 shrink-0 rounded-full bg-primary" title="Valeur différente du défaut" />}
            {label}
          </p>
          <p className="mt-0.5 text-[11px] text-muted">{description}</p>
        </div>
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {modified && onRevert && (
          <Button variant="ghost" size="icon-sm" title="Revenir au défaut" aria-label="Revenir au défaut" onClick={onRevert}>
            <Icon name="undo" size={14} />
          </Button>
        )}
        {children}
      </div>
    </div>
  );
}

/** Les cases qui ouvrent la sélection ; « derniere » (ADR-009), « texte » (ADR-010), « video » (ADR-016) et « ecran » (0.21) ont leur ligne. */
const CASES_SELECTION: NomCase[] = ["principal", "secondaire"];
const CASES: NomCase[] = [...CASES_SELECTION, "derniere", "texte", "video", "ecran"];
const NOMS_CASES: Record<NomCase, string> = {
  principal: "Principal",
  secondaire: "Secondaire",
  derniere: "Dernière zone",
  texte: "Texte d'une zone",
  video: "Vidéo",
  ecran: "Écran entier",
};

/**
 * Raccourcis système, en cases comme les touches d'un jeu (ADR-003) :
 * application immédiate. Deux cases ouvrent la sélection, la troisième reprend
 * la dernière zone (ADR-009), la quatrième copie le texte d'une zone (ADR-010). Un clic sur une case ouvre l'enregistreur, qui
 * suspend toutes les cases le temps de lire la nouvelle combinaison, au clavier
 * ou à la souris ; Rust l'accepte ou la refuse, et l'ancienne revient.
 */
function RaccourciCaptureRow() {
  const queryClient = useQueryClient();
  const { data: etat } = useQuery({ queryKey: CLE_RACCOURCI, queryFn: capturesEtatRaccourci, enabled: isTauri(), staleTime: 0 });
  /** La case qui écoute, `null` au repos. */
  const [ecoute, setEcoute] = useState<NomCase | null>(null);
  const [enregistre, setEnregistre] = useState<NomCase | null>(null);
  const [erreur, setErreur] = useState("");
  /** Ce que l'enregistreur dit pendant qu'il écoute, sans cesser d'écouter. */
  const [indice, setIndice] = useState("");
  const boutons = useRef<Record<NomCase, HTMLButtonElement | null>>({ principal: null, secondaire: null, derniere: null, texte: null, video: null, ecran: null });
  // La suspension demandée au clic : réglage et reprise l'attendent, sinon elle
  // arriverait après eux et laisserait les deux cases éteintes (relecture du 3 octobre 2026).
  const suspension = useRef<Promise<unknown>>(Promise.resolve());
  // Lue au montage, pas au début de l'écoute : la première frappe la trouve déjà là.
  const disposition = useRef<Disposition | null>(null);
  useEffect(() => {
    void lireDisposition().then((lue) => {
      disposition.current = lue;
    });
  }, []);

  const poser = (nouvel: EtatRaccourci) => queryClient.setQueryData(CLE_RACCOURCI, nouvel);

  async function regler(casePrise: NomCase, type: TypeDeclencheur, combinaison: string) {
    setEcoute(null);
    setErreur("");
    setIndice("");
    try {
      await suspension.current;
      poser(await capturesReglerRaccourci(casePrise, type, combinaison));
      setEnregistre(casePrise);
      window.setTimeout(() => setEnregistre((courante) => (courante === casePrise ? null : courante)), 2000);
    } catch (e) {
      setErreur(errorText(e));
      // L'ancienne combinaison a été remise par Rust : l'écran la relit.
      void queryClient.invalidateQueries({ queryKey: CLE_RACCOURCI });
    }
  }

  // Écoute en capture sur la fenêtre. Échap, un clic ailleurs, quitter la page
  // ou perdre la main rendent le raccourci : une frappe faite pour autre chose
  // ne devient jamais le raccourci système.
  useEffect(() => {
    if (!ecoute) return;
    const casePrise = ecoute;
    let rendu = false;
    let modificateurs = false;
    const rendre = () => {
      if (rendu) return;
      rendu = true;
      setEcoute(null);
      setIndice("");
      void suspension.current.then(() => capturesSuspendreRaccourci(false)).then(poser, () => undefined);
    };
    function surTouche(evenement: KeyboardEvent) {
      evenement.preventDefault();
      evenement.stopImmediatePropagation();
      if (evenement.key === "Escape") return rendre();
      if (MODIFICATEURS.includes(evenement.key)) {
        modificateurs = true;
        return;
      }
      modificateurs = false;
      const combinaison = combinaisonFrappee(evenement, disposition.current);
      if (!combinaison) {
        setIndice("Cette touche change de nom d'un clavier à l'autre : prenez une lettre, un chiffre ou une touche de fonction.");
        return;
      }
      rendu = true;
      void regler(casePrise, "clavier", combinaison);
    }
    // Des modificateurs enfoncés puis relâchés sans qu'aucune touche n'arrive :
    // si elle a été frappée, Windows l'a donnée à l'application qui tient déjà
    // cette combinaison, et la page ne la verra jamais.
    function surRelache(evenement: KeyboardEvent) {
      const tenus = evenement.ctrlKey || evenement.altKey || evenement.shiftKey || evenement.metaKey;
      if (modificateurs && MODIFICATEURS.includes(evenement.key) && !tenus) {
        modificateurs = false;
        setIndice("Aucune touche n'est arrivée après les modificateurs. Si vous en avez frappé une, une autre application tient cette combinaison : essayez-en une autre.");
      }
    }
    // Un clic ailleurs rend le raccourci. Sur la case, le bouton 4, le 5 ou la
    // molette deviennent le raccourci ; le clic gauche ou droit la laisse écouter.
    // ponytail: le bouton est lu par la page, comme les touches, pas par le
    // crochet : une seule voie d'écoute pour les deux types.
    function surAppui(evenement: PointerEvent) {
      if (!boutons.current[casePrise]?.contains(evenement.target as Node)) return rendre();
      const combinaison = combinaisonCliquee(evenement);
      if (!combinaison) return;
      evenement.preventDefault();
      evenement.stopImmediatePropagation();
      rendu = true;
      void regler(casePrise, "souris", combinaison);
    }
    function surMenu(evenement: MouseEvent) {
      if (boutons.current[casePrise]?.contains(evenement.target as Node)) evenement.preventDefault();
    }
    window.addEventListener("keydown", surTouche, true);
    window.addEventListener("keyup", surRelache, true);
    window.addEventListener("pointerdown", surAppui, true);
    window.addEventListener("contextmenu", surMenu, true);
    window.addEventListener("blur", rendre);
    return () => {
      window.removeEventListener("keydown", surTouche, true);
      window.removeEventListener("keyup", surRelache, true);
      window.removeEventListener("pointerdown", surAppui, true);
      window.removeEventListener("contextmenu", surMenu, true);
      window.removeEventListener("blur", rendre);
      rendre();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ecoute]);

  const indisponible = (c: NomCase) => etat?.[c].etat === "indisponible";
  const modifie = (c: NomCase) => etat !== undefined && (etat[c].type !== etat[c].defautType || etat[c].combinaison !== etat[c].defaut);
  const motif = CASES.map((c) => (indisponible(c) ? etat?.[c].motif : null)).find(Boolean);
  // Les repères du parcours d'avant restent ceux du principal.
  const repere = (c: NomCase, suffixe = "") => `settings-captures-raccourci${c === "principal" ? "" : `-${c}`}${suffixe}`;
  const ligne = (c: NomCase) => (
            <div key={c} className="flex items-center gap-2">
              <span
                className={cn("text-[11px]", ecoute === c ? "text-muted" : indisponible(c) && enregistre !== c ? "text-tertiary" : "text-primary")}
                data-testid={repere(c, "-etat")}
                aria-live="polite"
              >
                {ecoute === c ? "Échap annule" : enregistre === c ? "Enregistré" : indisponible(c) ? "Indisponible" : ""}
              </span>
              {modifie(c) && ecoute !== c && etat && (
                <Button
                  variant="ghost"
                  size="icon-sm"
                  title="Revenir au défaut"
                  aria-label={c === "principal" ? "Revenir au défaut" : `${NOMS_CASES[c]} par défaut`}
                  onClick={() => void regler(c, etat[c].defautType, etat[c].defaut)}
                >
                  <Icon name="undo" size={14} />
                </Button>
              )}
              <span className="text-[11px] text-muted">{NOMS_CASES[c]}</span>
              <Button
                ref={(element) => {
                  boutons.current[c] = element;
                }}
                variant="outline"
                size="sm"
                data-testid={repere(c)}
                data-ecoute={ecoute === c}
                aria-label={
                  ecoute === c
                    ? "Faites la nouvelle combinaison, au clavier ou à la souris, Échap annule"
                    : `Raccourci ${NOMS_CASES[c].toLowerCase()} : ${etat?.[c].type === "aucun" ? "aucun" : (etat?.[c].combinaison ?? "")}. Cliquer pour le changer`
                }
                disabled={!etat}
                onClick={() => {
                  if (ecoute) return;
                  setErreur("");
                  setIndice("");
                  setEcoute(c);
                  suspension.current = capturesSuspendreRaccourci(true).catch((e) => {
                    setEcoute(null);
                    setErreur(errorText(e));
                  });
                }}
              >
                {ecoute === c ? (
                  <span className="text-tertiary">Faites la combinaison</span>
                ) : !etat ? (
                  "…"
                ) : etat[c].type === "aucun" ? (
                  <span className="text-muted">Aucun</span>
                ) : (
                  <Touches combinaison={etat[c].combinaison} />
                )}
              </Button>
              {c !== "principal" && (
                <Button
                  variant="ghost"
                  size="sm"
                  data-testid={c === "secondaire" ? "settings-captures-raccourci-retirer" : repere(c, "-retirer")}
                  className={cn(etat?.[c].type === "aucun" || ecoute === c ? "invisible" : "")}
                  onClick={() => void regler(c, "aucun", "")}
                >
                  Retirer
                </Button>
              )}
              {c === "principal" && <span aria-hidden className="invisible px-2.5 text-xs">Retirer</span>}
            </div>
  );
  return (
    <div data-testid="settings-captures-rangee">
      <SettingLine
        icon="keyboard"
        label="Raccourci de capture"
        description="Ouvre la sélection, au clavier ou à la souris."
        modified={CASES_SELECTION.some(modifie)}
      >
        <div className="flex flex-col items-end gap-1.5">{CASES_SELECTION.map(ligne)}</div>
      </SettingLine>
      <SettingLine
        icon="history"
        label="Reprendre la dernière zone"
        description="Prend la dernière zone figée, sans sélection."
        modified={modifie("derniere")}
      >
        {ligne("derniere")}
      </SettingLine>
      <SettingLine
        icon="text_fields"
        label="Texte d'une zone"
        description="Trace une zone et copie son texte, pas l'image."
        modified={modifie("texte")}
      >
        {ligne("texte")}
      </SettingLine>
      <SettingLine
        icon="videocam"
        label="Filmer une zone"
        description="Ouvre la sélection en mode vidéo ; pendant un enregistrement, l'arrête."
        modified={modifie("video")}
      >
        {ligne("video")}
      </SettingLine>
      <SettingLine
        icon="screenshot_monitor"
        label="Écran entier"
        description="Prend tout l'écran sous la souris, sans sélection."
        modified={modifie("ecran")}
      >
        {ligne("ecran")}
      </SettingLine>
      {ecoute && indice && (
        <p className="border-t border-outline/60 px-3 py-2 text-[11px] text-tertiary" role="status" data-testid="settings-captures-raccourci-indice">
          {indice}
        </p>
      )}
      {!ecoute && (erreur || motif) && (
        <p className="border-t border-outline/60 px-3 py-2 text-[11px] text-error" role="alert" data-testid="settings-captures-raccourci-erreur">
          {erreur || motif}
        </p>
      )}
    </div>
  );
}

/** Vignettes flottantes : application immédiate, allumées par défaut. Coupées, la pile se vide. */
const CLE_VIGNETTES = ["vignettes-actives"];

function VignettesRow() {
  const queryClient = useQueryClient();
  // Une fenêtre de réglages restée ouverte suit le réglage changé ailleurs.
  useEffect(() => {
    const ecoute = surReglageVignettes((actives) => queryClient.setQueryData(CLE_VIGNETTES, actives));
    return () => void ecoute.then((arreter) => arreter());
  }, [queryClient]);
  return (
    <InterrupteurRow
      cle={CLE_VIGNETTES}
      lire={vignettesActives}
      ecrire={vignettesActiver}
      defaut
      testid="settings-vignettes"
      icon="photo_library"
      label="Vignette flottante après une capture"
      description="Garde les dernières captures près de la pastille, à recopier ou à glisser."
    />
  );
}

/** La pile se range avec la pastille : application immédiate, décoché par défaut. */
function VignettesAvecPastilleRow() {
  return (
    <InterrupteurRow
      cle={["vignettes-avec-pastille"]}
      lire={vignettesAvecPastille}
      ecrire={vignettesRangerAvecPastille}
      defaut={false}
      testid="settings-vignettes-avec-pastille"
      icon="visibility_off"
      label="Ranger les vignettes avec la pastille"
      description="Ranger la pastille dans la zone de notification cache aussi la pile. Une capture fait revenir les deux."
    />
  );
}

/** Un interrupteur à application immédiate, avec « Enregistré » et retour au défaut. */
function InterrupteurRow({
  cle,
  lire,
  ecrire,
  defaut,
  testid,
  icon,
  label,
  description,
}: {
  cle: string[];
  lire: () => Promise<boolean>;
  ecrire: (actif: boolean) => Promise<boolean>;
  defaut: boolean;
  testid: string;
  icon: string;
  label: string;
  description: string;
}) {
  const queryClient = useQueryClient();
  const { data, error } = useQuery({ queryKey: cle, queryFn: lire, enabled: isTauri(), staleTime: 0 });
  const actives = data ?? null;
  const [erreurReglage, setErreur] = useState("");
  const erreur = erreurReglage || (error ? errorText(error) : "");
  const [enregistre, setEnregistre] = useState(false);
  function regler(actif: boolean) {
    setErreur("");
    ecrire(actif).then(
      (valeur) => {
        queryClient.setQueryData(cle, valeur);
        setEnregistre(true);
        window.setTimeout(() => setEnregistre(false), 2000);
      },
      (e) => setErreur(errorText(e)),
    );
  }
  return (
    <div data-testid={`${testid}-rangee`}>
      <SettingLine icon={icon} label={label} description={description} modified={actives === !defaut} onRevert={() => regler(defaut)}>
        <span className="text-[11px] text-primary" data-testid={`${testid}-etat`} aria-live="polite">
          {enregistre ? "Enregistré" : ""}
        </span>
        <button
          type="button"
          role="switch"
          data-testid={testid}
          aria-checked={actives === true}
          aria-label={label}
          disabled={actives === null}
          onClick={() => actives !== null && regler(!actives)}
          className={cn(
            "relative h-5 w-9 shrink-0 rounded-full border border-outline outline-none transition-[background-color] duration-150 focus-visible:border-ring focus-visible:ring-3 focus-visible:ring-ring/50 disabled:opacity-50",
            actives ? "bg-primary" : "bg-surface-high",
          )}
        >
          <span
            className={cn(
              "absolute top-0.5 left-0.5 size-3.5 rounded-full bg-on-surface transition-transform duration-150",
              actives && "translate-x-4 bg-bg",
            )}
          />
        </button>
      </SettingLine>
      {erreur && (
        <p className="border-t border-outline/60 px-3 py-2 text-[11px] text-error" role="alert" data-testid={`${testid}-erreur`}>
          {erreur}
        </p>
      )}
    </div>
  );
}

/**
 * La rangée montre le dossier et l'ouvre. Celui des captures se change
 * (ADR-020) : « Changer… » choisit, la rangée demande confirmation, puis Rust
 * déplace. Rien ne bouge avant « Déplacer ».
 */
function DossierRow({
  lire = capturesDossier,
  ouvrir = capturesOuvrirDossier,
  changeable = false,
  label = "Dossier des captures",
  testid = "settings-dossier",
}: {
  lire?: () => Promise<string>;
  ouvrir?: () => Promise<string>;
  changeable?: boolean;
  label?: string;
  testid?: string;
}) {
  const [chemin, setChemin] = useState("");
  const [erreur, setErreur] = useState("");
  const [propose, setPropose] = useState<Deplacement | null>(null);
  const [enCours, setEnCours] = useState(false);
  const [fait, setFait] = useState(false);
  useEffect(() => {
    if (isTauri()) lire().then(setChemin, (e) => setErreur(errorText(e)));
  }, [lire]);
  function changer() {
    setErreur("");
    setFait(false);
    capturesChoisirDossier().then(setPropose, (e) => setErreur(errorText(e)));
  }
  function deplacer(cible: string) {
    setEnCours(true);
    capturesDeplacerDossier(cible)
      .then(
        (nouveau) => {
          setChemin(nouveau);
          setPropose(null);
          setFait(true);
        },
        (e) => setErreur(errorText(e)),
      )
      .finally(() => setEnCours(false));
  }
  return (
    <div data-testid={`${testid}-rangee`}>
      <SettingLine icon="folder" label={label} description={chemin || "…"}>
        {fait && (
          <span className="text-[11px] text-primary" data-testid={`${testid}-etat`} aria-live="polite">
            Déplacé
          </span>
        )}
        {changeable && (
          <Button variant="outline" size="sm" data-testid={`${testid}-changer`} disabled={enCours} onClick={changer}>
            Changer…
          </Button>
        )}
        <Button
          variant="outline"
          size="sm"
          data-testid={`${testid}-ouvrir`}
          onClick={() => {
            setErreur("");
            ouvrir().then(setChemin, (e) => setErreur(errorText(e)));
          }}
        >
          Ouvrir
        </Button>
      </SettingLine>
      {propose && (
        <div className="flex items-center gap-2 border-t border-outline/60 px-3 py-2 text-[11px]" data-testid={`${testid}-confirmation`}>
          <p className="min-w-0 flex-1 break-all text-on-surface">
            {propose.fichiers === 0
              ? `Aucune capture à déplacer. Les prochaines iront dans ${propose.cible}.`
              : `Déplacer ${propose.fichiers} fichier${propose.fichiers > 1 ? "s" : ""} vers ${propose.cible} ?`}
          </p>
          <Button size="sm" data-testid={`${testid}-deplacer`} disabled={enCours} onClick={() => deplacer(propose.cible)}>
            {enCours ? "Déplacement…" : propose.fichiers === 0 ? "Utiliser" : "Déplacer"}
          </Button>
          <Button variant="outline" size="sm" data-testid={`${testid}-annuler`} disabled={enCours} onClick={() => setPropose(null)}>
            Annuler
          </Button>
        </div>
      )}
      {erreur && (
        <p className="border-t border-outline/60 px-3 py-2 text-[11px] text-error" role="alert">
          {erreur}
        </p>
      )}
    </div>
  );
}

/**
 * Mises à jour (ADR-020) : vérifiées au lancement puis chaque jour par Rust ;
 * « Rechercher » relance, « Installer » télécharge, installe et relance EquiShot.
 */
function MajRow() {
  const [etat, setEtat] = useState<EtatMaj | null>(null);
  const [erreur, setErreur] = useState("");
  const [occupe, setOccupe] = useState<"" | "recherche" | "installation">("");
  useEffect(() => {
    if (!isTauri()) return;
    majEtat().then(setEtat, (e) => setErreur(errorText(e)));
    const ecoute = surMaj(setEtat);
    return () => void ecoute.then((arreter) => arreter());
  }, []);
  function agir(quoi: "recherche" | "installation") {
    setErreur("");
    setOccupe(quoi);
    const action = quoi === "recherche" ? majRechercher().then(setEtat) : majInstaller();
    action.catch((e) => setErreur(errorText(e))).finally(() => setOccupe(""));
  }
  const description = !etat
    ? "…"
    : etat.dev
      ? "Pas de vérification dans l'instance de développement."
      : etat.disponible
        ? `La version ${etat.disponible} est disponible.`
        : etat.erreur
          ? etat.erreur
          : etat.verifiee
            ? `EquiShot ${version} est à jour (vérifié à ${etat.verifiee}).`
            : "Vérifiée au lancement, puis une fois par jour.";
  return (
    <div data-testid="settings-maj-rangee">
      <SettingLine icon="system_update" label="Mises à jour" description={description}>
        {etat?.disponible ? (
          <Button size="sm" data-testid="settings-maj-installer" disabled={occupe !== ""} onClick={() => agir("installation")}>
            {occupe === "installation" ? "Installation…" : "Installer"}
          </Button>
        ) : (
          <Button variant="outline" size="sm" data-testid="settings-maj-rechercher" disabled={occupe !== "" || !etat || etat.dev} onClick={() => agir("recherche")}>
            {occupe === "recherche" ? "Recherche…" : "Rechercher"}
          </Button>
        )}
      </SettingLine>
      {erreur && (
        <p className="border-t border-outline/60 px-3 py-2 text-[11px] text-error" role="alert" data-testid="settings-maj-erreur">
          {erreur}
        </p>
      )}
    </div>
  );
}

/** Un choix de la vidéo (ADR-016), relu puis réécrit avec les autres. */
function ChoixVideoRow({ choix, ...props }: { choix: "curseur"; defaut: boolean; testid: string; icon: string; label: string; description: string }) {
  return (
    <InterrupteurRow
      cle={["video-reglages", choix]}
      lire={() => videoReglages().then((r) => r[choix])}
      ecrire={async (actif) => (await videoRegler({ ...(await videoReglages()), [choix]: actif }))[choix]}
      {...props}
    />
  );
}

const CLE_SCENES = ["video-scenes"];

/** Une scène : renommer (édition explicite), supprimer (confirmé dans la rangée, sans fenêtre). */
function SceneRow({ id, nom, description, illisible = false }: { id: number; nom: string; description: string; illisible?: boolean }) {
  const queryClient = useQueryClient();
  const [edition, setEdition] = useState<string | null>(null);
  const [confirmer, setConfirmer] = useState(false);
  const [erreur, setErreur] = useState("");
  async function action(f: () => Promise<void>) {
    setErreur("");
    try {
      await f();
      await queryClient.invalidateQueries({ queryKey: CLE_SCENES });
    } catch (e) {
      setErreur(errorText(e));
    }
  }
  const renommer = () =>
    void action(async () => {
      await videoSceneRenommer(id, edition ?? nom);
      setEdition(null);
    });
  return (
    <div data-testid="settings-scene" data-id={id}>
      <SettingLine icon={illisible ? "error" : "movie"} label={nom} description={description}>
        {edition !== null ? (
          <>
            <input
              data-testid="settings-scene-nom"
              aria-label={`Nouveau nom de « ${nom} »`}
              autoFocus
              maxLength={40}
              value={edition}
              onChange={(e) => setEdition(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter") renommer();
                else if (e.key === "Escape") setEdition(null);
              }}
              className="h-7 w-40 rounded-md border border-outline bg-surface-high px-2 text-[12px] text-on-surface"
            />
            <Button variant="ghost" size="sm" data-testid="settings-scene-annuler" onClick={() => setEdition(null)}>
              Annuler
            </Button>
            <Button size="sm" data-testid="settings-scene-enregistrer" disabled={edition.trim() === nom} onClick={renommer}>
              Enregistrer
            </Button>
          </>
        ) : confirmer ? (
          <>
            <span className="text-[11px] text-on-surface">Supprimer ?</span>
            <Button variant="destructive" size="sm" data-testid="settings-scene-oui" onClick={() => void action(() => videoSceneSupprimer(id))}>
              Oui
            </Button>
            <Button variant="ghost" size="sm" data-testid="settings-scene-non" onClick={() => setConfirmer(false)}>
              Non
            </Button>
          </>
        ) : (
          <>
            {!illisible && (
              <Button variant="ghost" size="sm" data-testid="settings-scene-renommer" onClick={() => setEdition(nom)}>
                Renommer
              </Button>
            )}
            <Button variant="ghost" size="sm" data-testid="settings-scene-supprimer" onClick={() => setConfirmer(true)}>
              Supprimer
            </Button>
          </>
        )}
      </SettingLine>
      {erreur && (
        <p className="border-t border-outline/60 px-3 py-2 text-[11px] text-error" role="alert" data-testid="settings-scene-erreur">
          {erreur}
        </p>
      )}
    </div>
  );
}

/** Les scènes (0.19) : relues à chaque changement (`video://scenes`), d'où qu'il vienne. */
function ScenesRows() {
  const queryClient = useQueryClient();
  const { data, error } = useQuery({ queryKey: CLE_SCENES, queryFn: videoScenes, enabled: isTauri(), staleTime: 0 });
  useEffect(() => {
    if (!isTauri()) return;
    const ecoute = surScenes(() => void queryClient.invalidateQueries({ queryKey: CLE_SCENES }));
    return () => void ecoute.then((arreter) => arreter());
  }, [queryClient]);
  const liste = data ?? { scenes: [], illisibles: [] };
  return (
    <div data-testid="settings-scenes" className="divide-y divide-outline/60">
      {error && (
        <p className="px-3 py-2 text-[11px] text-error" role="alert">
          {errorText(error)}
        </p>
      )}
      {liste.scenes.length === 0 && liste.illisibles.length === 0 && (
        <SettingLine icon="movie" label="Scènes" description="Aucune scène. « Enregistrer comme scène », dans la mise en scène d'une vidéo, en crée une.">
          <span />
        </SettingLine>
      )}
      {liste.scenes.map((s) => (
        <SceneRow key={s.id} id={s.id} nom={s.nom} description={resumeScene(s)} />
      ))}
      {liste.illisibles.map((s) => (
        <SceneRow key={s.id} id={s.id} nom={s.nom} description={`Illisible, ignorée par les menus : ${s.motif}`} illisible />
      ))}
    </div>
  );
}

export function ReglagesView() {
  return (
    <ViewShell title="Réglages">
      <div className="mx-auto flex max-w-2xl flex-col gap-4">
        <Section titre="Capture" note="Application immédiate">
          <RaccourciCaptureRow />
          <VignettesRow />
          <VignettesAvecPastilleRow />
          <DossierRow changeable />
        </Section>
        <Section titre="Application" note="Application immédiate">
          <InterrupteurRow
            cle={["demarrage"]}
            lire={demarrageLire}
            ecrire={demarrageRegler}
            defaut
            testid="settings-demarrage"
            icon="power_settings_new"
            label="Lancer EquiShot avec Windows"
            description="EquiShot démarre à l'ouverture de la session, prêt à capturer."
          />
          <MajRow />
        </Section>
        <Section titre="Vidéo" note="Pour le prochain enregistrement">
          <ChoixVideoRow choix="curseur" defaut testid="settings-video-curseur" icon="arrow_selector_tool" label="Curseur visible" description="La flèche de la souris apparaît dans la vidéo." />
          <DossierRow lire={videoDossier} ouvrir={videoOuvrirDossier} label="Dossier des vidéos" testid="settings-video-dossier" />
          <ScenesRows />
        </Section>
        <p data-testid="settings-version" className="text-center text-[11px] text-muted">
          EquiShot {version}
        </p>
      </div>
    </ViewShell>
  );
}
