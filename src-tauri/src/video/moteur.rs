//! Le moteur de la video (ADR-016), repris de la preuve technique
//! (`plans/preuve-video/main.rs`) : Windows.Graphics.Capture pour l'image,
//! copiee sur la carte dans un anneau de textures a la taille de la zone ;
//! un fil qui envoie la derniere image toutes les 1/30 s de temps actif, avec
//! son propre horodatage ; WASAPI pour le son, place sur la frise par
//! l'horodatage QPC de chaque paquet ; Media Foundation pour l'encodage.
//!
//! Rien ici ne tourne sur le thread principal : `demarrer` lance ses fils et
//! rend la main, `arreter` attend leur fin (a appeler hors du thread principal).
//! L'exe de release est en `panic = "abort"` : aucun `unwrap` ici.

use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, OnceLock, PoisonError};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant};

use image::RgbaImage;
use wasapi::{DeviceEnumerator, DeviceState, Direction, SampleType, StreamMode, WaveFormat};
use windows62::Graphics::DirectX::Direct3D11::IDirect3DSurface;
use windows62::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE,
    D3D11_MAP_READ, D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows_capture::capture::{CaptureControl, Context, GraphicsCaptureApiHandler};
use windows_capture::d3d11::SendDirectX;
use windows_capture::encoder::{AudioSettingsBuilder, ContainerSettingsBuilder, VideoEncoder, VideoSettingsBuilder, VideoSettingsSubType};
use windows_capture::frame::Frame;
use windows_capture::graphics_capture_api::InternalCaptureControl;
use windows_capture::monitor::Monitor;
use windows_capture::settings::{
    ColorFormat, CursorCaptureSettings, DirtyRegionSettings, DrawBorderSettings, MinimumUpdateIntervalSettings, SecondaryWindowSettings,
    Settings,
};

use super::bulle::Taille;
use super::effets::{Compositeur, Scene};
use super::frise::{crete, debit, en_pcm16, melanger, Frise, Pauses, IPS, TAUX};

type Erreur = Box<dyn std::error::Error + Send + Sync>;

/// Marge laissee aux sources son avant de figer la frise.
const LATENCE_S: f64 = 0.05;

pub(super) fn tenir<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(PoisonError::into_inner)
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize, PartialEq)]
#[serde(rename_all = "camelCase", default)]
pub struct Reglages {
    pub micro: bool,
    pub son_systeme: bool,
    pub curseur: bool,
    /// Le micro choisi (identifiant WASAPI) ; `None` : celui par defaut de Windows.
    pub micro_id: Option<String>,
    /// La sortie dont on prend la boucle ; `None` : celle par defaut.
    pub sortie_id: Option<String>,
    /// Volumes appliques au melange, de 0 a 2 (200 %). Windows n'est jamais touche.
    pub volume_micro: f32,
    pub volume_son: f32,
    /// La bulle de camera (0.18) : allumee, quelle camera (`deviceId` du navigateur ;
    /// `None` : celle par defaut), sa taille, son miroir, sa place en proportion de
    /// l'espace libre de la zone (`None` : en bas a gauche).
    pub camera: bool,
    pub camera_id: Option<String>,
    pub camera_taille: Taille,
    pub camera_miroir: bool,
    pub camera_place: Option<(f64, f64)>,
    /// Les effets (0.22, ADR-022) : l'onde au clic, la trainee bouton gauche tenu, et
    /// le grossissement du curseur (1 : Windows le dessine ; jusqu'a 3 : le moteur).
    pub onde: bool,
    pub trainee: bool,
    pub curseur_taille: f32,
    /// Le lot 2 et le lot 3 (0.23, ADR-023) : les touches montrees, le zoom qui vient
    /// seul au clic, le curseur dessine par le moteur avec douceur.
    pub touches: super::animation::ModeTouches,
    pub zoom_auto: bool,
    pub curseur_lisse: bool,
}

impl Default for Reglages {
    fn default() -> Self {
        Reglages {
            micro: true,
            son_systeme: false,
            curseur: true,
            micro_id: None,
            sortie_id: None,
            volume_micro: 1.0,
            volume_son: 1.0,
            camera: false,
            camera_id: None,
            camera_taille: Taille::Moyenne,
            camera_miroir: true,
            camera_place: None,
            onde: true,
            trainee: true,
            curseur_taille: 1.0,
            touches: super::animation::ModeTouches::Raccourcis,
            zoom_auto: false,
            curseur_lisse: false,
        }
    }
}

impl Reglages {
    /// Le grossissement du curseur que le moteur dessine ; `None` quand Windows le
    /// dessine (1x sans lissage) ou quand il est cache. Le curseur lisse est
    /// toujours dessine par le moteur, meme a 1x.
    pub fn curseur_agrandi(&self) -> Option<f32> {
        (self.curseur && (self.curseur_taille > 1.0 || self.curseur_lisse)).then(|| self.curseur_taille.clamp(1.0, 3.0))
    }
}

/// Le niveau crete de chaque source apres volume (0 a 1) : les jauges.
#[derive(Debug, Clone, Copy, Default, serde::Serialize)]
pub struct Niveaux {
    pub micro: f32,
    pub son: f32,
}

/// Ce qu'on filme : l'ecran dont le coin est `ecran` (pixels physiques du
/// bureau), la zone `(x, y, largeur, hauteur)` relative a cet ecran, deja paire.
pub struct Demande {
    pub ecran: (i32, i32),
    pub zone: (u32, u32, u32, u32),
    pub sortie: PathBuf,
    pub reglages: Reglages,
    /// Les effets (0.22) : la scene a dessiner pour l'image prise a cet instant.
    /// `None` : la zone filmee part telle quelle, sans passer par Direct2D.
    pub effets: Option<Effets>,
}

/// La scene du moment, demandee a chaque image envoyee.
pub type Effets = Arc<dyn Fn(Instant) -> Scene + Send + Sync>;

/// Direct2D et l'anneau qu'il remplit, sous un seul verrou.
struct Composition {
    compositeur: Compositeur,
    surfaces: Vec<IDirect3DSurface>,
    textures: Vec<ID3D11Texture2D>,
    device: ID3D11Device,
    contexte: ID3D11DeviceContext,
}

/// Ce qu'un enregistrement fini rend.
pub struct Bilan {
    pub duree_ms: i64,
    pub largeur: u32,
    pub hauteur: u32,
    pub premiere: Option<RgbaImage>,
}

struct Partage {
    zone: (u32, u32, u32, u32),
    t0: OnceLock<Instant>,
    /// Le meme instant que `t0`, en unites QPC de 100 ns (celles de WASAPI).
    t0_qpc: OnceLock<i64>,
    pauses: Mutex<Pauses>,
    /// Le temps actif ou s'arreter, pose par `arreter`.
    fin: Mutex<Option<f64>>,
    /// Abandon : les fils s'arretent sans rien finir.
    abandon: AtomicBool,
    encodeur: Mutex<Option<VideoEncoder>>,
    surface: Mutex<Option<SendDirectX<IDirect3DSurface>>>,
    premiere: Mutex<Option<RgbaImage>>,
    erreur: Mutex<Option<String>>,
    niveaux: Mutex<Niveaux>,
    /// Les sources perdues en route (« micro débranché »), a dire dans la barre.
    perdues: Mutex<Vec<String>>,
    effets: Option<Effets>,
    /// Direct2D ne s'est pas ouvert : la prise continue sans effets.
    sans_effets: AtomicBool,
    composition: Mutex<Option<SendDirectX<Composition>>>,
}

