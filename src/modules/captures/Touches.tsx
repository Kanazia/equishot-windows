import { touchesLisibles } from "@/lib/captures-api";

/** Une combinaison affichée en touches : `Ctrl` `Alt` `S`. */
export function Touches({ combinaison }: { combinaison: string }) {
  return (
    <span className="inline-flex items-center gap-1">
      {touchesLisibles(combinaison).map((touche) => (
        <kbd
          key={touche}
          className="rounded border border-outline bg-surface-low px-1.5 py-0.5 font-mono text-[11px] leading-none text-on-surface-variant"
        >
          {touche}
        </kbd>
      ))}
    </span>
  );
}
