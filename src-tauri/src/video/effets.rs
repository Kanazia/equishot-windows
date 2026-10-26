//! Les effets de presentation (lot 1, 0.22) : chaque image filmee est recomposee
//! sur la carte par Direct2D avant l'encodage, avec le zoom, les ondes des clics,
//! la trainee et le projecteur dessines dedans. Rien n'est pose a l'ecran : la
//! video seule les porte, prete a l'arret. Le lot 2 (0.23, ADR-023) y ajoute les
//! touches frappees, en bas de l'image, et les traits dessines pendant la prise.
//!
//! Le moteur copie la zone filmee dans `brute` ; a chaque image envoyee (30 i/s,
//! meme ecran immobile), `dessiner` compose `brute` et la scene du moment dans une
//! texture de l'anneau.

use windows62::core::Interface;
use std::collections::HashMap;

use windows62::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_IGNORE, D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_GRADIENT_STOP, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows62::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1DeviceContext, ID2D1Factory, ID2D1Factory1, ID2D1SolidColorBrush, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
    D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_OPTIONS_TARGET, D2D1_BITMAP_PROPERTIES1, D2D1_BUFFER_PRECISION_8BPC_UNORM,
    D2D1_CAP_STYLE_ROUND, D2D1_COLOR_SPACE_SRGB, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP, D2D1_FACTORY_TYPE_MULTI_THREADED,
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LINE_JOIN_ROUND, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_STROKE_STYLE_PROPERTIES, D2D1_DRAW_TEXT_OPTIONS_NONE,
    D2D1_ROUNDED_RECT,
};
use windows62::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory, IDWriteTextFormat, DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_STRETCH_NORMAL, DWRITE_FONT_STYLE_NORMAL,
    DWRITE_FONT_WEIGHT_SEMI_BOLD, DWRITE_TEXT_METRICS,
};
use windows62::Win32::Graphics::Direct3D11::{ID3D11Device, ID3D11Multithread, ID3D11Texture2D};
use windows62::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows62::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGISurface};
use windows_numerics::{Matrix3x2, Vector2};

type Erreur = Box<dyn std::error::Error + Send + Sync>;

/// Une couleur sans transparence, de 0 a 1.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Couleur(pub f32, pub f32, pub f32);

/// Un anneau qui s'ouvre au clic.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Onde {
    pub x: f32,
    pub y: f32,
    pub rayon: f32,
    pub epaisseur: f32,
    pub opacite: f32,
    pub couleur: Couleur,
}

/// Un segment de la trainee.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Segment {
    pub de: (f32, f32),
    pub a: (f32, f32),
    pub epaisseur: f32,
    pub opacite: f32,
    pub couleur: Couleur,
}

/// Le disque eclaire, l'ecran assombri autour.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Projecteur {
    pub x: f32,
    pub y: f32,
    pub rayon: f32,
    pub opacite: f32,
}

/// Le curseur courant, dessine par le moteur quand il est agrandi.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CurseurVu {
    /// La poignee Windows du curseur (`HCURSOR`) : la cle du cache de ses pixels.
    pub poignee: isize,
    pub x: f32,
    pub y: f32,
    pub taille: f32,
}

/// L'outil d'un trait dessine pendant la prise (0.23).
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Outil {
    Stylo,
    Fleche,
    Rectangle,
}

/// Un trait dessine pendant la prise, envoye par la page du dessin : ses points en
/// pixels de la zone filmee (le stylo en a autant qu'il veut, la fleche et le
/// rectangle deux : depart et arrivee).
#[derive(Debug, Clone, PartialEq, serde::Deserialize)]
pub struct Trait {
    pub outil: Outil,
    pub couleur: (f32, f32, f32),
    pub epaisseur: f32,
    pub points: Vec<(f32, f32)>,
}