impl Partage {
    fn mur(&self) -> Option<f64> {
        self.t0.get().map(|t| t.elapsed().as_secs_f64())
    }

    fn noter_erreur(&self, motif: String) {
        tenir(&self.erreur).get_or_insert(motif);
        self.abandon.store(true, Ordering::Relaxed);
    }

    fn fini(&self) -> bool {
        self.abandon.load(Ordering::Relaxed)
    }
}

/// Un enregistrement en cours.
pub struct Enregistrement {
    p: Arc<Partage>,
    sortie: PathBuf,
    controle: Option<CaptureControl<Capteur, Erreur>>,
    fils: Vec<JoinHandle<()>>,
}

impl Enregistrement {
    /// Ouvre l'encodeur et le son, lance la capture. Rend aussi les sources son
    /// demandees qui n'ont pas pu s'ouvrir : l'enregistrement se fait sans elles.
    pub fn demarrer(d: Demande) -> Result<(Enregistrement, Vec<String>), String> {
        let (_, _, largeur, hauteur) = d.zone;
        if let Some(parent) = d.sortie.parent() {
            std::fs::create_dir_all(parent).map_err(|e| format!("Création de {} impossible : {e}", parent.display()))?;
        }
        let encodeur = VideoEncoder::new(
            VideoSettingsBuilder::new(largeur, hauteur).sub_type(VideoSettingsSubType::H264).frame_rate(IPS).bitrate(debit(largeur, hauteur)),
            AudioSettingsBuilder::new(),
            ContainerSettingsBuilder::new(),
            &d.sortie,
        )
        .map_err(|e| format!("Encodeur vidéo impossible à ouvrir : {e}"))?;
        let p = Arc::new(Partage {
            zone: d.zone,
            t0: OnceLock::new(),
            t0_qpc: OnceLock::new(),
            pauses: Mutex::new(Pauses::default()),
            fin: Mutex::new(None),
            abandon: AtomicBool::new(false),
            encodeur: Mutex::new(Some(encodeur)),
            surface: Mutex::new(None),
            premiere: Mutex::new(None),
            erreur: Mutex::new(None),
            niveaux: Mutex::new(Niveaux::default()),
            perdues: Mutex::new(Vec::new()),
            effets: d.effets.clone(),
            sans_effets: AtomicBool::new(false),
            composition: Mutex::new(None),
        });
        let mut enreg = Enregistrement { p: p.clone(), sortie: d.sortie.clone(), controle: None, fils: Vec::new() };
        // Les clients WASAPI restent sur le fil qui les ouvre : il dit lesquels manquent.
        let (dire, absentes) = std::sync::mpsc::channel::<Vec<String>>();
        let ps = p.clone();
        let voulues = d.reglages.clone();
        enreg.fils.push(thread::spawn(move || {
            let _ = wasapi::initialize_mta();
            let mut absentes = Vec::new();
            let mut sources = Vec::new();
            for (voulue, direction, id, genre, volume) in [
                (voulues.son_systeme, Direction::Render, voulues.sortie_id.as_deref(), Genre::Son, voulues.volume_son),
                (voulues.micro, Direction::Capture, voulues.micro_id.as_deref(), Genre::Micro, voulues.volume_micro),
            ] {
                let ouvrir = || Source::ouvrir(direction, id, genre, volume.clamp(0.0, 2.0));
                if let Some((source, note)) = ouvrir_ou_noter(voulue, genre.nom(), ouvrir, &mut absentes) {
                    absentes.extend(note);
                    sources.push(source);
                }
            }
            let _ = dire.send(absentes);
            if let Err(e) = boucle_son(&ps, sources) {
                ps.noter_erreur(format!("Son : {e}"));
            }
        }));
        let absentes = absentes.recv_timeout(Duration::from_secs(5)).unwrap_or_else(|_| vec!["sans son (périphériques muets)".to_string()]);
        let pi = p.clone();
        enreg.fils.push(thread::spawn(move || {
            if let Err(e) = boucle_images(&pi) {
                pi.noter_erreur(format!("Image : {e}"));
            }
        }));
        let ecran = ecran_de(d.ecran);
        // Le curseur agrandi est dessine par le moteur : Windows filme sans le sien.
        let curseur = if d.reglages.curseur && d.reglages.curseur_agrandi().is_none() { CursorCaptureSettings::WithCursor } else { CursorCaptureSettings::WithoutCursor };
        let reglages = |bordure| {
            Settings::new(
                ecran,
                curseur,
                bordure,
                SecondaryWindowSettings::Default,
                MinimumUpdateIntervalSettings::Custom(Duration::from_millis(1000 / IPS as u64)),
                DirtyRegionSettings::Default,
                ColorFormat::Bgra8,
                p.clone(),
            )
        };
        // Sans bordure jaune si Windows le permet ; sinon avec, sans erreur.
        let controle = Capteur::start_free_threaded(reglages(DrawBorderSettings::WithoutBorder))
            .or_else(|_| Capteur::start_free_threaded(reglages(DrawBorderSettings::Default)));
        match controle {
            Ok(controle) => enreg.controle = Some(controle),
            Err(e) => {
                enreg.abandonner();
                return Err(format!("Capture de l'écran impossible à lancer : {e:?}"));
            }
        }
        Ok((enreg, absentes))
    }

    pub fn pauser(&self) {
        if let Some(mur) = self.p.mur() {
            tenir(&self.p.pauses).pauser(mur);
        }
    }

    pub fn reprendre(&self) {
        if let Some(mur) = self.p.mur() {
            tenir(&self.p.pauses).reprendre(mur);
        }
    }

    pub fn en_pause(&self) -> bool {
        tenir(&self.p.pauses).en_pause()
    }

