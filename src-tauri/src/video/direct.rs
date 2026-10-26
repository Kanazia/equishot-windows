//! Les effets pendant la prise (0.22, ADR-022) : la souris ecoutee par un crochet
//! `WH_MOUSE_LL` sur son propre fil, l'animation qu'elle nourrit, le curseur lu a
//! chaque image. Le moteur demande `scene` a chaque image envoyee.
//! 0.23 (ADR-023) : le clavier ecoute par un crochet `WH_KEYBOARD_LL` sur le meme
//! fil, seulement quand les touches sont montrees ; les traits du dessin ; le
//! curseur lisse.

use std::ptr::null_mut;
use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::Instant;

use super::animation::{Animateur, Bouton, Evenement, Geste, ModeTouches, Options, Touche};
use super::effets::{CurseurVu, Scene, Trait};
use super::moteur::tenir;

/// Le curseur lisse rattrape la souris avec cette constante de temps.
const LISSAGE_S: f64 = 0.08;

/// Une frappe brute, telle que le crochet la voit.
#[derive(Debug, Clone, Copy)]
pub struct Frappe {
    pub t: f64,
    pub vk: u32,
    pub scan: u32,
    pub appui: bool,
}

/// Ou le crochet du clavier ecrit : une seule prise a la fois.
static ENVOI_CLAVIER: Mutex<Option<(Sender<Frappe>, Instant)>> = Mutex::new(None);

unsafe extern "system" fn procedure_clavier(code: i32, wparam: usize, lparam: isize) -> isize {
    use windows_sys::Win32::UI::WindowsAndMessaging::{CallNextHookEx, KBDLLHOOKSTRUCT, WM_KEYDOWN, WM_KEYUP, WM_SYSKEYDOWN, WM_SYSKEYUP};
    if code >= 0 {
        let appui = match wparam as u32 {
            WM_KEYDOWN | WM_SYSKEYDOWN => Some(true),
            WM_KEYUP | WM_SYSKEYUP => Some(false),
            _ => None,
        };
        if let Some(appui) = appui {
            // SAFETY: pour WH_KEYBOARD_LL et code >= 0, lparam pointe un KBDLLHOOKSTRUCT valide le temps de l'appel.
            let k = unsafe { &*(lparam as *const KBDLLHOOKSTRUCT) };
            // Jamais d'attente ici : Windows retire un crochet trop lent.
            if let Ok(envoi) = ENVOI_CLAVIER.try_lock() {
                if let Some((vers, origine)) = envoi.as_ref() {
                    let _ = vers.send(Frappe { t: origine.elapsed().as_secs_f64(), vk: k.vkCode, scan: k.scanCode, appui });
                }
            }
        }
    }
    // SAFETY: transmission telle quelle a la chaine des crochets.
    unsafe { CallNextHookEx(null_mut(), code, wparam, lparam) }
}

/// Ce qu'une touche affiche dans un raccourci ; `None` pour un modificateur ou une
/// touche sans nom.
pub fn nom_de_touche(vk: u32) -> Option<String> {
    let nom = match vk {
        0x41..=0x5A | 0x30..=0x39 => return char::from_u32(vk).map(String::from),
        0x60..=0x69 => return Some((vk - 0x60).to_string()),
        0x70..=0x87 => return Some(format!("F{}", vk - 0x6F)),
        0x08 => "Retour",
        0x09 => "Tab",
        0x0D => "Entrée",
        0x1B => "Échap",
        0x20 => "Espace",
        0x21 => "Pg préc",
        0x22 => "Pg suiv",
        0x23 => "Fin",
        0x24 => "Début",
        0x25 => "←",
        0x26 => "↑",
        0x27 => "→",
        0x28 => "↓",
        0x2C => "Impr écran",
        0x2D => "Inser",
        0x2E => "Suppr",
        0x6A => "*",
        0x6B => "+",
        0x6D => "-",
        0x6E => ".",
        0x6F => "/",
        0x10..=0x12 | 0x14 | 0x5B | 0x5C | 0xA0..=0xA5 => return None,
        _ => {
            use windows_sys::Win32::UI::Input::KeyboardAndMouse::{MapVirtualKeyW, MAPVK_VK_TO_CHAR};
            // SAFETY: lecture de la disposition du clavier, sans pointeur.
            let c = unsafe { MapVirtualKeyW(vk, MAPVK_VK_TO_CHAR) } & 0xFFFF;
            return char::from_u32(c).filter(|c| !c.is_control()).map(|c| c.to_uppercase().collect());
        }
    };
    Some(nom.to_string())
}

