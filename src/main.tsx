/**
 * Fenêtre « EquiShot », ouverte à la demande depuis la pastille ou l'icône :
 * la galerie et les réglages, choisis par le hash (`#galerie`, `#reglages`).
 */
import { QueryClient, QueryClientProvider, useQueryClient } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import ReactDOM from "react-dom/client";

import "@fontsource/inter/500.css";
import "@fontsource/inter/600.css";
import "@fontsource/jetbrains-mono/400.css";
import "@fontsource/plus-jakarta-sans/600.css";
import "material-symbols/outlined.css";

import { isTauri, surRaccourciChange } from "@/lib/captures-api";
import { cn } from "@/lib/utils";
import { CapturesView, CLE_RACCOURCI } from "@/modules/captures/CapturesView";
import { ReglagesView } from "@/modules/ReglagesView";
import "@/index.css";

type Page = "galerie" | "reglages";

const lirePage = (): Page => (window.location.hash === "#reglages" ? "reglages" : "galerie");

function EquiShot() {
  const queryClient = useQueryClient();
  const [page, setPage] = useState<Page>(lirePage);

  // Rust change le hash quand la fenêtre déjà ouverte est redemandée.
  useEffect(() => {
    const suivre = () => setPage(lirePage());
    window.addEventListener("hashchange", suivre);
    return () => window.removeEventListener("hashchange", suivre);
  }, []);

  // Un raccourci réglé ailleurs (ou rendu indisponible) se voit ici aussi.
  useEffect(() => {
    if (!isTauri()) return;
    const ecoute = surRaccourciChange((etat) => queryClient.setQueryData(CLE_RACCOURCI, etat));
    return () => void ecoute.then((arreter) => arreter());
  }, [queryClient]);

  const aller = (cible: Page) => {
    window.location.hash = cible;
  };

  return (
    <div className="flex h-screen flex-col bg-background text-on-surface">
      <nav className="flex items-center gap-1 border-b border-outline px-4 py-2" aria-label="EquiShot">
        <span className="mr-4 font-display text-[14px] font-semibold">EquiShot</span>
        {(["galerie", "reglages"] as const).map((cible) => (
          <button
            key={cible}
            type="button"
            data-testid={`onglet-${cible}`}
            aria-current={page === cible ? "page" : undefined}
            onClick={() => aller(cible)}
            className={cn(
              "eq-hover rounded-md px-2.5 py-1 text-[12px]",
              page === cible ? "bg-surface-high text-on-surface" : "text-muted",
            )}
          >
            {cible === "galerie" ? "Galerie" : "Réglages"}
          </button>
        ))}
      </nav>
      {page === "galerie" ? <CapturesView onReglages={() => aller("reglages")} /> : <ReglagesView />}
    </div>
  );
}

const queryClient = new QueryClient({ defaultOptions: { queries: { retry: false, refetchOnWindowFocus: true } } });

ReactDOM.createRoot(document.getElementById("root") as HTMLElement).render(
  <QueryClientProvider client={queryClient}>
    <EquiShot />
  </QueryClientProvider>,
);
