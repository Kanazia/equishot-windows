//! Les effets pendant la prise (0.22, ADR-022) : la souris ecoutee par un crochet
//! `WH_MOUSE_LL` sur son propre fil, l'animation qu'elle nourrit, le curseur lu a
//! chaque image. Le moteur demande `scene` a chaque image envoyee.

use std::ptr::null_mut;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use super::animation::{Animateur, Bouton, Evenement, Geste, Options};
use super::effets::{CurseurVu, Scene};
use super::moteur::tenir;

/// Ce que le crochet envoie : l'origine des temps et le coin de la zone, pixels du bureau.
struct Envoi {
    vers: Sender<Evenement>,
    origine: Instant,
    coin: (i32, i32),
}

/// Une seule prise a la fois : le crochet ecrit ici.
static ENVOI: Mutex<Option<Envoi>> = Mutex::new(None);

/// Le geste d'un message de la souris ; `None` pour la molette et les boutons lateraux.
pub fn geste_du_message(message: u32) -> Option<Geste> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MBUTTONUP, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
    };
    Some(match message {
        WM_MOUSEMOVE => Geste::Bouge,
        WM_LBUTTONDOWN => Geste::Appui(Bouton::Gauche),
        WM_LBUTTONUP => Geste::Relache(Bouton::Gauche),
        WM_RBUTTONDOWN => Geste::Appui(Bouton::Droit),
        WM_RBUTTONUP => Geste::Relache(Bouton::Droit),
        WM_MBUTTONDOWN => Geste::Appui(Bouton::Milieu),
        WM_MBUTTONUP => Geste::Relache(Bouton::Milieu),
        _ => return None,
    })
}

unsafe extern "system" fn procedure(code: i32, wparam: usize, lparam: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallNextHookEx, MSLLHOOKSTRUCT};
    if code >= 0 {
        if let Some(geste) = geste_du_message(wparam as u32) {
            // SAFETY: pour WH_MOUSE_LL et code >= 0, lparam pointe un MSLLHOOKSTRUCT valide le temps de l'appel.
            let pt = unsafe { (*(lparam as *const MSLLHOOKSTRUCT)).pt };
            // Jamais d'attente ici : Windows retire un crochet trop lent.
            if let Ok(envoi) = ENVOI.try_lock() {
                if let Some(e) = envoi.as_ref() {
                    let t = e.origine.elapsed().as_secs_f64();
                    let _ = e.vers.send(Evenement { t, x: (pt.x - e.coin.0) as f32, y: (pt.y - e.coin.1) as f32, geste });
                }
            }
        }
    }
    // SAFETY: transmission telle quelle a la chaine des crochets.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

/// Le fil du crochet, arrete et joint a la fin de la prise.
pub struct EcouteSouris {
    fil: Option<JoinHandle<()>>,
    id_fil: u32,
}

impl EcouteSouris {
    fn lancer(vers: Sender<Evenement>, origine: Instant, coin: (i32, i32)) -> Result<Self, String> {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, WH_MOUSE_LL};
        *tenir(&ENVOI) = Some(Envoi { vers, origine, coin });
        let (dire, pose) = channel::<Result<u32, String>>();
        let fil = std::thread::spawn(move || {
            // SAFETY: crochet global de bas niveau, procedure de ce module ; hmod est ce module.
            let poignee = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(procedure), GetModuleHandleW(null_mut()), 0) };
            if poignee.is_null() {
                let _ = dire.send(Err("crochet de la souris refusé par Windows".to_string()));
                return;
            }
            // SAFETY: sans argument.
            let _ = dire.send(Ok(unsafe { GetCurrentThreadId() }));
            // SAFETY: MSG ecrit par l'appel ; la boucle sert le crochet jusqu'a WM_QUIT.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } > 0 {}
            // SAFETY: poignee rendue par SetWindowsHookExW.
            unsafe { UnhookWindowsHookEx(poignee) };
        });
        match pose.recv() {
            Ok(Ok(id_fil)) => Ok(EcouteSouris { fil: Some(fil), id_fil }),
            Ok(Err(motif)) => {
                let _ = fil.join();
                *tenir(&ENVOI) = None;
                Err(motif)
            }
            Err(_) => {
                *tenir(&ENVOI) = None;
                Err("fil du crochet de la souris arrêté".to_string())
            }
        }
    }
}

