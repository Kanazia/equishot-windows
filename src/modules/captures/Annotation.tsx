import { useEffect, useImperativeHandle, useLayoutEffect, useRef, useState, type Ref } from "react";

import { Icon } from "@/components/Icon";
import { Button } from "@/components/ui/button";
import { cn } from "@/lib/utils";
import {
  PALETTE,
  SYMBOLES,
  TAILLES,
  TAILLE_TEXTE,
  annuler,
  boite,
  deplacer,
  dessinerFormes,
  engager,
  formeSous,
  historique,
  matriceOrientation,
  numeroSuivant,
  poignees,
  police,
  remanierHistorique,
  restyler,
  retablir,
  simplifierTrace,
  styleDe,
  tirer,
  type Forme,
  type Mesure,
  type Outil,
  type Poignee,
  type Taille,
  type Zone,
} from "@/modules/captures/formes";

/** L'outil, la couleur et la taille choisis : retenus d'une capture à l'autre (ADR-005). */
export interface Choix {
  outil: Outil;
  couleur: string;
  taille: Taille;
  /** Le symbole du prochain tampon (ADR-014). */
  symbole: string;
}

const OUTILS: { outil: Outil; libelle: string; icone: string; touche: string }[] = [
  { outil: "fleche", libelle: "Flèche", icone: "north_east", touche: "F" },
  { outil: "rectangle", libelle: "Rectangle", icone: "rectangle", touche: "R" },
  { outil: "ellipse", libelle: "Ellipse", icone: "circle", touche: "E" },
  { outil: "ligne", libelle: "Ligne", icone: "horizontal_rule", touche: "L" },
  { outil: "crayon", libelle: "Crayon", icone: "draw", touche: "C" },
  { outil: "surligneur", libelle: "Surligneur", icone: "ink_highlighter", touche: "S" },
  { outil: "texte", libelle: "Texte", icone: "title", touche: "T" },
  { outil: "etape", libelle: "Étape numérotée", icone: "counter_1", touche: "N" },
  { outil: "pixellisation", libelle: "Pixellisation", icone: "blur_on", touche: "P" },
  { outil: "projecteur", libelle: "Projecteur", icone: "highlight", touche: "M" },
  { outil: "loupe", libelle: "Loupe", icone: "zoom_in", touche: "G" },
  { outil: "tampon", libelle: "Tampon", icone: "add_reaction", touche: "A" },
];

const CLE_CHOIX = "equishot.annotation";
/** Points d'un tracé de crayon au plus : miroir de `stockage::POINTS_MAX`. */
const POINTS_MAX = 4000;
const CHOIX_DEFAUT: Choix = { outil: "fleche", couleur: PALETTE[0].couleur, taille: 2, symbole: SYMBOLES[0].symbole };
/** Portée d'un clic sur une forme, et d'une poignée, en pixels CSS. */
const MARGE_CLIC = 6;
const MARGE_POIGNEE = 8;

function lireChoix(): Choix {
  try {
    const lu = JSON.parse(localStorage.getItem(CLE_CHOIX) ?? "{}") as Partial<Choix>;
    return {
      outil: OUTILS.some((o) => o.outil === lu.outil) ? (lu.outil as Outil) : CHOIX_DEFAUT.outil,
      couleur: PALETTE.some((p) => p.couleur === lu.couleur) ? (lu.couleur as string) : CHOIX_DEFAUT.couleur,
      taille: lu.taille === 1 || lu.taille === 2 || lu.taille === 3 ? lu.taille : CHOIX_DEFAUT.taille,
      symbole: SYMBOLES.some((s) => s.symbole === lu.symbole) ? (lu.symbole as string) : CHOIX_DEFAUT.symbole,
    };
  } catch {
    return CHOIX_DEFAUT;
  }
}

/** Le choix courant, lu au montage et écrit à chaque changement. */
function useChoixAnnotation(): [Choix, (changement: Partial<Choix>) => void] {
  const [choix, setChoix] = useState(lireChoix);
  return [
    choix,
    (changement) =>
      setChoix((avant) => {
        const apres = { ...avant, ...changement };
        try {
          localStorage.setItem(CLE_CHOIX, JSON.stringify(apres));
        } catch {
          // Stockage indisponible : le choix vaut pour cette fenêtre seulement.
        }
        return apres;
      }),
  ];
}