    /// Le temps de video d'un instant : `None` avant la premiere image ou pendant
    /// une pause. Figee a l'appel : a prendre juste avant `arreter`.
    pub fn horloge(&self) -> impl Fn(Instant) -> Option<f64> + Send + 'static {
        let t0 = self.p.t0.get().copied();
        let pauses = tenir(&self.p.pauses).clone();
        move |quand| pauses.actif(quand.checked_duration_since(t0?)?.as_secs_f64())
    }

    /// Le temps deja filme, pauses retirees.
    pub fn actif_ms(&self) -> u64 {
        self.p.mur().map_or(0, |mur| (tenir(&self.p.pauses).actif_borne(mur) * 1000.0) as u64)
    }

    pub fn niveaux(&self) -> Niveaux {
        *tenir(&self.p.niveaux)
    }

    /// Les sources perdues en route, a dire dans la barre.
    pub fn perdues(&self) -> Vec<String> {
        tenir(&self.p.perdues).clone()
    }

    /// L'erreur qui a arrete l'enregistrement, s'il y en a une.
    pub fn erreur(&self) -> Option<String> {
        tenir(&self.p.erreur).clone()
    }

    /// Arrete au temps actif de cet instant, finit le fichier et le rend.
    /// Bloquant (une a quelques dixiemes de seconde).
    pub fn arreter(mut self) -> Result<Bilan, String> {
        let fin = self.p.mur().map(|mur| tenir(&self.p.pauses).actif_borne(mur));
        let Some(fin) = fin.filter(|&f| f > 0.0) else {
            self.abandonner();
            return Err("Aucune image n'est arrivée de Windows : rien n'a été filmé.".to_string());
        };
        *tenir(&self.p.fin) = Some(fin);
        self.joindre();
        if let Some(motif) = self.erreur() {
            self.effacer();
            return Err(motif);
        }
        let encodeur = tenir(&self.p.encodeur).take();
        if let Some(encodeur) = encodeur {
            if let Err(e) = encodeur.finish() {
                self.effacer();
                return Err(format!("Fin de l'encodage impossible : {e}"));
            }
        }
        let (_, _, largeur, hauteur) = self.p.zone;
        Ok(Bilan { duree_ms: (fin * 1000.0).round() as i64, largeur, hauteur, premiere: tenir(&self.p.premiere).take() })
    }

    /// Arrete tout et efface le fichier : rien n'est garde.
    pub fn abandonner(mut self) {
        self.p.abandon.store(true, Ordering::Relaxed);
        self.joindre();
        if let Some(encodeur) = tenir(&self.p.encodeur).take() {
            let _ = encodeur.finish();
        }
        self.effacer();
    }

    fn joindre(&mut self) {
        for fil in self.fils.drain(..) {
            let _ = fil.join();
        }
        if let Some(controle) = self.controle.take() {
            let _ = controle.stop();
        }
    }

    fn effacer(&self) {
        let _ = std::fs::remove_file(&self.sortie);
    }
}

/// Une source demandee qui ne s'ouvre pas devient une ligne de `absentes`.
fn ouvrir_ou_noter<T>(voulue: bool, nom: &str, ouvrir: impl FnOnce() -> Result<T, Erreur>, absentes: &mut Vec<String>) -> Option<T> {
    if !voulue {
        return None;
    }
    match ouvrir() {
        Ok(source) => Some(source),
        Err(e) => {
            absentes.push(format!("sans {nom} ({e})"));
            None
        }
    }
}

/// L'ecran de Windows.Graphics.Capture qui contient le coin `ecran`.
fn ecran_de(ecran: (i32, i32)) -> Monitor {
    use windows62::Win32::Foundation::POINT;
    use windows62::Win32::Graphics::Gdi::{MonitorFromPoint, MONITOR_DEFAULTTONEAREST};
    let hmonitor = unsafe { MonitorFromPoint(POINT { x: ecran.0 + 1, y: ecran.1 + 1 }, MONITOR_DEFAULTTONEAREST) };
    Monitor::from_raw_hmonitor(hmonitor.0)
}

// ---------------------------------------------------------------- image

struct Capteur {
    p: Arc<Partage>,
    anneau: Vec<SendDirectX<(ID3D11Texture2D, IDirect3DSurface)>>,
    indice: usize,
}

impl GraphicsCaptureApiHandler for Capteur {
    type Flags = Arc<Partage>;
    type Error = Erreur;

    fn new(ctx: Context<Self::Flags>) -> Result<Self, Self::Error> {
        Ok(Self { p: ctx.flags, anneau: Vec::new(), indice: 0 })
    }

    fn on_frame_arrived(&mut self, frame: &mut Frame, controle: InternalCaptureControl) -> Result<(), Self::Error> {
        if self.p.fini() || tenir(&self.p.fin).is_some() {
            controle.stop();
            return Ok(());
        }
        // Une erreur ici arrete la capture : elle est notee pour que la barre la dise.
        self.recevoir(frame).inspect_err(|e| self.p.noter_erreur(format!("Image : {e}")))
    }
}

impl Capteur {
    fn recevoir(&mut self, frame: &mut Frame) -> Result<(), Erreur> {
        let (x, y, l, h) = self.p.zone;
        let source = frame.desc();
        if x + l > source.Width || y + h > source.Height {
            return Err(format!("la zone {l} x {h} en ({x}, {y}) sort de l'écran filmé ({} x {})", source.Width, source.Height).into());
        }
        if self.anneau.is_empty() {
            self.anneau = anneau(frame, l, h)?;
        }
        let boite = D3D11_BOX { left: x, top: y, front: 0, right: x + l, bottom: y + h, back: 1 };
        if self.p.effets.is_some() && !self.p.sans_effets.load(Ordering::Relaxed) && self.recevoir_pour_composer(frame, &boite, l, h)? {
            return Ok(());
        }
        let i = self.indice % self.anneau.len();
        self.indice += 1;
        unsafe {
            frame.device_context().CopySubresourceRegion(&self.anneau[i].0 .0, 0, 0, 0, 0, frame.as_raw_texture(), 0, Some(&raw const boite));
            frame.device_context().Flush();
        }
        if self.indice == 1 {
            *tenir(&self.p.premiere) = lire_texture(frame.device(), frame.device_context(), &self.anneau[i].0 .0, l, h).ok();
        }
        *tenir(&self.p.surface) = Some(SendDirectX::new(self.anneau[i].0 .1.clone()));
        self.partir();
        Ok(())
    }

    /// Le depart de la frise : la premiere image copiee.
    fn partir(&self) {
        self.p.t0.get_or_init(|| {
            let _ = self.p.t0_qpc.set(qpc_100ns());
            Instant::now()
        });
    }

    /// Avec effets : la zone va dans la texture brute, le fil des images compose.
    /// Faux si Direct2D ne s'ouvre pas : la prise passe sans effets, et le dit.
    fn recevoir_pour_composer(&mut self, frame: &mut Frame, boite: &D3D11_BOX, l: u32, h: u32) -> Result<bool, Erreur> {
        let mut composition = tenir(&self.p.composition);
        if composition.is_none() {
            let brute = anneau(frame, l, h)?.into_iter().next().ok_or("texture brute non créée")?.0 .0;
            let textures: Vec<ID3D11Texture2D> = self.anneau.iter().map(|t| t.0 .0.clone()).collect();
            let compositeur = match Compositeur::new(frame.device(), brute, &textures, l, h) {
                Ok(c) => c,
                Err(e) => {
                    tenir(&self.p.perdues).push(format!("sans effets ({e})"));
                    self.p.sans_effets.store(true, Ordering::Relaxed);
                    return Ok(false);
                }
            };
            *composition = Some(SendDirectX::new(Composition {
                compositeur,
                surfaces: self.anneau.iter().map(|t| t.0 .1.clone()).collect(),
                textures,
                device: frame.device().clone(),
                contexte: frame.device_context().clone(),
            }));
        }
        let c = &composition.as_ref().ok_or("composition absente")?.0;
        unsafe {
            c.contexte.CopySubresourceRegion(c.compositeur.brute(), 0, 0, 0, 0, frame.as_raw_texture(), 0, Some(boite));
            c.contexte.Flush();
        }
        drop(composition);
        self.partir();
        Ok(true)
    }
}

