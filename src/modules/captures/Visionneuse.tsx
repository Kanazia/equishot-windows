import { useEffect, useRef, useState, type CSSProperties, type PointerEvent } from "react";

import { Icon } from "@/components/Icon";

type Vue = { z: number; x: number; y: number };
const AJUSTEE: Vue = { z: 1, x: 0, y: 0 };

/**
 * Zoom à la molette vers le pointeur, comme une visionneuse d'images. Le cadre centre son
 * contenu (flex) ; la vue zoomée se déplace au bouton du milieu, et aussi au bouton gauche
 * quand `glisserGauche` (en consultation : le gauche sert à annoter en modification).
 * `largeur100` : la largeur CSS du contenu à taille réelle, pour le pourcentage affiché.
 */
export function useVisionneuse({ largeur100, glisserGauche = false }: { largeur100: number; glisserGauche?: boolean }) {
  const [cadre, setCadre] = useState<HTMLElement | null>(null);
  const contenu = useRef<HTMLElement | null>(null);
  const [vue, setVue] = useState(AJUSTEE);
  const vueActuelle = useRef(vue);
  vueActuelle.current = vue;
  const prise = useRef<{ id: number; x: number; y: number; vx: number; vy: number } | null>(null);

  // Le contenu ne quitte pas le cadre : il ne bouge que de ce qui dépasse.
  const borner = (v: Vue): Vue => {
    const d = contenu.current;
    if (!cadre || !d) return v;
    const bx = Math.max(0, (d.offsetWidth * v.z - cadre.clientWidth) / 2);
    const by = Math.max(0, (d.offsetHeight * v.z - cadre.clientHeight) / 2);
    return { z: v.z, x: Math.min(bx, Math.max(-bx, v.x)), y: Math.min(by, Math.max(-by, v.y)) };
  };

  // Zoome d'un facteur en gardant immobile le point sous (clientX, clientY).
  const zoomer = (facteur: number, clientX: number, clientY: number) => {
    const d = contenu.current;
    if (!cadre || !d || d.offsetWidth === 0) return;
    const v = vueActuelle.current;
    // ponytail: plafond fixe ×8, ou deux fois la largeur du cadre pour une toute petite capture.
    const max = Math.max(8, (2 * cadre.clientWidth) / d.offsetWidth, (2 * cadre.clientHeight) / d.offsetHeight);
    const z = Math.min(max, Math.max(1, v.z * facteur));
    const r = cadre.getBoundingClientRect();
    const sx = clientX - (r.left + r.width / 2);
    const sy = clientY - (r.top + r.height / 2);
    const k = z / v.z;
    setVue(borner({ z, x: sx - k * (sx - v.x), y: sy - k * (sy - v.y) }));
  };
  const zoomerActuel = useRef(zoomer);
  zoomerActuel.current = zoomer;

  // Écouteur natif non passif : sans preventDefault, Ctrl + molette zoomerait toute la page.
  useEffect(() => {
    if (!cadre) return;
    function molette(evenement: WheelEvent) {
      evenement.preventDefault();
      zoomerActuel.current(Math.exp(-evenement.deltaY * 0.0015), evenement.clientX, evenement.clientY);
    }
    cadre.addEventListener("wheel", molette, { passive: false });
    return () => cadre.removeEventListener("wheel", molette);
  }, [cadre]);

  const zoomee = vue.z > 1.001;
  const lacher = (evenement: PointerEvent) => {
    if (prise.current?.id === evenement.pointerId) prise.current = null;
  };

  const surCadre = {
    // En capture : l'annotation arrête la propagation de ses appuis.
    onPointerDownCapture: (evenement: PointerEvent<HTMLElement>) => {
      if (!zoomee || !(evenement.button === 1 || (glisserGauche && evenement.button === 0))) return;
      if ((evenement.target as Element).closest("[data-visionneuse-ajuster]")) return;
      evenement.preventDefault();
      evenement.stopPropagation();
      evenement.currentTarget.setPointerCapture(evenement.pointerId);
      const v = vueActuelle.current;
      prise.current = { id: evenement.pointerId, x: evenement.clientX, y: evenement.clientY, vx: v.x, vy: v.y };
    },
    onPointerMove: (evenement: PointerEvent<HTMLElement>) => {
      const p = prise.current;
      if (!p || p.id !== evenement.pointerId) return;
      setVue((v) => borner({ z: v.z, x: p.vx + evenement.clientX - p.x, y: p.vy + evenement.clientY - p.y }));
    },
    onPointerUp: lacher,
    onPointerCancel: lacher,
    // Le bouton du milieu ne lance pas le défilement automatique de la WebView.
    onMouseDown: (evenement: { button: number; preventDefault: () => void }) => {
      if (evenement.button === 1) evenement.preventDefault();
    },
    onDoubleClick: glisserGauche
      ? (evenement: { clientX: number; clientY: number }) =>
          zoomee ? setVue(AJUSTEE) : zoomer(2.5, evenement.clientX, evenement.clientY)
      : undefined,
  };

  const style: CSSProperties = {
    transform: zoomee ? `translate(${vue.x}px, ${vue.y}px) scale(${vue.z})` : undefined,
    cursor: zoomee && glisserGauche ? (prise.current ? "grabbing" : "grab") : undefined,
  };

  const largeurAffichee = (contenu.current?.offsetWidth ?? 0) * vue.z;
  const ajuster = zoomee && (
    <button
      type="button"
      data-visionneuse-ajuster
      data-testid="visionneuse-ajuster"
      title={glisserGauche ? "Revenir à la taille de la fenêtre (double-clic)" : "Revenir à la taille de la fenêtre"}
      onClick={() => setVue(AJUSTEE)}
      className="absolute right-2 bottom-2 z-10 flex items-center gap-1 rounded-md border border-outline bg-surface/90 px-2 py-1 text-[11px] text-on-surface tabular-nums shadow-sm transition-transform duration-150 ease-out active:scale-[0.97]"
    >
      <Icon name="fit_screen" size={14} />
      {largeur100 > 0 ? `${Math.round((largeurAffichee / largeur100) * 100)} %` : "Ajuster"}
    </button>
  );

  return { cadre: setCadre, contenu: (el: HTMLElement | null) => void (contenu.current = el), surCadre, style, ajuster, zoom: vue.z };
}
