import { Icon } from "@/components/Icon";

interface ViewShellProps {
  /** Nom de la vue, lu par les lecteurs d'écran : il n'est plus affiché. */
  title: string;
  actions?: React.ReactNode;
  children: React.ReactNode;
}

/**
 * Cadre commun des vues : une barre fine pour les actions, et le contenu qui
 * défile. Pas de titre affiché, le module est déjà nommé dans la barre du haut
 * (la place va au contenu).
 */
export function ViewShell({ title, actions, children }: ViewShellProps) {
  return (
    <section aria-label={title} className="flex min-w-0 flex-1 flex-col overflow-hidden">
      {actions && (
        <div className="flex items-center justify-end gap-2 border-b border-outline px-6 py-2">
          {actions}
        </div>
      )}

      <div className="eq-scrollbar min-h-0 flex-1 overflow-y-auto px-6 py-5">
        {children}
      </div>
    </section>
  );
}


interface EmptyStateProps {
  icon: string;
  title: string;
  hint: string;
}

export function EmptyState({ icon, title, hint }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center justify-center rounded-lg border border-dashed border-outline bg-surface-low/60 px-6 py-16 text-center">
      <Icon name={icon} size={28} className="text-muted" />
      <p className="mt-3 font-display text-[14px] text-on-surface-variant">{title}</p>
      <p className="mt-1 max-w-md text-[12px] text-muted">{hint}</p>
    </div>
  );
}