impl Drop for EcouteSouris {
    fn drop(&mut self) {
        use windows_sys::Win32::UI::WindowsAndMessaging::{PostThreadMessageW, WM_QUIT};
        // SAFETY: message sans pointeur vers le fil du crochet.
        unsafe { PostThreadMessageW(self.id_fil, WM_QUIT, 0, 0) };
        if let Some(fil) = self.fil.take() {
            let _ = fil.join();
        }
        *tenir(&ENVOI) = None;
    }
}

/// Le curseur courant, pixels du bureau ; `None` cache.
fn curseur() -> Option<(isize, i32, i32)> {
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetCursorInfo, CURSORINFO, CURSOR_SHOWING};
    let mut infos = CURSORINFO { cbSize: std::mem::size_of::<CURSORINFO>() as u32, flags: 0, hCursor: null_mut(), ptScreenPos: windows_sys::Win32::Foundation::POINT { x: 0, y: 0 } };
    // SAFETY: infos ecrit par l'appel, cbSize pose.
    let lu = unsafe { GetCursorInfo(&mut infos) } != 0;
    (lu && infos.flags & CURSOR_SHOWING != 0 && !infos.hCursor.is_null()).then(|| (infos.hCursor as isize, infos.ptScreenPos.x, infos.ptScreenPos.y))
}

/// Les effets d'une prise : l'animation, sa souris, et le curseur agrandi.
pub struct Direct {
    animateur: Animateur,
    recus: Receiver<Evenement>,
    origine: Instant,
    coin: (i32, i32),
    /// Le grossissement du curseur dessine par le moteur ; `None` : Windows le dessine (ou rien).
    curseur: Option<f32>,
    _ecoute: Option<EcouteSouris>,
}

pub type Partage = Arc<Mutex<Direct>>;

impl Direct {
    /// Lance l'ecoute. Sans crochet, les effets de la souris manquent, le reste tient :
    /// la note le dit.
    pub fn lancer(options: Options, coin: (i32, i32), curseur: Option<f32>) -> (Partage, Option<String>) {
        let origine = Instant::now();
        let (vers, recus) = channel();
        let (ecoute, note) = match EcouteSouris::lancer(vers, origine, coin) {
            Ok(e) => (Some(e), None),
            Err(motif) => (None, Some(format!("sans clics mis en avant ({motif})"))),
        };
        let direct = Direct { animateur: Animateur::new(options), recus, origine, coin, curseur, _ecoute: ecoute };
        (Arc::new(Mutex::new(direct)), note)
    }

    fn maintenant(&self, quand: Instant) -> f64 {
        quand.saturating_duration_since(self.origine).as_secs_f64()
    }

    /// La scene de l'image prise a `quand`.
    pub fn scene(&mut self, quand: Instant) -> Scene {
        while let Ok(e) = self.recus.try_recv() {
            self.animateur.recevoir(e);
        }
        let t = self.maintenant(quand);
        let mut scene = self.animateur.scene(t);
        scene.curseur = self.curseur.and_then(|taille| {
            let (poignee, x, y) = curseur()?;
            Some(CurseurVu { poignee, x: (x - self.coin.0) as f32, y: (y - self.coin.1) as f32, taille })
        });
        scene
    }

    pub fn basculer_zoom(&mut self) -> bool {
        let t = self.maintenant(Instant::now());
        self.animateur.basculer_zoom(t)
    }

    pub fn basculer_projecteur(&mut self) -> bool {
        let t = self.maintenant(Instant::now());
        self.animateur.basculer_projecteur(t)
    }

