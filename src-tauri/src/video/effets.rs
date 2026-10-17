//! Les effets de presentation (lot 1, 0.22) : chaque image filmee est recomposee
//! sur la carte par Direct2D avant l'encodage, avec le zoom, les ondes des clics,
//! la trainee et le projecteur dessines dedans. Rien n'est pose a l'ecran : la
//! video seule les porte, prete a l'arret.
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
    D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LINE_JOIN_ROUND, D2D1_RADIAL_GRADIENT_BRUSH_PROPERTIES, D2D1_STROKE_STYLE_PROPERTIES,
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

/// Ce qu'il faut dessiner dans une image, en pixels de la zone filmee.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    /// Le point de la zone montre au centre de l'image, et le grossissement (1 : la zone entiere).
    pub zoom: (f32, f32, f32),
    pub ondes: Vec<Onde>,
    pub segments: Vec<Segment>,
    pub projecteur: Option<Projecteur>,
    pub curseur: Option<CurseurVu>,
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
            Ok(Compositeur { ctx, brute, source, cibles, pinceau, rond, curseurs: HashMap::new(), l, h })
        }
    }

    /// La texture ou le moteur copie la zone filmee.
    pub fn brute(&self) -> &ID3D11Texture2D {
        &self.brute
    }

    /// Compose `brute` et `scene` dans la texture `i` de l'anneau.
    pub fn dessiner(&mut self, i: usize, scene: &Scene) -> Result<(), Erreur> {
        let curseur = scene.curseur.and_then(|c| self.curseur(c.poignee).map(|b| (c, b)));
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
            for o in &scene.ondes {
                self.pinceau.SetColor(&couleur(o.couleur, o.opacite));
                ctx.DrawEllipse(&D2D1_ELLIPSE { point: v((o.x, o.y)), radiusX: o.rayon, radiusY: o.rayon }, &self.pinceau, o.epaisseur, None);
            }
            if let Some((c, (bitmap, (bl, bh), (hx, hy)))) = &curseur {
                let (x, y) = (c.x - hx * c.taille, c.y - hy * c.taille);
                let place = D2D_RECT_F { left: x, top: y, right: x + bl * c.taille, bottom: y + bh * c.taille };
                ctx.DrawBitmap(bitmap, Some(&place), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            }
            ctx.EndDraw(None, None)?;
            ctx.SetTarget(None);
        }
        Ok(())
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
    fn le_zoom_ne_sort_jamais_de_la_zone() {
        // Un point pres du coin : le coin de la zone reste au coin de l'image.
        let m = transformation((5.0, 5.0, 2.0), 640.0, 360.0);
        assert_eq!(appliquer(&m, (0.0, 0.0)), (0.0, 0.0));
        let m = transformation((639.0, 359.0, 3.0), 640.0, 360.0);
        assert_eq!(appliquer(&m, (640.0, 360.0)), (640.0, 360.0));
    }
}
