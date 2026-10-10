import { cn } from "@/lib/utils";

interface IconProps {
  /** Nom Material Symbols, variante outlined. */
  name: string;
  className?: string;
  /** Taille en pixels. */
  size?: number;
  filled?: boolean;
}

/**
 * Icone Material Symbols servie par le paquet npm `material-symbols`
 * (police variable, aucun appel reseau).
 */
export function Icon({ name, className, size = 18, filled = false }: IconProps) {
  return (
    <span
      aria-hidden="true"
      // overflow-hidden : tant que la police charge, le nom s'écrit en toutes lettres et
      // déborderait sur le bouton voisin, qui perdrait ses clics (0.20, « Épingler »).
      className={cn("material-symbols-outlined select-none overflow-hidden leading-none", className)}
      style={{
        fontSize: `${size}px`,
        width: `${size}px`,
        height: `${size}px`,
        fontVariationSettings: `'FILL' ${filled ? 1 : 0}, 'wght' 300, 'GRAD' 0, 'opsz' ${size}`,
      }}
    >
      {name}
    </span>
  );
}
