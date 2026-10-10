# Fabrique les images temoins des tests OCR (ADR-010), dans src-tauri/tests.
# A relancer seulement pour les refaire : les PNG sont versionnes.
Add-Type -AssemblyName System.Drawing
$dossier = Join-Path $PSScriptRoot "..\src-tauri\tests"
New-Item -ItemType Directory -Force $dossier | Out-Null

function Dessiner($nom, $texte, $taillePolice, $largeur, $hauteur) {
    $image = New-Object System.Drawing.Bitmap $largeur, $hauteur
    $g = [System.Drawing.Graphics]::FromImage($image)
    $g.Clear([System.Drawing.Color]::White)
    $g.TextRenderingHint = [System.Drawing.Text.TextRenderingHint]::ClearTypeGridFit
    $police = New-Object System.Drawing.Font "Segoe UI", $taillePolice, ([System.Drawing.FontStyle]::Regular), ([System.Drawing.GraphicsUnit]::Pixel)
    $g.DrawString($texte, $police, [System.Drawing.Brushes]::Black, 4, 2)
    $g.Dispose()
    $image.Save((Join-Path $dossier $nom), [System.Drawing.Imaging.ImageFormat]::Png)
    $image.Dispose()
}

Dessiner "ocr-temoin.png" "EquiShot facture 2026" 20 400 60
Dessiner "ocr-petit.png" "Bonjour" 12 120 24
# Petit texte dans une grande zone : lu vide a 10 px sans agrandissement.
Dessiner "ocr-large-petit.png" "Bonjour" 10 400 120