/** Les formes, leur historique, la forme choisie et le choix d'outil (ADR-006). */
export interface Edition {
  formes: Forme[];
  choix: Choix;
  /** Indice de la forme choisie, ou `null`. */
  choisie: number | null;
  peutAnnuler: boolean;
  peutRetablir: boolean;
  /** Un nouvel état des formes, annulable. */
  poser: (formes: Forme[]) => void;
  choisirForme: (indice: number | null) => void;
  /** Change l'outil (et lâche la forme), ou le style (et l'applique à la forme choisie). */
  choisir: (changement: Partial<Choix>) => void;
  annuler: () => void;
  retablir: () => void;
  supprimerChoisie: () => void;
  /** Repart de zéro, sans historique : une zone retracée. */
  vider: () => void;
  /** La même fonction sur le passé, le présent et le futur, sans engager de pas (ADR-018). */
  remanier: (f: (formes: Forme[]) => Forme[]) => void;
}

/**
 * L'état d'une annotation, et ses touches : Ctrl+Z, Ctrl+Y (ou Ctrl+Maj+Z), et
 * avec une forme choisie Suppr, Retour arrière et Échap. Écoutées en phase de
 * capture : ce qui est traité ici n'atteint pas les gestes de la page (Suppr
 * jette la capture, Échap ferme la sélection).
 */
export function useAnnotation(initiales: Forme[] = []): Edition {
  const [h, setH] = useState(() => historique(initiales));
  const [choisie, setChoisie] = useState<number | null>(null);
  const [choix, setChoix] = useChoixAnnotation();

  const edition: Edition = {
    formes: h.present,
    choix,
    choisie: choisie !== null && choisie < h.present.length ? choisie : null,
    peutAnnuler: h.passe.length > 0,
    peutRetablir: h.futur.length > 0,
    poser: (formes) => setH((avant) => engager(avant, formes)),
    choisirForme: setChoisie,
    choisir: (changement) => {
      setChoix(changement);
      if (changement.outil) {
        setChoisie(null);
      } else if (choisie !== null) {
        const style = { ...(changement.couleur && { couleur: changement.couleur }), ...(changement.taille && { taille: changement.taille }) };
        const { symbole } = changement;
        const refaite = (f: Forme) => {
          const r = restyler(f, style);
          // Un symbole choisi remplace celui du tampon choisi (ADR-014).
          return symbole && r.type === "tampon" ? { ...r, symbole } : r;
        };
        setH((avant) => engager(avant, avant.present.map((f, i) => (i === choisie ? refaite(f) : f))));
      }
    },
    annuler: () => {
      setChoisie(null);
      setH(annuler);
    },
    retablir: () => {
      setChoisie(null);
      setH(retablir);
    },
    supprimerChoisie: () => {
      if (choisie === null) return;
      setChoisie(null);
      setH((avant) => engager(avant, avant.present.filter((_, i) => i !== choisie)));
    },
    vider: () => {
      setChoisie(null);
      setH(historique([]));
    },
    remanier: (f) => setH((avant) => remanierHistorique(avant, f)),
  };
  const courante = useRef(edition);
  courante.current = edition;

  useEffect(() => {
    function surTouche(evenement: KeyboardEvent) {
      const cible = evenement.target as HTMLElement | null;
      if (cible && (cible.tagName === "INPUT" || cible.tagName === "TEXTAREA" || cible.isContentEditable)) return;
      const e = courante.current;
      const touche = evenement.key.toLowerCase();
      if (evenement.ctrlKey && touche === "z" && !evenement.shiftKey) e.annuler();
      else if (evenement.ctrlKey && (touche === "y" || touche === "z")) e.retablir();
      else if (e.choisie !== null && (evenement.key === "Delete" || evenement.key === "Backspace")) e.supprimerChoisie();
      else if (e.choisie !== null && evenement.key === "Escape") e.choisirForme(null);
      else return;
      evenement.preventDefault();
      evenement.stopImmediatePropagation();
    }
    window.addEventListener("keydown", surTouche, true);
    return () => window.removeEventListener("keydown", surTouche, true);
  }, []);

  return edition;
}