/// Quatre textures a la taille de la zone, sur la carte de la capture.
fn anneau(frame: &Frame, l: u32, h: u32) -> Result<Vec<SendDirectX<(ID3D11Texture2D, IDirect3DSurface)>>, Erreur> {
    use windows62::core::Interface;
    use windows62::Win32::Graphics::Dxgi::IDXGISurface;
    use windows62::Win32::System::WinRT::Direct3D11::CreateDirect3D11SurfaceFromDXGISurface;
    let mut d = *frame.desc();
    d.Width = l;
    d.Height = h;
    d.Usage = D3D11_USAGE_DEFAULT;
    d.BindFlags = (D3D11_BIND_RENDER_TARGET.0 | D3D11_BIND_SHADER_RESOURCE.0) as u32;
    d.CPUAccessFlags = 0;
    d.MiscFlags = 0;
    d.MipLevels = 1;
    d.ArraySize = 1;
    let mut anneau = Vec::with_capacity(4);
    for _ in 0..4 {
        let mut t = None;
        unsafe { frame.device().CreateTexture2D(&d, None, Some(&mut t))? };
        let t = t.ok_or("texture de l'anneau non créée")?;
        let s: IDirect3DSurface = unsafe { CreateDirect3D11SurfaceFromDXGISurface(&t.cast::<IDXGISurface>()?)? }.cast()?;
        anneau.push(SendDirectX::new((t, s)));
    }
    Ok(anneau)
}

/// La texture `t` (BGRA, `l` x `h`) lue par le processeur, une seule fois :
/// la vignette de la video.
fn lire_texture(device: &ID3D11Device, contexte: &ID3D11DeviceContext, t: &ID3D11Texture2D, l: u32, h: u32) -> Result<RgbaImage, Erreur> {
    let mut d = Default::default();
    unsafe { t.GetDesc(&mut d) };
    d.Usage = D3D11_USAGE_STAGING;
    d.BindFlags = 0;
    d.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
    d.MiscFlags = 0;
    d.MipLevels = 1;
    d.ArraySize = 1;
    let mut copie = None;
    unsafe { device.CreateTexture2D(&d, None, Some(&mut copie))? };
    let copie = copie.ok_or("texture de lecture non créée")?;
    let mut lue = D3D11_MAPPED_SUBRESOURCE::default();
    unsafe {
        contexte.CopyResource(&copie, t);
        contexte.Map(&copie, 0, D3D11_MAP_READ, 0, Some(&raw mut lue))?;
    }
    let mut image = RgbaImage::new(l, h);
    for ligne in 0..h as usize {
        let debut = unsafe { (lue.pData as *const u8).add(ligne * lue.RowPitch as usize) };
        let octets = unsafe { std::slice::from_raw_parts(debut, l as usize * 4) };
        for (colonne, bgra) in octets.chunks_exact(4).enumerate() {
            image.put_pixel(colonne as u32, ligne as u32, image::Rgba([bgra[2], bgra[1], bgra[0], 255]));
        }
    }
    unsafe { contexte.Unmap(&copie, 0) };
    Ok(image)
}

/// L'image `k` composee dans l'anneau ; la premiere est aussi lue pour la vignette.
fn composer(p: &Partage, k: u64, scene: &Scene) -> Result<IDirect3DSurface, Erreur> {
    let mut composition = tenir(&p.composition);
    let c = &mut composition.as_mut().ok_or("aucune image copiée")?.0;
    let i = (k as usize) % c.surfaces.len();
    c.compositeur.dessiner(i, scene)?;
    unsafe { c.contexte.Flush() };
    if k == 0 {
        let (_, _, l, h) = p.zone;
        *tenir(&p.premiere) = lire_texture(&c.device, &c.contexte, &c.textures[i], l, h).ok();
    }
    Ok(c.surfaces[i].clone())
}

/// Une image toutes les 1/30 s de temps actif : la derniere copiee, avec son
/// horodatage calcule. La pause arrete le temps actif, donc les envois.
fn boucle_images(p: &Partage) -> Result<(), Erreur> {
    let t0 = loop {
        if p.fini() {
            return Ok(());
        }
        if let Some(t) = p.t0.get() {
            break *t;
        }
        thread::sleep(Duration::from_millis(2));
    };
    for k in 0u64.. {
        let actif = k as f64 / IPS as f64;
        let mur = loop {
            if p.fini() || tenir(&p.fin).is_some_and(|fin| actif >= fin) {
                return Ok(());
            }
            let pauses = tenir(&p.pauses).clone();
            let mur = pauses.mur_de(actif);
            if !pauses.en_pause() && t0.elapsed().as_secs_f64() >= mur {
                break mur;
            }
            thread::sleep(Duration::from_millis(2));
        };
        let surface = match &p.effets {
            Some(effets) if !p.sans_effets.load(Ordering::Relaxed) => composer(p, k, &effets(t0 + Duration::from_secs_f64(mur)))?,
            _ => tenir(&p.surface).as_ref().map(|s| s.0.clone()).ok_or("aucune image copiée")?,
        };
        if let Some(encodeur) = tenir(&p.encodeur).as_mut() {
            encodeur.send_directx_surface_at(&surface, k as i64 * 10_000_000 / IPS as i64)?;
        }
    }
    Ok(())
}

// ---------------------------------------------------------------- son

fn format_f32() -> WaveFormat {
    WaveFormat::new(32, 32, &SampleType::Float, TAUX as usize, 2, None)
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum Genre {
    Micro,
    Son,
}

impl Genre {
    pub(super) fn nom(self) -> &'static str {
        match self {
            Genre::Micro => "micro",
            Genre::Son => "son de l'ordinateur",
        }
    }
}

/// L'identifiant voulu s'il s'ouvre ; sinon le defaut, avec la ligne a dire.
fn repli<T>(voulu: Option<&str>, par_id: impl FnOnce(&str) -> Result<T, Erreur>, defaut: impl FnOnce() -> Result<T, Erreur>, nom: &str) -> Result<(T, Option<String>), Erreur> {
    match voulu {
        None => Ok((defaut()?, None)),
        Some(id) => match par_id(id) {
            Ok(t) => Ok((t, None)),
            Err(_) => Ok((defaut()?, Some(format!("{nom} choisi absent, celui par défaut")))),
        },
    }
}

pub(super) struct Source {
    client: wasapi::AudioClient,
    capture: wasapi::AudioCaptureClient,
    frise: Frise,
    octets: std::collections::VecDeque<u8>,
    pub(super) genre: Genre,
    /// Le volume applique au melange, deja borne a 0..2.
    volume: f32,
}

