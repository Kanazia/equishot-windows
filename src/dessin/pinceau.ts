/**
 * Le pinceau d'encre de Chine (0.24, ADR-024) : l'épaisseur à chaque point d'un tracé
 * `[x, y, secondes]`. Lent, le trait s'épaissit jusqu'à 1,9 fois `base` ; rapide, il
 * s'affine jusqu'à 0,3 fois ; l'épaisseur suit avec un peu de retard, comme l'encre.
 * Il s'effile sur 2 `base` au départ (le pinceau posé) et sur 8 à l'arrivée (le pinceau levé).
 * Même calcul que `pinceau` de `src-tauri/src/video/effets.rs` : les deux se changent ensemble.
 */
export function pinceau(points: [number, number, number][], base: number): number[] {
  const n = points.length;
  if (n < 2) return points.map(() => base);
  const pas = points.slice(1).map((p, i) => Math.hypot(p[0] - points[i][0], p[1] - points[i][1]));
  const cible = (i: number) => {
    const j = Math.max(i, 1);
    const vitesse = pas[j - 1] / Math.max(points[j][2] - points[j - 1][2], 0.004);
    return base * (0.3 + 1.6 * Math.exp(-vitesse / (150 * base)));
  };
  const largeurs: number[] = [];
  let w = cible(0);
  for (let i = 0; i < n; i++) {
    w += 0.35 * (cible(i) - w);
    largeurs.push(w);
  }
  const total = pas.reduce((a, b) => a + b, 0);
  let parcouru = 0;
  return largeurs.map((w, i) => {
    if (i > 0) parcouru += pas[i - 1];
    const f = Math.min(Math.max(Math.min(parcouru / (2 * base), (total - parcouru) / (8 * base)), 0), 1);
    return w * (0.25 + 0.75 * Math.sqrt(f));
  });
}