export interface AnnotationPoignee {
  /** L'image annotée telle qu'affichée, en PNG encodé en base64. */
  aplatir: () => string;
  /** La largeur d'un texte à `px` pixels, mesurée par le canevas : l'orientation d'un texte s'en sert (ADR-018). */
  mesurer: Mesure;
}

interface AnnotationProps {
  ref?: Ref<AnnotationPoignee>;
  image: HTMLImageElement;
  /** Partie de l'image montrée, en pixels de l'image. */
  zone: Zone;
  /** Échelle de l'écran d'origine : épaisseur des traits et taille du texte. */
  echelle: number;
  edition: Edition;
  /** L'orientation affichée (ADR-018) : 0 hors de la réédition, l'image est dessinée telle quelle. */
  orientation?: number;
  className?: string;
}

interface Point {
  x: number;
  y: number;
}

/** Ce que tire l'appui en cours sur une forme posée. */
type Geste = { forme: number; origine: Forme; depart: Point; poignee: Poignee | null };

const CURSEUR_POIGNEE: Record<Poignee, string> = { no: "nwse-resize", se: "nwse-resize", ne: "nesw-resize", so: "nesw-resize", a: "grab", b: "grab" };

/** La forme tirée jusqu'au point : déplacée par son corps, redimensionnée par une poignée. */
function suivre(geste: Geste, p: Point): Forme {
  return geste.poignee ? tirer(geste.origine, geste.poignee, p) : deplacer(geste.origine, p.x - geste.depart.x, p.y - geste.depart.y);
}

/**
 * Le canevas d'annotation, partagé par la fenêtre de sélection et par la
 * réédition depuis la galerie. Il porte l'image et ses formes à la résolution
 * de l'image : l'aplatir, c'est lire ce qu'il affiche. Le cadre de la forme
 * choisie est un calque HTML, que l'aplatissement ne voit pas.
 */