impl Source {
    /// `Render` : la boucle du son de l'ordinateur ; `Capture` : le micro.
    /// `id` absent ou debranche : le peripherique par defaut, et la ligne qui le dit.
    pub(super) fn ouvrir(direction: Direction, id: Option<&str>, genre: Genre, volume: f32) -> Result<(Self, Option<String>), Erreur> {
        let e = DeviceEnumerator::new()?;
        // GetDevice rend aussi un peripherique debranche : seul un actif compte.
        let par_id = |i: &str| -> Result<_, Erreur> {
            let d = e.get_device(i)?;
            if d.get_state()? != DeviceState::Active {
                return Err("inactif".into());
            }
            Ok(d)
        };
        let (peripherique, note) = repli(id, par_id, || Ok(e.get_default_device(&direction)?), genre.nom())?;
        let mut client = peripherique.get_iaudioclient()?;
        // Un appareil de rendu ouvert en capture : la boucle (AUDCLNT_STREAMFLAGS_LOOPBACK).
        client.initialize_client(&format_f32(), &Direction::Capture, &StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: 2_000_000 })?;
        let capture = client.get_audiocaptureclient()?;
        client.start_stream()?;
        Ok((Self { client, capture, frise: Frise::default(), octets: Default::default(), genre, volume }, note))
    }

    /// Le prochain paquet disponible et son horodatage QPC, `None` s'il n'y en a plus.
    fn paquet(&mut self) -> Result<Option<(Vec<f32>, i64)>, Erreur> {
        match self.capture.get_next_packet_size()? {
            None | Some(0) => return Ok(None),
            Some(_) => {}
        }
        self.octets.clear();
        let info = self.capture.read_from_device_to_deque(&mut self.octets)?;
        let n = self.octets.len() / 4;
        let echantillons: Vec<f32> = if info.flags.silent {
            vec![0.0; n]
        } else {
            let (a, b) = self.octets.as_slices();
            let tout: Vec<u8> = a.iter().chain(b).copied().collect();
            tout.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
        };
        Ok(Some((echantillons, info.timestamp as i64)))
    }

    /// Tous les paquets disponibles, bout a bout, sans frise : la jauge de la preparation.
    pub(super) fn lire_brut(&mut self) -> Result<Vec<f32>, Erreur> {
        let mut tout = Vec::new();
        while let Some((echantillons, _)) = self.paquet()? {
            tout.extend(echantillons);
        }
        Ok(tout)
    }

    pub(super) fn arreter_flux(&self) {
        let _ = self.client.stop_stream();
    }

    /// Pose chaque paquet disponible sur la frise, par son horodatage QPC.
    fn lire(&mut self, p: &Partage, ecrit: i64) -> Result<(), Erreur> {
        while let Some((echantillons, horodatage)) = self.paquet()? {
            // Avant la premiere image, le son n'a pas de place : jete.
            let Some(&t0q) = p.t0_qpc.get() else { continue };
            let mur = (horodatage - t0q) as f64 / 1e7;
            if mur < 0.0 {
                continue;
            }
            // Dans une pause : jete.
            let Some(actif) = tenir(&p.pauses).actif(mur) else { continue };
            self.frise.placer((actif * TAUX as f64).round() as i64, &echantillons, ecrit);
        }
        Ok(())
    }
}

/// Le melange de toutes les sources, ecrit toutes les 10 ms jusqu'a « temps
/// actif moins la latence », puis complete exactement a la fin. Sans source,
/// une piste de silence : la duree du son suit toujours celle de l'image.
fn boucle_son(p: &Partage, mut sources: Vec<Source>) -> Result<(), Erreur> {
    let mut ecrit: i64 = 0;
    let resultat = (|| -> Result<(), Erreur> {
        loop {
            thread::sleep(Duration::from_millis(10));
            if p.fini() {
                return Ok(());
            }
            // Une source perdue (casque debranche) se tait, la video continue, la barre le dit.
            sources.retain_mut(|source| match source.lire(p, ecrit) {
                Ok(()) => true,
                Err(e) => {
                    eprintln!("video : source son perdue ({e})");
                    let _ = source.client.stop_stream();
                    let ligne = match source.genre {
                        Genre::Micro => "micro débranché",
                        Genre::Son => "son de l'ordinateur perdu",
                    };
                    tenir(&p.perdues).push(ligne.to_string());
                    false
                }
            });
            let Some(mur) = p.mur() else { continue };
            let fin = *tenir(&p.fin);
            let cible = match fin {
                Some(fin) => (fin * TAUX as f64).round() as i64,
                None => match tenir(&p.pauses).actif(mur) {
                    Some(actif) => ((actif - LATENCE_S).max(0.0) * TAUX as f64) as i64,
                    // En pause : les jauges tombent a zero.
                    None => {
                        *tenir(&p.niveaux) = Niveaux::default();
                        continue;
                    }
                },
            };
            let n = (cible - ecrit).max(0) as usize;
            if n > 0 {
                let mut melange = vec![0f32; n * 2];
                let mut niveaux = Niveaux::default();
                for source in sources.iter_mut() {
                    let morceau = source.frise.prendre(ecrit, n);
                    let niveau = crete(&morceau, source.volume);
                    match source.genre {
                        Genre::Micro => niveaux.micro = niveau,
                        Genre::Son => niveaux.son = niveau,
                    }
                    melanger(&mut melange, &morceau, source.volume);
                }
                *tenir(&p.niveaux) = niveaux;
                if let Some(encodeur) = tenir(&p.encodeur).as_mut() {
                    encodeur.send_audio_buffer(&en_pcm16(&melange), 0)?;
                }
                ecrit += n as i64;
            }
            if fin.is_some() && ecrit >= cible {
                return Ok(());
            }
        }
    })();
    for source in &sources {
        let _ = source.client.stop_stream();
    }
    resultat
}