/// Le caractere que tape une touche dans la disposition de la fenetre au premier
/// plan, Maj et Alt Gr compris ; `None` s'il n'est pas imprimable ou si c'est une
/// touche morte. L'etat du clavier de Windows n'est pas touche (drapeau 4).
fn caractere(vk: u32, scan: u32, maj: bool, altgr: bool) -> Option<String> {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::{GetKeyboardLayout, ToUnicodeEx};
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
    let mut etat = [0u8; 256];
    if maj {
        etat[0x10] = 0x80;
    }
    if altgr {
        etat[0x11] = 0x80;
        etat[0x12] = 0x80;
    }
    let mut tampon = [0u16; 8];
    // SAFETY: etat fait 256 octets, tampon 8 mots ; la disposition vient du fil de la fenetre au premier plan.
    let n = unsafe {
        let disposition = GetKeyboardLayout(GetWindowThreadProcessId(GetForegroundWindow(), null_mut()));
        ToUnicodeEx(vk, scan, etat.as_ptr(), tampon.as_mut_ptr(), tampon.len() as i32, 4, disposition)
    };
    if n <= 0 {
        return None;
    }
    let texte = String::from_utf16(&tampon[..n as usize]).ok()?;
    (!texte.chars().any(char::is_control)).then_some(texte)
}

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
    /// `clavier` : ou envoyer les frappes ; `None`, pas de crochet du clavier.
    fn lancer(vers: Sender<Evenement>, origine: Instant, coin: (i32, i32), clavier: Option<Sender<Frappe>>) -> Result<Self, String> {
        use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
        use windows_sys::Win32::System::Threading::GetCurrentThreadId;
        use windows_sys::Win32::UI::WindowsAndMessaging::{GetMessageW, SetWindowsHookExW, UnhookWindowsHookEx, MSG, WH_KEYBOARD_LL, WH_MOUSE_LL};
        *tenir(&ENVOI) = Some(Envoi { vers, origine, coin });
        let avec_clavier = clavier.is_some();
        *tenir(&ENVOI_CLAVIER) = clavier.map(|c| (c, origine));
        let (dire, pose) = channel::<Result<u32, String>>();
        let fil = std::thread::spawn(move || {
            // SAFETY: crochet global de bas niveau, procedure de ce module ; hmod est ce module.
            let poignee = unsafe { SetWindowsHookExW(WH_MOUSE_LL, Some(procedure), GetModuleHandleW(null_mut()), 0) };
            if poignee.is_null() {
                let _ = dire.send(Err("crochet de la souris refusé par Windows".to_string()));
                return;
            }
            // SAFETY: idem pour le clavier ; refuse, les touches manquent et la souris tient.
            let touches = if avec_clavier { unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(procedure_clavier), GetModuleHandleW(null_mut()), 0) } } else { null_mut() };
            // SAFETY: sans argument.
            let _ = dire.send(Ok(unsafe { GetCurrentThreadId() }));
            // SAFETY: MSG ecrit par l'appel ; la boucle sert les crochets jusqu'a WM_QUIT.
            let mut msg: MSG = unsafe { std::mem::zeroed() };
            while unsafe { GetMessageW(&mut msg, null_mut(), 0, 0) } > 0 {}
            // SAFETY: poignees rendues par SetWindowsHookExW.
            unsafe {
                UnhookWindowsHookEx(poignee);
                if !touches.is_null() {
                    UnhookWindowsHookEx(touches);
                }
            }
        });
        match pose.recv() {
            Ok(Ok(id_fil)) => Ok(EcouteSouris { fil: Some(fil), id_fil }),
            Ok(Err(motif)) => {
                let _ = fil.join();
                *tenir(&ENVOI) = None;
                *tenir(&ENVOI_CLAVIER) = None;
                Err(motif)
            }
            Err(_) => {
                *tenir(&ENVOI) = None;
                *tenir(&ENVOI_CLAVIER) = None;
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
        *tenir(&ENVOI_CLAVIER) = None;
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
    /// Le curseur lisse (0.23) : sa derniere place dessinee et l'instant.
    lisse: bool,
    place_lissee: Option<(f32, f32, f64)>,
    recus_clavier: Option<Receiver<Frappe>>,
    /// Le dessin a l'ecran (0.23) : ouvert, et ses traits en pixels de la zone.
    dessin: bool,
    traits: Vec<Trait>,
    _ecoute: Option<EcouteSouris>,
}

pub type Partage = Arc<Mutex<Direct>>;

impl Direct {
    /// Lance l'ecoute. Sans crochet, les effets de la souris manquent, le reste tient :
    /// la note le dit.
    /// `lisse` : le curseur dessine par le moteur suit la souris avec douceur.
    pub fn lancer(options: Options, coin: (i32, i32), curseur: Option<f32>, lisse: bool) -> (Partage, Option<String>) {
        let origine = Instant::now();
        let (vers, recus) = channel();
        // Pas de crochet du clavier quand les touches ne se montrent pas.
        let (vers_clavier, recus_clavier) = if options.touches == ModeTouches::Aucune {
            (None, None)
        } else {
            let (v, r) = channel();
            (Some(v), Some(r))
        };
        let (ecoute, note) = match EcouteSouris::lancer(vers, origine, coin, vers_clavier) {
            Ok(e) => (Some(e), None),
            Err(motif) => (None, Some(format!("sans clics mis en avant ({motif})"))),
        };
        let direct = Direct {
            animateur: Animateur::new(options),
            recus,
            origine,
            coin,
            curseur,
            lisse,
            place_lissee: None,
            recus_clavier,
            dessin: false,
            traits: Vec::new(),
            _ecoute: ecoute,
        };
        (Arc::new(Mutex::new(direct)), note)
    }

    fn maintenant(&self, quand: Instant) -> f64 {
        quand.saturating_duration_since(self.origine).as_secs_f64()
    }

    /// Donne a l'animation ce que les crochets ont recu. Pendant le dessin, un appui
    /// trace un trait : il ne fait ni onde, ni trainee, ni zoom.
    fn vider(&mut self) {
        while let Ok(mut e) = self.recus.try_recv() {
            if self.dessin && matches!(e.geste, Geste::Appui(_)) {
                e.geste = Geste::Bouge;
            }
            self.animateur.recevoir(e);
        }
        if let Some(recus) = &self.recus_clavier {
            while let Ok(f) = recus.try_recv() {
                let (maj, altgr) = self.animateur.modificateurs();
                let caractere = if f.appui { caractere(f.vk, f.scan, maj, altgr) } else { None };
                self.animateur.recevoir_touche(&Touche { t: f.t, vk: f.vk, appui: f.appui, nom: nom_de_touche(f.vk), caractere });
            }
        }
    }

    /// La scene de l'image prise a `quand`.
    pub fn scene(&mut self, quand: Instant) -> Scene {
        self.vider();
        let t = self.maintenant(quand);
        let mut scene = self.animateur.scene(t);
        scene.curseur = match (self.curseur, curseur()) {
            (Some(taille), Some((poignee, x, y))) => {
                let (mut x, mut y) = ((x - self.coin.0) as f32, (y - self.coin.1) as f32);
                if self.lisse {
                    let (px, py, pt) = self.place_lissee.unwrap_or((x, y, t));
                    let k = 1.0 - (-(t - pt).max(0.0) / LISSAGE_S).exp() as f32;
                    (x, y) = (px + (x - px) * k, py + (y - py) * k);
                    self.place_lissee = Some((x, y, t));
                }
                Some(CurseurVu { poignee, x, y, taille })
            }
            _ => None,
        };
        if self.dessin {
            scene.traits = self.traits.clone();
        }
        scene
    }

    /// Ouvre ou ferme le dessin ; ferme, ses traits s'effacent.
    pub fn dessin(&mut self, ouvert: bool) {
        self.dessin = ouvert;
        self.traits.clear();
    }

    pub fn traits(&mut self, traits: Vec<Trait>) {
        if self.dessin {
            self.traits = traits;
        }
    }

    pub fn dessin_ouvert(&self) -> bool {
        self.dessin
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
        self.vider();
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
    fn noms_des_touches() {
        assert_eq!(nom_de_touche(0x53).as_deref(), Some("S"));
        assert_eq!(nom_de_touche(0x31).as_deref(), Some("1"));
        assert_eq!(nom_de_touche(0x74).as_deref(), Some("F5"));
        assert_eq!(nom_de_touche(0x0D).as_deref(), Some("Entrée"));
        assert_eq!(nom_de_touche(0x25).as_deref(), Some("←"));
        assert_eq!(nom_de_touche(0xA2), None, "Ctrl gauche est un modificateur");
        assert_eq!(nom_de_touche(0x5B), None, "Win est un modificateur");
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
        let ecoute = EcouteSouris::lancer(vers, Instant::now(), (0, 0), None).expect("crochet pose");
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