export function Annotation({ ref, image, zone, echelle, edition, orientation = 0, className }: AnnotationProps) {
  const canevas = useRef<HTMLCanvasElement>(null);
  const champ = useRef<HTMLInputElement>(null);
  const depart = useRef<Point | null>(null);
  /** Les points du crayon en cours, à plat. */
  const trace = useRef<number[]>([]);
  const geste = useRef<Geste | null>(null);
  /** L'appui a choisi, tiré ou lâché une forme : le clic qui suit ne pose rien. */
  const clicPris = useRef(false);
  const saisieEnCours = useRef<Point | null>(null);
  const [brouillon, setBrouillon] = useState<Forme | null>(null);
  /** La forme choisie telle qu'on la tire, avant de la poser. */
  const [apercu, setApercu] = useState<Forme | null>(null);
  const [saisie, setSaisie] = useState<(Point & { ratio: number }) | null>(null);
  const [curseur, setCurseur] = useState<string | null>(null);
  const { formes, choix, choisie } = edition;
  const { outil, couleur, taille } = choix;
  const style = { couleur, taille };

  const affichees = apercu && choisie !== null ? formes.map((f, i) => (i === choisie ? apercu : f)) : formes;

  // En layout : le canevas est à jour avant tout clic sur « Enregistrer ».
  useLayoutEffect(() => {
    const ctx = canevas.current?.getContext("2d", { willReadFrequently: true });
    if (!ctx) return;
    ctx.clearRect(0, 0, zone.largeur, zone.hauteur);
    if (orientation === 0) {
      ctx.drawImage(image, zone.x, zone.y, zone.largeur, zone.hauteur, 0, 0, zone.largeur, zone.hauteur);
    } else {
      // ADR-018 : l'image entière, orientée au dessin ; la zone est alors sa taille orientée.
      // ponytail: deux branches plutôt qu'une matrice qui porterait aussi la zone de la sélection.
      ctx.setTransform(...matriceOrientation(orientation, image.naturalWidth, image.naturalHeight));
      ctx.drawImage(image, 0, 0);
      ctx.setTransform(1, 0, 0, 1, 0, 0);
    }
    dessinerFormes(ctx, brouillon ? [...affichees, brouillon] : affichees, echelle);
  }, [image, zone.x, zone.y, zone.largeur, zone.hauteur, affichees, brouillon, echelle, orientation]);

  /** Ratio d'affichage : pixels CSS par pixel du canevas. */
  const ratio = () => {
    const c = canevas.current;
    return c && c.width > 0 ? c.getBoundingClientRect().width / c.width : 1;
  };

  const mesure: Mesure = (texte, px) => {
    const ctx = canevas.current?.getContext("2d");
    if (!ctx) return texte.length * px * 0.6;
    ctx.font = police(px);
    return ctx.measureText(texte).width;
  };

  useImperativeHandle(ref, () => ({
    aplatir: () => canevas.current?.toDataURL("image/png").split(",")[1] ?? "",
    mesurer: mesure,
  }));

  function point(evenement: { clientX: number; clientY: number }): Point {
    const cadre = canevas.current?.getBoundingClientRect();
    if (!cadre) return { x: 0, y: 0 };
    const r = ratio();
    return { x: (evenement.clientX - cadre.left) / r, y: (evenement.clientY - cadre.top) / r };
  }

  /** La poignée de la forme choisie sous le point, s'il y en a une. */
  function poigneeSous(p: Point): Poignee | null {
    if (choisie === null) return null;
    const portee = MARGE_POIGNEE / ratio();
    return poignees(formes[choisie]).find((q) => Math.hypot(q.x - p.x, q.y - p.y) <= portee)?.id ?? null;
  }

  const sous = (p: Point) => formeSous(formes, p, echelle, MARGE_CLIC / ratio(), mesure);

  function formeEntre(a: Point, b: Point): Forme | null {
    if (outil === "rectangle" || outil === "ellipse" || outil === "pixellisation" || outil === "projecteur" || outil === "loupe") {
      return { type: outil, x: Math.min(a.x, b.x), y: Math.min(a.y, b.y), largeur: Math.abs(b.x - a.x), hauteur: Math.abs(b.y - a.y), ...style };
    }
    if (outil === "fleche" || outil === "surligneur" || outil === "ligne") return { type: outil, x1: a.x, y1: a.y, x2: b.x, y2: b.y, ...style };
    if (outil === "crayon") return { type: "crayon", points: [...trace.current], ...style };
    return null;
  }

  function validerTexte() {
    const position = saisieEnCours.current;
    if (!position) return;
    saisieEnCours.current = null;
    setSaisie(null);
    const texte = champ.current?.value.trim() ?? "";
    if (texte) edition.poser([...formes, { type: "texte", x: position.x, y: position.y, texte, ...style }]);
  }

  /** Arrondi au dixième de pixel : un tracé de crayon n'a pas besoin de plus. */
  const net = (n: number) => Math.round(n * 10) / 10;

  const choisieAffichee = choisie !== null ? affichees[choisie] : null;
  const pourcent = (v: number, total: number) => `${(v / total) * 100}%`;

  return (
    // Un appui sur l'annotation, quel que soit l'outil, ne retrace pas la zone qui la porte.
    <div className={cn("relative", className)} onPointerDown={(evenement) => evenement.stopPropagation()}>
      <canvas
        ref={canevas}
        width={zone.largeur}
        height={zone.hauteur}
        data-testid="annotation-canevas"
        className="block size-full"
        style={{ cursor: curseur ?? (outil === "texte" ? "text" : "crosshair") }}
        onPointerDown={(evenement) => {
          clicPris.current = false;
          if (evenement.button !== 0) return;
          const p = point(evenement);
          const poignee = poigneeSous(p);
          const touchee = poignee !== null ? choisie : sous(p);
          if (touchee !== null) {
            edition.choisirForme(touchee);
            geste.current = { forme: touchee, origine: formes[touchee], depart: p, poignee };
            clicPris.current = true;
            evenement.currentTarget.setPointerCapture(evenement.pointerId);
            return;
          }
          // Hors de toute forme, l'appui lâche la forme choisie, sans rien tracer.
          if (choisie !== null) {
            edition.choisirForme(null);
            clicPris.current = true;
            return;
          }
          if (outil === "texte" || outil === "etape" || outil === "tampon") return;
          depart.current = p;
          trace.current = [net(p.x), net(p.y)];
          evenement.currentTarget.setPointerCapture(evenement.pointerId);
        }}
        onPointerMove={(evenement) => {
          const p = point(evenement);
          if (geste.current) {
            setApercu(suivre(geste.current, p));
          } else if (depart.current) {
            if (outil === "crayon") trace.current.push(net(p.x), net(p.y));
            setBrouillon(formeEntre(depart.current, p));
          } else {
            const poignee = poigneeSous(p);
            setCurseur(poignee ? CURSEUR_POIGNEE[poignee] : sous(p) !== null ? "move" : null);
          }
        }}
        onPointerUp={(evenement) => {
          const g = geste.current;
          if (g) {
            geste.current = null;
            setApercu(null);
            const fin = point(evenement);
            // Un clic sans glisser choisit la forme, sans rien changer.
            if (Math.hypot(fin.x - g.depart.x, fin.y - g.depart.y) * ratio() >= 2) {
              const tiree = suivre(g, fin);
              edition.poser(formes.map((f, i) => (i === g.forme ? tiree : f)));
            }
            return;
          }
          const debut = depart.current;
          if (!debut) return;
          depart.current = null;
          setBrouillon(null);
          const fin = point(evenement);
          if (outil === "crayon") {
            trace.current.push(net(fin.x), net(fin.y));
            // Un clic au crayon laisse un point ; un tracé perd ses points trop serrés,
            // et sous la borne de Rust (4 000 points) en élargissant l'écart.
            let ecart = 2 * echelle;
            let points = simplifierTrace(trace.current, ecart);
            while (points.length > 2 * POINTS_MAX) points = simplifierTrace(points, (ecart *= 2));
            edition.poser([...formes, { type: "crayon", points, ...style }]);
            return;
          }
          const forme = formeEntre(debut, fin);
          // Un clic sans glisser ne laisse pas un point sur la capture.
          if (forme && Math.hypot(fin.x - debut.x, fin.y - debut.y) >= 4 * echelle) edition.poser([...formes, forme]);
        }}
        // Le texte, l'étape et le tampon se posent au clic, pas à l'appui : l'appui rendrait le focus à la page.
        onClick={(evenement) => {
          if (clicPris.current) {
            clicPris.current = false;
            return;
          }
          const position = point(evenement);
          if (outil === "etape") {
            edition.poser([...formes, { type: "etape", x: net(position.x), y: net(position.y), numero: numeroSuivant(formes), ...style }]);
          } else if (outil === "tampon") {
            edition.poser([...formes, { type: "tampon", x: net(position.x), y: net(position.y), symbole: choix.symbole, ...style }]);
          } else if (outil === "texte") {
            saisieEnCours.current = position;
            setSaisie({ ...position, ratio: ratio() });
          }
        }}
      />
      {choisieAffichee &&
        (() => {
          const b = boite(choisieAffichee, echelle, mesure);
          return (
            <div className="pointer-events-none absolute inset-0">
              <div
                data-testid="annotation-choisie"
                data-forme={choisieAffichee.type}
                className="absolute -m-1.5 border border-dashed border-primary"
                style={{
                  left: pourcent(b.x, zone.largeur),
                  top: pourcent(b.y, zone.hauteur),
                  width: `calc(${pourcent(b.largeur, zone.largeur)} + 12px)`,
                  height: `calc(${pourcent(b.hauteur, zone.hauteur)} + 12px)`,
                }}
              />
              {poignees(choisieAffichee).map((q) => (
                <span
                  key={q.id}
                  data-testid={`annotation-poignee-${q.id}`}
                  className="absolute size-2.5 -translate-1/2 rounded-sm border border-primary bg-white"
                  style={{ left: pourcent(q.x, zone.largeur), top: pourcent(q.y, zone.hauteur) }}
                />
              ))}
            </div>
          );
        })()}
      {saisie && (
        <input
          ref={champ}
          autoFocus
          aria-label="Texte de l'annotation"
          data-testid="annotation-texte"
          className="absolute min-w-24 border border-dashed bg-transparent p-0 font-semibold leading-none outline-none"
          style={{
            left: saisie.x * saisie.ratio,
            top: saisie.y * saisie.ratio,
            fontSize: TAILLE_TEXTE * TAILLES[taille].facteur * echelle * saisie.ratio,
            color: couleur,
            borderColor: couleur,
          }}
          onBlur={validerTexte}
          onKeyDown={(evenement) => {
            // Entrée et Échap valent pour le texte, pas pour la capture.
            evenement.stopPropagation();
            if (evenement.key === "Enter") validerTexte();
            if (evenement.key === "Escape") {
              saisieEnCours.current = null;
              setSaisie(null);
            }
          }}
        />
      )}
    </div>
  );
}