fn qpc_100ns() -> i64 {
    use windows62::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
    let (mut c, mut f) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut c);
        let _ = QueryPerformanceFrequency(&mut f);
    }
    if f == 0 {
        return 0;
    }
    (c as i128 * 10_000_000 / f as i128) as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reglages_de_la_016_relus_sans_perte() {
        let r: Reglages = serde_json::from_str(r#"{"micro":false,"sonSysteme":true,"curseur":false}"#).expect("lu");
        assert_eq!((r.micro, r.son_systeme, r.curseur), (false, true, false));
        assert_eq!((r.micro_id, r.sortie_id), (None, None));
        assert_eq!((r.volume_micro, r.volume_son), (1.0, 1.0));
    }

    #[test]
    fn reglages_de_la_017_relus_sans_perte() {
        let r: Reglages = serde_json::from_str(r#"{"micro":false,"sonSysteme":true,"curseur":false,"microId":"m","sortieId":null,"volumeMicro":0.5,"volumeSon":2.0}"#).expect("lu");
        assert_eq!((r.micro, r.son_systeme, r.curseur, r.micro_id.as_deref(), r.volume_micro, r.volume_son), (false, true, false, Some("m"), 0.5, 2.0));
        assert_eq!((r.camera, r.camera_id, r.camera_taille, r.camera_miroir, r.camera_place), (false, None, Taille::Moyenne, true, None));
        let json = serde_json::to_string(&Reglages { camera: true, camera_taille: Taille::Grande, camera_place: Some((0.25, 1.0)), ..Reglages::default() }).expect("ecrit");
        assert!(json.contains(r#""cameraTaille":"grande""#) && json.contains(r#""cameraPlace":[0.25,1.0]"#) && json.contains(r#""cameraMiroir":true"#), "{json}");
    }

    #[test]
    fn repli_sur_le_defaut() {
        let ok = repli(Some("x"), |_| Ok::<_, Erreur>(1), || Ok(2), "micro").expect("ok");
        assert_eq!(ok, (1, None));
        let absent = repli(Some("x"), |_| Err::<i32, Erreur>("absent".into()), || Ok(2), "micro").expect("ok");
        assert_eq!(absent, (2, Some("micro choisi absent, celui par défaut".to_string())));
        assert_eq!(repli(None, |_| Ok::<_, Erreur>(1), || Ok(2), "micro").expect("ok"), (2, None));
    }

    #[test]
    fn source_absente_sans_erreur() {
        let mut absentes = Vec::new();
        let ouverte = ouvrir_ou_noter(true, "micro", || Err::<(), Erreur>("aucun micro".into()), &mut absentes);
        assert!(ouverte.is_none());
        assert_eq!(absentes, vec!["sans micro (aucun micro)".to_string()]);
        let non_voulue = ouvrir_ou_noter(false, "son", || -> Result<(), Erreur> { panic!("pas ouverte si non voulue") }, &mut absentes);
        assert!(non_voulue.is_none());
        assert_eq!(absentes.len(), 1);
    }

    /// Un bip de 200 ms a 880 Hz, amplitude 0,05, sur la sortie par defaut ; le flux
    /// s'ouvre et se ferme a chaque bip, comme dans la preuve.
    fn bip() -> Result<(), Erreur> {
        let _ = wasapi::initialize_mta();
        let peripherique = DeviceEnumerator::new()?.get_default_device(&Direction::Render)?;
        let mut c = peripherique.get_iaudioclient()?;
        c.initialize_client(&format_f32(), &Direction::Render, &StreamMode::PollingShared { autoconvert: true, buffer_duration_hns: 5_000_000 })?;
        let rendu = c.get_audiorenderclient()?;
        let n = ((TAUX as f64 * 0.2) as usize).min(c.get_available_space_in_frames()? as usize);
        let mut octets = Vec::with_capacity(n * 8);
        for i in 0..n {
            let enveloppe = (i.min(n - 1 - i) as f64 / (0.005 * TAUX as f64)).min(1.0);
            let v = (0.05 * enveloppe * (2.0 * std::f64::consts::PI * 880.0 * i as f64 / TAUX as f64).sin()) as f32;
            octets.extend_from_slice(&v.to_le_bytes());
            octets.extend_from_slice(&v.to_le_bytes());
        }
        rendu.write_to_device(n, &octets, None)?;
        c.start_stream()?;
        thread::sleep(Duration::from_millis(260));
        c.stop_stream()?;
        Ok(())
    }

    /// Le chemin de l'application (etape e2e 32) : `Direct::lancer` puis une zone
    /// decalee de 440 x 320 en (340, 340), onde et trainee, curseur 1x. Rouge le
    /// 16 octobre 2026 dans le parcours (MP4 vide, phase `filme` jamais vue).
    #[test]
    #[ignore = "ecran reel"]
    fn video_reelle_chemin_de_l_app() {
        let dossier = std::env::temp_dir().join(format!("equishot-chemin-essai-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier");
        let sortie = dossier.join("chemin.mp4");
        let reglages = Reglages { micro: false, son_systeme: false, curseur: true, onde: true, trainee: true, curseur_taille: 1.0, ..Reglages::default() };
        let options = super::super::animation::Options { onde: true, trainee: true, echelle: 1.0, zoom_auto: false, touches: Default::default() };
        let debut = Instant::now();
        let (direct, note) = super::super::direct::Direct::lancer(options, (340, 340), reglages.curseur_agrandi(), false);
        println!("Direct::lancer {} ms, note {note:?}", debut.elapsed().as_millis());
        let pour_moteur = direct.clone();
        let effets: Effets = Arc::new(move |quand| tenir(&pour_moteur).scene(quand));
        let demande = Demande { ecran: (0, 0), zone: (340, 340, 440, 320), sortie: sortie.clone(), reglages, effets: Some(effets) };
        let debut = Instant::now();
        let (enreg, absentes) = Enregistrement::demarrer(demande).expect("demarre");
        println!("demarrer {} ms, absentes {absentes:?}", debut.elapsed().as_millis());
        for _ in 0..30 {
            thread::sleep(Duration::from_millis(100));
            if let Some(e) = enreg.erreur() {
                panic!("erreur du moteur apres {} ms : {e}", debut.elapsed().as_millis());
            }
        }
        println!("perdues {:?}, actif {} ms", enreg.perdues(), enreg.actif_ms());
        let bilan = enreg.arreter().expect("arrete");
        let r = super::super::lecture::relever(&sortie).expect("relu");
        println!("bilan {} ms, releve {r:?}", bilan.duree_ms);
        let _ = std::fs::remove_dir_all(&dossier);
        assert!(bilan.duree_ms > 2500, "duree {}", bilan.duree_ms);
    }

    /// Le curseur agrandi (0.22) : la fleche de Windows (12 x 19 px opaques a 1x) dessinee
    /// 3x en (100, 100). Dans le MP4, les lignes 125 a 155, qu'une fleche 1x n'atteint
    /// pas, changent nettement par rapport a la reference.
    #[test]
    #[ignore = "ecran reel"]
    fn video_reelle_curseur_agrandi() {
        use super::super::effets::CurseurVu;
        use windows_sys::Win32::UI::WindowsAndMessaging::{LoadCursorW, IDC_ARROW};
        // SAFETY: curseur systeme partage, jamais libere.
        let fleche = unsafe { LoadCursorW(std::ptr::null_mut(), IDC_ARROW) } as isize;
        let dossier = std::env::temp_dir().join(format!("equishot-curseur-essai-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier");
        let sortie = dossier.join("curseur.mp4");
        let effets: Effets = Arc::new(move |_| Scene {
            zoom: (320.0, 180.0, 1.0),
            ondes: Vec::new(),
            segments: Vec::new(),
            projecteur: None,
            curseur: Some(CurseurVu { poignee: fleche, x: 100.0, y: 100.0, taille: 3.0 }),
            traits: Vec::new(),
            touches: None,
        });
        let reference = xcap::Monitor::all().expect("ecrans").into_iter().find(|m| m.is_primary().unwrap_or(false)).expect("principal").capture_region(0, 0, 640, 360).expect("reference");
        let reglages = Reglages { micro: false, son_systeme: false, curseur_taille: 3.0, ..Reglages::default() };
        let demande = Demande { ecran: (0, 0), zone: (0, 0, 640, 360), sortie: sortie.clone(), reglages, effets: Some(effets) };
        let (enreg, _) = Enregistrement::demarrer(demande).expect("demarre");
        thread::sleep(Duration::from_secs(2));
        enreg.arreter().expect("arrete");
        let mut lue = None;
        super::super::lecture::images(&sortie, 10.0, 0.5, |i| lue = lue.take().or(Some(i))).expect("relue");
        let lue = lue.expect("une image");
        let change = |x: u32, y: u32| {
            let (a, b) = (lue.get_pixel(x, y), reference.get_pixel(x, y));
            (0..3).map(|c| (a[c] as i32 - b[c] as i32).abs()).max().unwrap_or(0) > 60
        };
        let au_dela = (125..155).flat_map(|y| (100..140).map(move |x| (x, y))).filter(|&(x, y)| change(x, y)).count();
        let ailleurs = (200..296).flat_map(|y| (300..396).map(move |x| (x, y))).filter(|&(x, y)| change(x, y)).count();
        // La boite temoin dit seulement si le bureau a bouge pendant l'essai.
        println!("pixels changes : {au_dela} au-dela de la fleche 1x, {ailleurs} dans une boite temoin");
        if std::env::var_os("EQUISHOT_GARDER").is_none() {
            let _ = std::fs::remove_dir_all(&dossier);
        } else {
            lue.save(dossier.join("image.png")).expect("png");
        }
        assert!(au_dela > 250, "la fleche n'est pas agrandie : {au_dela}");
    }

    /// Le cout des effets en plein ecran principal : temps processeur du processus
    /// pendant 5 s, sans effets puis avec zoom, onde et projecteur (a lire, pas de seuil).
    #[test]
    #[ignore = "ecran reel, mesure"]
    fn cout_des_effets_plein_ecran() {
        use super::super::effets::{Couleur, Onde, Projecteur};
        use windows_sys::Win32::Foundation::FILETIME;
        use windows_sys::Win32::System::Threading::{GetCurrentProcess, GetProcessTimes};
        let cpu = || {
            let z = FILETIME { dwLowDateTime: 0, dwHighDateTime: 0 };
            let (mut a, mut b, mut k, mut u) = (z, z, z, z);
            unsafe { GetProcessTimes(GetCurrentProcess(), &mut a, &mut b, &mut k, &mut u) };
            let t = |f: FILETIME| ((f.dwHighDateTime as u64) << 32 | f.dwLowDateTime as u64) as f64 / 1e7;
            t(k) + t(u)
        };
        let m = xcap::Monitor::all().expect("ecrans").into_iter().find(|m| m.is_primary().unwrap_or(false)).expect("principal");
        let (l, h) = (m.width().expect("l") & !1, m.height().expect("h") & !1);
        let dossier = std::env::temp_dir().join(format!("equishot-cout-{}", std::process::id()));
        for avec in [false, true] {
            let effets: Option<Effets> = avec.then(|| {
                let debut = Instant::now();
                Arc::new(move |quand: Instant| {
                    let t = quand.saturating_duration_since(debut).as_secs_f32();
                    Scene {
                    zoom: (l as f32 / 2.0, h as f32 / 2.0, 1.5 + t.sin() * 0.5),
                    ondes: vec![Onde { x: 400.0, y: 300.0, rayon: 20.0 + (t * 60.0) % 40.0, epaisseur: 3.0, opacite: 0.8, couleur: Couleur(1.0, 0.8, 0.0) }],
                    segments: Vec::new(),
                    projecteur: Some(Projecteur { x: 400.0, y: 300.0, rayon: 200.0, opacite: 0.6 }),
                    curseur: None,
                    traits: Vec::new(),
                    touches: None,
                }}) as Effets
            });
            let demande = Demande { ecran: (0, 0), zone: (0, 0, l, h), sortie: dossier.join(format!("cout-{avec}.mp4")), reglages: Reglages { micro: false, son_systeme: false, ..Reglages::default() }, effets };
            let (enreg, _) = Enregistrement::demarrer(demande).expect("demarre");
            thread::sleep(Duration::from_secs(1));
            let (c0, t0) = (cpu(), Instant::now());
            thread::sleep(Duration::from_secs(5));
            let part = (cpu() - c0) / t0.elapsed().as_secs_f64() * 100.0;
            enreg.arreter().expect("arrete");
            println!("{l} x {h}, effets {avec} : {part:.1} % d'un coeur");
        }
        let _ = std::fs::remove_dir_all(&dossier);
    }

    /// Les effets (0.22) sur le vrai ecran : zoom 2x sur (160, 90), une onde au
    /// centre de l'image, un projecteur. L'image relue du MP4 doit porter l'onde et
    /// le zoom ; l'image d'origine est prise par xcap au meme moment.
    #[test]
    #[ignore = "ecran reel"]
    fn video_reelle_avec_effets() {
        use super::super::effets::{Couleur, Onde, Projecteur};
        let dossier = std::env::temp_dir().join(format!("equishot-effets-essai-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier");
        let sortie = dossier.join("effets.mp4");
        let appels = Arc::new(std::sync::atomic::AtomicU64::new(0));
        let compte = appels.clone();
        let effets: Effets = Arc::new(move |_t| {
            compte.fetch_add(1, Ordering::Relaxed);
            Scene {
                zoom: (160.0, 90.0, 2.0),
                ondes: vec![Onde { x: 160.0, y: 90.0, rayon: 20.0, epaisseur: 3.0, opacite: 1.0, couleur: Couleur(1.0, 0.0, 1.0) }],
                segments: Vec::new(),
                projecteur: Some(Projecteur { x: 160.0, y: 90.0, rayon: 60.0, opacite: 0.6 }),
                curseur: None,
                traits: Vec::new(),
                touches: None,
            }
        });
        let reference = xcap::Monitor::all().expect("ecrans").into_iter().find(|m| m.is_primary().unwrap_or(false)).expect("principal").capture_region(0, 0, 640, 360).expect("reference");
        let demande = Demande { ecran: (0, 0), zone: (0, 0, 640, 360), sortie: sortie.clone(), reglages: Reglages { micro: false, son_systeme: false, ..Reglages::default() }, effets: Some(effets) };
        let (enreg, _) = Enregistrement::demarrer(demande).expect("demarre");
        thread::sleep(Duration::from_secs(3));
        let bilan = enreg.arreter().expect("arrete");
        let r = super::super::lecture::relever(&sortie).expect("relu");
        println!("bilan {} ms, releve {r:?}, scenes demandees {}", bilan.duree_ms, appels.load(Ordering::Relaxed));
        assert!((r.image_s - 3.0).abs() <= 0.1 && r.trou_image_ms <= 40.0, "image {} trou {}", r.image_s, r.trou_image_ms);
        // Une scene par image envoyee, ecran immobile compris : 90 a 3 s.
        assert!((88..=92).contains(&appels.load(Ordering::Relaxed)), "scenes {}", appels.load(Ordering::Relaxed));
        let mut lue = None;
        super::super::lecture::images(&sortie, 10.0, 0.5, |i| lue = lue.take().or(Some(i))).expect("relue");
        let lue = lue.expect("une image");
        lue.save(dossier.join("image.png")).expect("png");
        reference.save(dossier.join("reference.png")).expect("png");
        // L'onde : rayon 20 x 2 = 40 px autour du centre (320, 180), magenta.
        let p = lue.get_pixel(320 + 40, 180);
        println!("onde {:?}", p);
        assert!(p[0] > 180 && p[1] < 90 && p[2] > 180, "onde absente : {p:?}");
        // Le zoom : le centre de l'image montre le point (160, 90) de la zone ; le
        // carre de 30 px autour (dans le disque eclaire) doit ressembler a la
        // reference agrandie 2x, bien plus qu'a la reference sans zoom.
        // Ecart moyen du carre de 30 px au centre de l'image avec la reference lue par `ref_xy`.
        let ecart = |ref_xy: &dyn Fn(u32, u32) -> (u32, u32)| {
            let mut s = 0.0;
            for y in 0..30 {
                for x in 0..30 {
                    let a = lue.get_pixel(305 + x, 165 + y);
                    let (rx, ry) = ref_xy(305 + x, 165 + y);
                    let b = reference.get_pixel(rx, ry);
                    s += (0..3).map(|c| (a[c] as f64 - b[c] as f64).abs()).sum::<f64>() / 3.0;
                }
            }
            s / 900.0
        };
        // Le point (X, Y) de l'image montre le point (160 + (X - 320) / 2, 90 + (Y - 180) / 2) de la zone.
        let zoome = ecart(&|x, y| (160 + x / 2 - 160, 90 + y / 2 - 90));
        let a_plat = ecart(&|x, y| (x, y));
        println!("zoom : ecart {zoome:.1} contre {a_plat:.1} sans zoom");
        assert!(zoome < 10.0 && zoome < a_plat, "le centre ne montre pas la zone zoomee : {zoome:.1} contre {a_plat:.1}");
        // Le projecteur : le coin (20, 20) de l'image montre (10, 10) de la zone, assombri a 40 %.
        let (c, rc) = (lue.get_pixel(20, 20), reference.get_pixel(10, 10));
        let lum = |p: &image::Rgba<u8>| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0;
        println!("coin {:?} reference {:?}", c, rc);
        assert!((lum(c) - 0.4 * lum(rc)).abs() < 15.0, "coin {} attendu {}", lum(c), 0.4 * lum(rc));
        if std::env::var_os("EQUISHOT_GARDER").is_none() {
            let _ = std::fs::remove_dir_all(&dossier);
        } else {
            println!("garde dans {}", dossier.display());
        }
    }

    /// Essai reel, a la main : `cargo test --lib video_reelle -- --ignored --nocapture`,
    /// quand l'ordinateur ne joue rien d'autre : la boucle prend tout le son de la
    /// machine, et une video ouverte dans un navigateur ajoute des bips (vu le 7 octobre 2026).
    /// Ecran principal, son de l'ordinateur seul (jamais le micro), bips a 1 et
    /// 3 s murales, pause de 1,5 a 2,5 s, arret a 4 s : 3 s de fichier, bips a 1 et 2 s.
    #[test]
    #[ignore = "ecran et sortie son reels"]
    fn video_reelle_zone_avec_pause() {
        let dossier = std::env::temp_dir().join(format!("equishot-video-essai-{}", std::process::id()));
        std::fs::create_dir_all(&dossier).expect("dossier");
        let sortie = dossier.join("essai.mp4");
        let demande = Demande { ecran: (0, 0), zone: (0, 0, 640, 360), sortie: sortie.clone(), reglages: Reglages { micro: false, son_systeme: true, ..Reglages::default() }, effets: None };
        let (enreg, absentes) = Enregistrement::demarrer(demande).expect("demarre");
        assert!(absentes.is_empty(), "{absentes:?}");
        let debut = Instant::now();
        let attendre = |s: f64| thread::sleep(Duration::from_secs_f64(s).saturating_sub(debut.elapsed()));
        attendre(1.0);
        bip().expect("bip 1");
        attendre(1.5);
        enreg.pauser();
        attendre(2.5);
        enreg.reprendre();
        attendre(3.0);
        bip().expect("bip 2");
        attendre(4.0);
        let bilan = enreg.arreter().expect("arrete");
        let r = super::super::lecture::relever(&sortie).expect("relu");
        println!("bilan {} ms {}x{} premiere {:?} ; releve {r:?}", bilan.duree_ms, bilan.largeur, bilan.hauteur, bilan.premiere.as_ref().map(|i| i.dimensions()));
        assert!((bilan.duree_ms - 3000).abs() <= 100, "duree {}", bilan.duree_ms);
        assert!((r.image_s - 3.0).abs() <= 0.1 && (r.son_s - 3.0).abs() <= 0.1, "image {} son {}", r.image_s, r.son_s);
        assert!(r.trou_image_ms <= 40.0 && r.trou_son_ms <= 40.0, "trous {} {}", r.trou_image_ms, r.trou_son_ms);
        assert_eq!((r.largeur, r.hauteur), (640, 360));
        let premiere = bilan.premiere.expect("premiere image");
        assert_eq!(premiere.dimensions(), (640, 360));
        // Le GIF (0.16) : 3 s a 10 i/s, a l'endroit.
        let gif = dossier.join("essai.gif");
        assert_eq!(super::super::gif::depuis_video(&sortie, &gif).expect("gif"), 30);
        let mut premiere_du_gif = None;
        super::super::lecture::images(&sortie, 10.0, 0.05, |i| premiere_du_gif = Some(i)).expect("relue");
        let lue = premiere_du_gif.expect("une image");
        let ecart = |a: &image::RgbaImage, b: &image::RgbaImage| a.pixels().zip(b.pixels()).map(|(p, q)| (p[1] as f64 - q[1] as f64).abs()).sum::<f64>() / (640.0 * 360.0);
        let (droit, envers) = (ecart(&lue, &premiere), ecart(&lue, &image::imageops::flip_vertical(&premiere)));
        println!("orientation : ecart droit {droit:.2}, a l'envers {envers:.2}");
        assert!(droit < envers, "la video relue est a l'endroit ({droit:.2} contre {envers:.2})");
        if std::env::var_os("EQUISHOT_GARDER").is_none() {
            let _ = std::fs::remove_dir_all(&dossier);
        }
        // En dernier : un autre son de la machine ajoute des bips (voir plus haut).
        assert_eq!(r.bips_s.len(), 2, "bips {:?}", r.bips_s);
        assert!((r.bips_s[0] - 1.0).abs() < 0.15 && (r.bips_s[1] - 2.0).abs() < 0.15, "bips {:?}", r.bips_s);
    }
}
