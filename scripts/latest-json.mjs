// Le `latest.json` que lit la mise a jour d'EquiShot (ADR-020), ecrit par
// `publier.ps1` dans `publication/` a cote de l'installeur et de sa signature.
//
//   node scripts/latest-json.mjs <version> <installeur.exe> <installeur.exe.sig> <sortie.json>
//
// Format du greffon `tauri-plugin-updater` 2.12 (fichier statique) : version,
// notes, pub_date RFC 3339, et par plateforme la signature (contenu du .sig) et l'URL.

import { readFileSync, writeFileSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

export const BASE = "https://www.equinox-ai.fr/equishot/";

/** La section `## <version>, ...` du CHANGELOG, sans son titre, jusqu'a la suivante. */
export function notesDe(changelog, version) {
  const lignes = changelog.replace(/\r\n/g, "\n").split("\n");
  const debut = lignes.findIndex((l) => l.startsWith(`## ${version},`) || l.startsWith(`## ${version} `) || l === `## ${version}`);
  if (debut === -1) throw new Error(`Aucune section « ## ${version} » dans le CHANGELOG : rien n'est publié sans ses notes.`);
  const fin = lignes.findIndex((l, i) => i > debut && l.startsWith("## "));
  return lignes.slice(debut + 1, fin === -1 ? undefined : fin).join("\n").trim();
}

export function latestJson({ version, notes, signature, installeur, date = new Date() }) {
  if (!/^\d+\.\d+\.\d+$/.test(version)) throw new Error(`Version illisible : ${version}`);
  if (!signature.trim()) throw new Error("Signature vide : l'installeur n'a pas été signé.");
  return {
    version,
    notes,
    pub_date: date.toISOString(),
    platforms: {
      "windows-x86_64": { signature: signature.trim(), url: BASE + encodeURIComponent(basename(installeur)) },
    },
  };
}

if (process.argv[1] && fileURLToPath(import.meta.url) === process.argv[1]) {
  const [version, installeur, sig, sortie] = process.argv.slice(2);
  if (!sortie) {
    console.error("usage : node scripts/latest-json.mjs <version> <installeur.exe> <installeur.exe.sig> <sortie.json>");
    process.exit(2);
  }
  const racine = join(dirname(fileURLToPath(import.meta.url)), "..");
  const notes = notesDe(readFileSync(join(racine, "docs", "CHANGELOG.md"), "utf8"), version);
  const json = latestJson({ version, notes, signature: readFileSync(sig, "utf8"), installeur });
  writeFileSync(sortie, JSON.stringify(json, null, 2) + "\n");
  console.log(`latest.json : ${version}, ${json.platforms["windows-x86_64"].url}`);
}