    /// Le zoom et le projecteur, allumes ou non.
    pub fn allumes(&self) -> (bool, bool) {
        self.animateur.allumes()
    }

    /// L'origine des temps et les evenements gardes, pour le journal.
    pub fn journal(&mut self) -> (Instant, Vec<Evenement>) {
        while let Ok(e) = self.recus.try_recv() {
            self.animateur.recevoir(e);
        }
        (self.origine, self.animateur.journal().to_vec())
    }
}

/// Un curseur en BGRA premultiplie : `noir` et `blanc` sont le meme curseur dessine
/// sur fond noir et sur fond blanc. L'ecart des deux donne l'opacite. Un pixel qui
/// inverse l'ecran (la barre de texte) devient noir.
pub fn fondre_curseur(noir: &[u8], blanc: &[u8]) -> Vec<u8> {
    let mut sortie = Vec::with_capacity(noir.len());
    for (n, b) in noir.chunks_exact(4).zip(blanc.chunks_exact(4)) {
        let ecart = (0..3).map(|c| i32::from(b[c]) - i32::from(n[c])).max().unwrap_or(0);
        if ecart < 0 {
            sortie.extend_from_slice(&[0, 0, 0, 255]);
            continue;
        }
        let alpha = (255 - ecart).clamp(0, 255) as u8;
        // Premultiplie : sur fond noir, chaque canal vaut deja couleur x alpha.
        sortie.extend_from_slice(&[n[0].min(alpha), n[1].min(alpha), n[2].min(alpha), alpha]);
    }
    sortie
}