/// Les touches frappees, en bas de l'image (0.23).
#[derive(Debug, Clone, PartialEq)]
pub struct Touches {
    pub texte: String,
    pub opacite: f32,
    /// La taille du texte, en pixels de l'image.
    pub taille: f32,
}

/// Ce qu'il faut dessiner dans une image, en pixels de la zone filmee.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Le point de la zone montre au centre de l'image, et le grossissement (1 : la zone entiere).
    pub zoom: (f32, f32, f32),
    pub ondes: Vec<Onde>,
    pub segments: Vec<Segment>,
    pub projecteur: Option<Projecteur>,
    pub curseur: Option<CurseurVu>,
    pub traits: Vec<Trait>,
    /// Hors du zoom : toujours en bas de l'image.
    pub touches: Option<Touches>,
}

/// Les deux branches de la tete d'une fleche qui va de `de` a `a`.
pub fn tete_de_fleche(de: (f32, f32), a: (f32, f32), longueur: f32) -> [(f32, f32); 2] {
    let angle = (a.1 - de.1).atan2(a.0 - de.0);
    let ouverture = 28f32.to_radians();
    [angle + std::f32::consts::PI - ouverture, angle + std::f32::consts::PI + ouverture].map(|t| (a.0 + longueur * t.cos(), a.1 + longueur * t.sin()))
}

/// La transformation de la zone vers l'image pour un zoom `(cx, cy, f)` : le centre
/// est borne pour que l'image ne montre jamais rien hors de la zone.
pub fn transformation(zoom: (f32, f32, f32), l: f32, h: f32) -> Matrix3x2 {
    let (cx, cy, f) = zoom;
    let f = f.max(1.0);
    let borner = |c: f32, cote: f32| c.clamp(cote / (2.0 * f), cote - cote / (2.0 * f));
    let (cx, cy) = (borner(cx, l), borner(cy, h));
    Matrix3x2::translation(-cx, -cy) * Matrix3x2::scale(f, f) * Matrix3x2::translation(l / 2.0, h / 2.0)
}

fn couleur(c: Couleur, a: f32) -> D2D1_COLOR_F {
    D2D1_COLOR_F { r: c.0, g: c.1, b: c.2, a }
}

fn v(p: (f32, f32)) -> Vector2 {
    Vector2 { X: p.0, Y: p.1 }
}

/// Direct2D sur la carte de la capture. A tenir sous un verrou : un seul fil a la fois.
pub struct Compositeur {
    ctx: ID2D1DeviceContext,
    brute: ID3D11Texture2D,
    source: ID2D1Bitmap1,
    cibles: Vec<ID2D1Bitmap1>,
    pinceau: ID2D1SolidColorBrush,
    rond: ID2D1StrokeStyle,
    /// Les curseurs deja lus : bitmap, taille et point chaud ; `None` si illisible.
    curseurs: HashMap<isize, Option<(ID2D1Bitmap1, (f32, f32), (f32, f32))>>,
    ecriture: IDWriteFactory,
    /// Le format du texte des touches et sa taille ; refait si la taille change.
    format: Option<(f32, IDWriteTextFormat)>,
    l: u32,
    h: u32,
}