interface BarreAnnotationProps {
  edition: Edition;
  /** Côté où s'ouvre la palette : vers le haut par défaut. */
  palette?: "haut" | "bas";
  /** Les actions de fin, à droite des outils. */
  children: React.ReactNode;
  ref?: Ref<HTMLDivElement>;
  className?: string;
  style?: React.CSSProperties;
}

/** Les outils, le style, Annuler, Rétablir et la suppression de la forme choisie, puis les actions de fin. */
export function BarreAnnotation({ edition, palette = "haut", children, ref, className, style }: BarreAnnotationProps) {
  const [ouverte, setOuverte] = useState(false);
  const styleRef = useRef<HTMLDivElement>(null);
  const choisirCourant = useRef(edition.choisir);
  choisirCourant.current = edition.choisir;
  const { choix, choisie, formes } = edition;
  // La palette montre le style de la forme choisie : un clic le lui change.
  const montre = choisie !== null ? styleDe(formes[choisie]) : choix;
  // Les symboles ne se montrent que pour un tampon : l'outil, ou le tampon choisi.
  const formeChoisie = choisie !== null ? formes[choisie] : null;
  const tamponMontre = formeChoisie?.type === "tampon" ? formeChoisie.symbole : formeChoisie === null && choix.outil === "tampon" ? choix.symbole : null;

  // Une lettre choisit l'outil, sauf dans un champ de saisie ou avec un modificateur.
  useEffect(() => {
    function surTouche(evenement: KeyboardEvent) {
      if (evenement.ctrlKey || evenement.altKey || evenement.metaKey) return;
      const cible = evenement.target as HTMLElement | null;
      if (cible && (cible.tagName === "INPUT" || cible.tagName === "TEXTAREA" || cible.isContentEditable)) return;
      const o = OUTILS.find((x) => x.touche === evenement.key.toUpperCase());
      if (o) choisirCourant.current({ outil: o.outil });
    }
    window.addEventListener("keydown", surTouche);
    return () => window.removeEventListener("keydown", surTouche);
  }, []);

  // La palette se referme sur un appui hors d'elle.
  useEffect(() => {
    if (!ouverte) return;
    function surAppui(evenement: PointerEvent) {
      if (!styleRef.current?.contains(evenement.target as Node)) setOuverte(false);
    }
    window.addEventListener("pointerdown", surAppui, true);
    return () => window.removeEventListener("pointerdown", surAppui, true);
  }, [ouverte]);

  return (
    <div
      ref={ref}
      role="toolbar"
      aria-label="Outils d'annotation"
      data-testid="annotation-barre"
      className={cn("flex items-center gap-1 rounded-lg border border-outline bg-surface p-1 shadow-lg shadow-black/40", className)}
      style={style}
      // La barre ne retrace pas la zone qu'elle borde.
      onPointerDown={(evenement) => evenement.stopPropagation()}
    >
      {OUTILS.map((o) => (
        <Button
          key={o.outil}
          variant="ghost"
          size="icon"
          title={`${o.libelle} (${o.touche})`}
          aria-label={o.libelle}
          aria-pressed={choix.outil === o.outil}
          data-testid={`annotation-outil-${o.outil}`}
          className={choix.outil === o.outil ? "bg-surface-highest text-primary" : "text-on-surface-variant"}
          onClick={() => edition.choisir({ outil: o.outil })}
        >
          <Icon name={o.icone} size={16} />
        </Button>
      ))}
      <span className="mx-0.5 h-4 w-px bg-outline" />
      <div ref={styleRef} className="relative">
        <Button
          variant="ghost"
          size="icon"
          title={choisie !== null ? "Couleur et taille de la forme" : "Couleur et taille"}
          aria-label="Couleur et taille"
          aria-expanded={ouverte}
          data-testid="annotation-style"
          onClick={() => setOuverte((o) => !o)}
        >
          <span
            className="rounded-full ring-1 ring-white/40"
            style={{ backgroundColor: montre.couleur, width: 6 + montre.taille * 4, height: 6 + montre.taille * 4 }}
          />
        </Button>
        {ouverte && (
          <div
            data-testid="annotation-palette"
            className={cn(
              "absolute left-0 z-10 flex flex-col gap-1.5 rounded-lg border border-outline bg-surface p-2 shadow-lg shadow-black/40",
              palette === "haut" ? "bottom-full mb-2" : "top-full mt-2",
            )}
          >
            <div className="flex gap-1">
              {PALETTE.map((p) => (
                <button
                  key={p.couleur}
                  type="button"
                  title={p.nom}
                  aria-label={p.nom}
                  aria-pressed={montre.couleur === p.couleur}
                  data-testid={`annotation-couleur-${p.couleur.slice(1)}`}
                  className={cn(
                    "size-6 rounded-full ring-1 ring-white/30 transition-transform hover:scale-110",
                    montre.couleur === p.couleur && "ring-2 ring-primary ring-offset-2 ring-offset-surface",
                  )}
                  style={{ backgroundColor: p.couleur }}
                  onClick={() => edition.choisir({ couleur: p.couleur })}
                />
              ))}
            </div>
            <div className="flex gap-1">
              {([1, 2, 3] as const).map((t) => (
                <button
                  key={t}
                  type="button"
                  title={`Taille ${TAILLES[t].nom.toLowerCase()}`}
                  aria-label={`Taille ${TAILLES[t].nom.toLowerCase()}`}
                  aria-pressed={montre.taille === t}
                  data-testid={`annotation-taille-${t}`}
                  className={cn(
                    "flex h-7 flex-1 items-center justify-center rounded-md text-on-surface-variant hover:bg-surface-highest",
                    montre.taille === t && "bg-surface-highest text-primary",
                  )}
                  onClick={() => edition.choisir({ taille: t })}
                >
                  <span className="rounded-full bg-current" style={{ width: 2 + t * 4, height: 2 + t * 4 }} />
                </button>
              ))}
            </div>
            {tamponMontre !== null && (
              <div className="grid grid-cols-6 gap-1" data-testid="annotation-symboles">
                {SYMBOLES.map((s, i) => (
                  <button
                    key={s.symbole}
                    type="button"
                    title={s.nom}
                    aria-label={s.nom}
                    aria-pressed={tamponMontre === s.symbole}
                    data-testid={`annotation-symbole-${i}`}
                    className={cn(
                      "flex size-7 items-center justify-center rounded-md text-base hover:bg-surface-highest",
                      tamponMontre === s.symbole && "bg-surface-highest ring-1 ring-primary",
                    )}
                    style={{ fontFamily: '"Segoe UI Emoji", sans-serif' }}
                    onClick={() => edition.choisir({ symbole: s.symbole })}
                  >
                    {s.symbole}
                  </button>
                ))}
              </div>
            )}
          </div>
        )}
      </div>
      <Button
        variant="ghost"
        size="icon"
        title="Annuler (Ctrl+Z)"
        aria-label="Annuler"
        data-testid="annotation-annuler"
        disabled={!edition.peutAnnuler}
        onClick={edition.annuler}
      >
        <Icon name="undo" size={16} />
      </Button>
      <Button
        variant="ghost"
        size="icon"
        title="Rétablir (Ctrl+Y)"
        aria-label="Rétablir"
        data-testid="annotation-retablir"
        disabled={!edition.peutRetablir}
        onClick={edition.retablir}
      >
        <Icon name="redo" size={16} />
      </Button>
      {choisie !== null && (
        <Button
          variant="ghost"
          size="icon"
          title="Supprimer la forme (Suppr)"
          aria-label="Supprimer la forme"
          data-testid="annotation-supprimer-forme"
          className="text-on-surface-variant hover:text-error"
          onClick={edition.supprimerChoisie}
        >
          <Icon name="delete" size={16} />
        </Button>
      )}
      <span className="mx-0.5 h-4 w-px bg-outline" />
      {children}
    </div>
  );
}