/// Les pixels d'un curseur : largeur, hauteur, point chaud, BGRA premultiplie.
pub fn pixels_du_curseur(poignee: isize) -> Option<(u32, u32, (u32, u32), Vec<u8>)> {
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetObjectW, SelectObject, BITMAP, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{DrawIconEx, GetIconInfo, DI_NORMAL, ICONINFO};
    let poignee = poignee as windows_sys::Win32::UI::WindowsAndMessaging::HCURSOR;
    // SAFETY: lectures GDI sur des objets crees et liberes ici ; chaque pointeur pointe une variable locale.
    unsafe {
        let mut infos: ICONINFO = std::mem::zeroed();
        if GetIconInfo(poignee, &mut infos) == 0 {
            return None;
        }
        let mut bm: BITMAP = std::mem::zeroed();
        let (source, double) = if infos.hbmColor.is_null() { (infos.hbmMask, true) } else { (infos.hbmColor, false) };
        let lu = GetObjectW(source, std::mem::size_of::<BITMAP>() as i32, (&raw mut bm).cast());
        let point_chaud = (infos.xHotspot, infos.yHotspot);
        DeleteObject(infos.hbmMask);
        if !infos.hbmColor.is_null() {
            DeleteObject(infos.hbmColor);
        }
        if lu == 0 || bm.bmWidth <= 0 || bm.bmHeight <= 0 {
            return None;
        }
        let (l, h) = (bm.bmWidth, if double { bm.bmHeight / 2 } else { bm.bmHeight });
        let dc = CreateCompatibleDC(null_mut());
        let mut tete: BITMAPINFO = std::mem::zeroed();
        tete.bmiHeader = BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: l, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB, ..std::mem::zeroed() };
        let mut bits = null_mut();
        let dib = CreateDIBSection(dc, &tete, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if dib.is_null() || bits.is_null() {
            DeleteDC(dc);
            return None;
        }
        let ancien = SelectObject(dc, dib);
        let n = (l * h * 4) as usize;
        let dessiner = |fond: u8| {
            std::ptr::write_bytes(bits.cast::<u8>(), fond, n);
            DrawIconEx(dc, 0, 0, poignee, l, h, 0, null_mut(), DI_NORMAL);
            std::slice::from_raw_parts(bits.cast::<u8>(), n).to_vec()
        };
        let (noir, blanc) = (dessiner(0), dessiner(255));
        SelectObject(dc, ancien);
        DeleteObject(dib);
        DeleteDC(dc);
        Some((l as u32, h as u32, point_chaud, fondre_curseur(&noir, &blanc)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn curseur_fondu_opaque_transparent_et_inverse() {
        // Un pixel blanc opaque, un transparent, un a moitie noir, un qui inverse l'ecran.
        let noir = [255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 0, 0, 255, 255, 255, 0];
        let blanc = [255, 255, 255, 0, 255, 255, 255, 0, 128, 128, 128, 0, 0, 0, 0, 0];
        assert_eq!(fondre_curseur(&noir, &blanc), vec![255, 255, 255, 255, 0, 0, 0, 0, 0, 0, 0, 127, 0, 0, 0, 255]);
    }

    #[test]
    fn gestes_des_messages() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{WM_LBUTTONDOWN, WM_MOUSEWHEEL, WM_RBUTTONUP};
        assert_eq!(geste_du_message(WM_LBUTTONDOWN), Some(Geste::Appui(Bouton::Gauche)));
        assert_eq!(geste_du_message(WM_RBUTTONUP), Some(Geste::Relache(Bouton::Droit)));
        assert_eq!(geste_du_message(WM_MOUSEWHEEL), None);
    }

    /// Le crochet recoit la souris de Windows : un deplacement relatif d'un pixel aller
    /// et retour (SendInput), sans clic, arrive en evenement dans le canal.
    #[test]
    #[ignore = "souris reelle (un pixel)"]
    fn le_crochet_recoit_la_souris() {
        use windows_sys::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT};
        let (vers, recus) = channel();
        let ecoute = EcouteSouris::lancer(vers, Instant::now(), (0, 0)).expect("crochet pose");
        let bouger = |dx: i32| INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dx, dy: 0, mouseData: 0, dwFlags: MOUSEEVENTF_MOVE, time: 0, dwExtraInfo: 0 } } };
        let entrees = [bouger(1), bouger(-1)];
        // SAFETY: deux INPUT valides, taille exacte.
        unsafe { SendInput(2, entrees.as_ptr(), std::mem::size_of::<INPUT>() as i32) };
        let premier = recus.recv_timeout(std::time::Duration::from_secs(2)).expect("un evenement");
        assert_eq!(premier.geste, Geste::Bouge);
        drop(ecoute);
        assert!(tenir(&ENVOI).is_none(), "l'envoi est retire a l'arret");
    }

    /// Le curseur flèche de Windows se lit : taille, point chaud en haut a gauche, des pixels opaques.
    #[test]
    fn la_fleche_de_windows_se_lit() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{LoadCursorW, IDC_ARROW};
        // SAFETY: curseur systeme partage, jamais libere.
        let fleche = unsafe { LoadCursorW(null_mut(), IDC_ARROW) };
        let (l, h, chaud, px) = pixels_du_curseur(fleche as isize).expect("fleche lue");
        assert!(l >= 16 && h >= 16, "{l} x {h}");
        assert!(chaud.0 < l / 4 && chaud.1 < h / 4, "point chaud {chaud:?}");
        assert!(px.chunks_exact(4).any(|p| p[3] == 255) && px.chunks_exact(4).any(|p| p[3] == 0));
        let opaques: Vec<(u32, u32)> = px.chunks_exact(4).enumerate().filter(|(_, p)| p[3] > 0).map(|(i, _)| (i as u32 % l, i as u32 / l)).collect();
        let boite = |f: fn(&(u32, u32)) -> u32| (opaques.iter().map(f).min().unwrap_or(0), opaques.iter().map(f).max().unwrap_or(0));
        println!("fleche {l} x {h}, point chaud {chaud:?}, opaque en x {:?} et en y {:?}", boite(|p| p.0), boite(|p| p.1));
    }
}
