import { test } from "node:test";
import assert from "node:assert/strict";

import { latestJson, notesDe } from "../../scripts/latest-json.mjs";

const CHANGELOG = `# Journal

## 0.21.0, 2026-10-09 (ADR-020)

EquiShot s'installe.

- Mise à jour.

## 0.20.3, 2026-10-09

- Réglages dans le menu.
`;

test("les notes sont la section de la version, sans son titre ni la suivante", () => {
  assert.equal(notesDe(CHANGELOG, "0.21.0"), "EquiShot s'installe.\n\n- Mise à jour.");
  assert.equal(notesDe(CHANGELOG, "0.20.3"), "- Réglages dans le menu.");
  assert.equal(notesDe(CHANGELOG.replace(/\n/g, "\r\n"), "0.20.3"), "- Réglages dans le menu.", "fins de ligne Windows");
  assert.throws(() => notesDe(CHANGELOG, "0.21.1"), /Aucune section/);
  assert.throws(() => notesDe(CHANGELOG, "0.21"), /Aucune section/, "0.21 n'attrape pas 0.21.0");
});

test("latest.json a la forme du greffon, l'URL du site et la signature telle quelle", () => {
  const json = latestJson({
    version: "0.21.0",
    notes: "n",
    signature: "c2lnbmF0dXJl\n",
    installeur: "C:\\x\\publication\\EquiShot_0.21.0_x64-setup.exe",
    date: new Date("2026-10-09T10:00:00Z"),
  });
  assert.deepEqual(json, {
    version: "0.21.0",
    notes: "n",
    pub_date: "2026-10-09T10:00:00.000Z",
    platforms: {
      "windows-x86_64": { signature: "c2lnbmF0dXJl", url: "https://www.equinox-ai.fr/equishot/EquiShot_0.21.0_x64-setup.exe" },
    },
  });
  assert.throws(() => latestJson({ version: "0.21.0", notes: "", signature: " ", installeur: "a.exe" }), /Signature vide/);
  assert.throws(() => latestJson({ version: "v0.21", notes: "", signature: "s", installeur: "a.exe" }), /Version illisible/);
});