impl Compositeur {
    /// `brute` recoit la zone copiee par le moteur ; `anneau`, les images composees.
    pub fn new(device: &ID3D11Device, brute: ID3D11Texture2D, anneau: &[ID3D11Texture2D], l: u32, h: u32) -> Result<Self, Erreur> {
        unsafe {
            // Media Foundation lit l'anneau sur son propre fil : la carte se protege elle-meme.
            let avant = device.cast::<ID3D11Multithread>()?.SetMultithreadProtected(true);
            if cfg!(test) {
                eprintln!("protection multifil deja posee : {}", avant.as_bool());
            }
            let fabrique: ID2D1Factory1 = D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None)?;
            let ctx = fabrique.CreateDevice(&device.cast::<IDXGIDevice>()?)?.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let proprietes = |options| D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_IGNORE },
                dpiX: 96.0,
                dpiY: 96.0,
                bitmapOptions: options,
                ..Default::default()
            };
            let source = ctx.CreateBitmapFromDxgiSurface(&brute.cast::<IDXGISurface>()?, Some(&proprietes(D2D1_BITMAP_OPTIONS_NONE)))?;
            let mut cibles = Vec::with_capacity(anneau.len());
            for t in anneau {
                cibles.push(ctx.CreateBitmapFromDxgiSurface(&t.cast::<IDXGISurface>()?, Some(&proprietes(D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW)))?);
            }
            let pinceau = ctx.CreateSolidColorBrush(&couleur(Couleur(1.0, 1.0, 1.0), 1.0), None)?;
            let rond = ID2D1Factory::CreateStrokeStyle(&fabrique,
                &D2D1_STROKE_STYLE_PROPERTIES { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, miterLimit: 10.0, ..Default::default() },
                None,
            )?;
            ctx.SetAntialiasMode(D2D1_ANTIALIAS_MODE_PER_PRIMITIVE);
            let ecriture: IDWriteFactory = DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)?;
            Ok(Compositeur { ctx, brute, source, cibles, pinceau, rond, curseurs: HashMap::new(), ecriture, format: None, l, h })
        }
    }

    /// La texture ou le moteur copie la zone filmee.
    pub fn brute(&self) -> &ID3D11Texture2D {
        &self.brute
    }

    /// Compose `brute` et `scene` dans la texture `i` de l'anneau.
    pub fn dessiner(&mut self, i: usize, scene: &Scene) -> Result<(), Erreur> {
        let curseur = scene.curseur.and_then(|c| self.curseur(c.poignee).map(|b| (c, b)));
        let format = match &scene.touches {
            Some(t) if t.opacite > 0.0 && !t.texte.is_empty() => self.format(t.taille),
            _ => None,
        };
        let (l, h) = (self.l as f32, self.h as f32);
        let ctx = &self.ctx;
        unsafe {
            ctx.SetTarget(&self.cibles[i % self.cibles.len()]);
            ctx.BeginDraw();
            ctx.SetTransform(&transformation(scene.zoom, l, h));
            ctx.DrawBitmap(&self.source, None, 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            if let Some(p) = scene.projecteur.filter(|p| p.opacite > 0.0) {
                let noir = D2D1_COLOR_F { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };
                let arrets = [
                    D2D1_GRADIENT_STOP { position: 0.0, color: noir },
                    D2D1_GRADIENT_STOP { position: 0.92, color: noir },
                    D2D1_GRADIENT_STOP { position: 1.0, color: D2D1_COLOR_F { a: p.opacite, ..noir } },
                ];
                let collection =
                    ctx.CreateGradientStopCollection(&arrets, D2D1_COLOR_SPACE_SRGB, D2D1_COLOR_SPACE_SRGB, D2D1_BUFFER_PRECISION_8BPC_UNORM, D2D1_EXTEND_MODE_CLAMP, Default::default())?;
                let degrade = ctx.CreateRadialGradientBrush(
                    &D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES { center: v((p.x, p.y)), gradientOriginOffset: v((0.0, 0.0)), radiusX: p.rayon, radiusY: p.rayon },
                    None,
                    &collection,
                )?;
                ctx.FillRectangle(&D2D_RECT_F { left: 0.0, top: 0.0, right: l, bottom: h }, &degrade);
            }
            for s in &scene.segments {
                self.pinceau.SetColor(&couleur(s.couleur, s.opacite));
                ctx.DrawLine(v(s.de), v(s.a), &self.pinceau, s.epaisseur, &self.rond);
            }
            for t in &scene.traits {
                let (r, g, b) = t.couleur;
                self.pinceau.SetColor(&couleur(Couleur(r, g, b), 1.0));
                match (t.outil, t.points.as_slice()) {
                    (_, []) => {}
                    (Outil::Stylo, [p]) => ctx.DrawLine(v(*p), v(*p), &self.pinceau, t.epaisseur, &self.rond),
                    (Outil::Stylo, points) => {
                        for paire in points.windows(2) {
                            ctx.DrawLine(v(paire[0]), v(paire[1]), &self.pinceau, t.epaisseur, &self.rond);
                        }
                    }
                    (_, [_]) => {}
                    (Outil::Rectangle, [de, .., a]) => {
                        let rect = D2D_RECT_F { left: de.0.min(a.0), top: de.1.min(a.1), right: de.0.max(a.0), bottom: de.1.max(a.1) };
                        ctx.DrawRectangle(&rect, &self.pinceau, t.epaisseur, &self.rond);
                    }
                    (Outil::Fleche, [de, .., a]) => {
                        ctx.DrawLine(v(*de), v(*a), &self.pinceau, t.epaisseur, &self.rond);
                        for bout in tete_de_fleche(*de, *a, t.epaisseur * 4.0) {
                            ctx.DrawLine(v(*a), v(bout), &self.pinceau, t.epaisseur, &self.rond);
                        }
                    }
                }
            }
            for o in &scene.ondes {
                self.pinceau.SetColor(&couleur(o.couleur, o.opacite));
                ctx.DrawEllipse(&D2D1_ELLIPSE { point: v((o.x, o.y)), radiusX: o.rayon, radiusY: o.rayon }, &self.pinceau, o.epaisseur, None);
            }
            if let Some((c, (bitmap, (bl, bh), (hx, hy)))) = &curseur {
                let (x, y) = (c.x - hx * c.taille, c.y - hy * c.taille);
                let place = D2D_RECT_F { left: x, top: y, right: x + bl * c.taille, bottom: y + bh * c.taille };
                ctx.DrawBitmap(bitmap, Some(&place), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            }
            if let (Some(t), Some(format)) = (&scene.touches, &format) {
                // Hors du zoom : la pastille reste en bas, au milieu de l'image.
                ctx.SetTransform(&Matrix3x2::identity());
                let texte: Vec<u16> = t.texte.encode_utf16().collect();
                let mise = self.ecriture.CreateTextLayout(&texte, format, l * 0.9, t.taille * 2.0)?;
                let mut mesure = DWRITE_TEXT_METRICS::default();
                mise.GetMetrics(&mut mesure)?;
                let marge = t.taille * 0.6;
                let (largeur, hauteur) = (mesure.width + 2.0 * marge, mesure.height + marge);
                let (x, y) = ((l - largeur) / 2.0, h - hauteur - t.taille * 1.5);
                self.pinceau.SetColor(&D2D1_COLOR_F { r: 0.08, g: 0.08, b: 0.1, a: 0.78 * t.opacite });
                let fond = D2D1_ROUNDED_RECT { rect: D2D_RECT_F { left: x, top: y, right: x + largeur, bottom: y + hauteur }, radiusX: hauteur / 2.0, radiusY: hauteur / 2.0 };
                ctx.FillRoundedRectangle(&fond, &self.pinceau);
                self.pinceau.SetColor(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: t.opacite });
                ctx.DrawTextLayout(v((x + marge - mesure.left, y + marge / 2.0 - mesure.top)), &mise, &self.pinceau, D2D1_DRAW_TEXT_OPTIONS_NONE);
            }
            ctx.EndDraw(None, None)?;
            ctx.SetTarget(None);
        }
        Ok(())
    }

    /// Le format du texte des touches pour `taille` ; `None` si DirectWrite le refuse.
    fn format(&mut self, taille: f32) -> Option<IDWriteTextFormat> {
        if let Some((t, f)) = &self.format {
            if *t == taille {
                return Some(f.clone());
            }
        }
        // SAFETY: chaines constantes terminees par zero, collection systeme (None).
        let f = unsafe {
            self.ecriture.CreateTextFormat(
                windows62::core::w!("Segoe UI"),
                None,
                DWRITE_FONT_WEIGHT_SEMI_BOLD,
                DWRITE_FONT_STYLE_NORMAL,
                DWRITE_FONT_STRETCH_NORMAL,
                taille,
                windows62::core::w!("fr-fr"),
            )
        }
        .ok()?;
        self.format = Some((taille, f.clone()));
        Some(f)
    }

    /// Le bitmap d'un curseur, lu une fois par forme.
    fn curseur(&mut self, poignee: isize) -> Option<(ID2D1Bitmap1, (f32, f32), (f32, f32))> {
        if let Some(lu) = self.curseurs.get(&poignee) {
            return lu.clone();
        }
        let lu = super::direct::pixels_du_curseur(poignee).and_then(|(l, h, chaud, pixels)| {
            let proprietes = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0,
                dpiY: 96.0,
                ..Default::default()
            };
            // SAFETY: pixels fait l x h x 4 octets, pas de l x 4.
            let bitmap = unsafe { self.ctx.CreateBitmap(D2D_SIZE_U { width: l, height: h }, Some(pixels.as_ptr().cast()), l * 4, &proprietes) }.ok()?;
            Some((bitmap, (l as f32, h as f32), (chaud.0 as f32, chaud.1 as f32)))
        });
        // ponytail: cache sans plafond, quelques dizaines de formes au plus dans une prise.
        self.curseurs.insert(poignee, lu.clone());
        lu
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn appliquer(m: &Matrix3x2, p: (f32, f32)) -> (f32, f32) {
        (p.0 * m.M11 + p.1 * m.M21 + m.M31, p.0 * m.M12 + p.1 * m.M22 + m.M32)
    }

    #[test]
    fn sans_zoom_la_zone_reste_en_place() {
        let m = transformation((10.0, 10.0, 1.0), 640.0, 360.0);
        assert_eq!(appliquer(&m, (0.0, 0.0)), (0.0, 0.0));
        assert_eq!(appliquer(&m, (640.0, 360.0)), (640.0, 360.0));
    }

    #[test]
    fn le_zoom_met_son_point_au_centre() {
        let m = transformation((200.0, 150.0, 2.0), 640.0, 360.0);
        assert_eq!(appliquer(&m, (200.0, 150.0)), (320.0, 180.0));
        assert_eq!(appliquer(&m, (210.0, 150.0)), (340.0, 180.0));
    }

    #[test]
    fn la_tete_de_fleche_revient_vers_le_depart() {
        // Une fleche vers la droite : ses deux branches partent en arriere, de part et d'autre.
        let [a, b] = tete_de_fleche((0.0, 0.0), (100.0, 0.0), 10.0);
        assert!(a.0 < 100.0 && b.0 < 100.0, "{a:?} {b:?}");
        assert!((a.1 + b.1).abs() < 1e-4 && a.1.abs() > 3.0, "{a:?} {b:?}");
    }

    #[test]
    fn un_trait_se_lit_de_la_page() {
        let t: Trait = serde_json::from_str(r#"{"outil":"fleche","couleur":[1,0,0],"epaisseur":4,"points":[[1,2],[30,40]]}"#).expect("trait lu");
        assert_eq!(t.outil, Outil::Fleche);
        assert_eq!(t.points, vec![(1.0, 2.0), (30.0, 40.0)]);
    }

    #[test]
    fn le_zoom_ne_sort_jamais_de_la_zone() {
        // Un point pres du coin : le coin de la zone reste au coin de l'image.
        let m = transformation((5.0, 5.0, 2.0), 640.0, 360.0);
        assert_eq!(appliquer(&m, (0.0, 0.0)), (0.0, 0.0));
        let m = transformation((639.0, 359.0, 3.0), 640.0, 360.0);
        assert_eq!(appliquer(&m, (640.0, 360.0)), (640.0, 360.0));
    }
}
