//! Capture defilante (ADR-015) : la zone relue pendant que la page defile, et
//! chaque vue recollee sous les precedentes. Le recollage est une fonction pure,
//! testee sur une page synthetique dont chaque ligne est connue.

use image::RgbaImage;

/// Ce qu'une nouvelle vue a donne.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pas {
    /// La page a defile de `d` lignes, recollees.
    Decale(u32),
    /// Rien n'a bouge : la fin de la page, ou une page qui ne defile pas.
    Immobile,
    /// Aucun decalage ne recolle la vue : elle est sautee.
    Introuvable,
}

/// La bande ecartee de chaque cote d'une ligne avant de la comparer : la barre de
/// defilement y vit. Celle de ShareX, avec un plancher plus bas pour les zones etroites.
pub fn bande_laterale(largeur: u32) -> u32 {
    (largeur / 20).max(16).min(largeur / 3)
}

/// Une page animee (le fond d'une carte qui pulse, un cadre qui bat, une icone qui
/// tourne : la liste animee d'une application de bureau, 13 octobre 2026) n'est jamais relue a
/// l'identique. Un pixel ne compte comme change qu'au-dela de cet ecart sur un canal,
/// une ligne reste la meme tant que moins d'un pixel sur 20 a change, et un decalage
/// tient tant que moins d'une ligne sur 10 s'ecarte. Mesure sur deux lectures de cette
/// application immobile : 1 % de lignes ecartees au bon decalage, 31 % au meilleur des faux.
const ECART_PIXEL: u8 = 40;
const PIXELS_TOLERES: usize = 20;
const LIGNES_TOLEREES: usize = 10;

/// La barre de defilement (13 octobre 2026) : un curseur gris 77 sur un rail 29, soit
/// 48 d'ecart, et 4 colonnes fixes jusqu'au bord de la fenetre. Une colonne de la bande
/// droite en fait partie quand elle change d'une vue a l'autre (au-dela de cet ecart)
/// sur plus d'une ligne recouverte sur 50 ; la barre commence a quelques colonnes du bord au plus.
const ECART_BARRE: u8 = 24;
const LIGNES_BARRE: u64 = 50;
const BORD_FIXE: usize = 6;

/// Les lignes d'une vue, sans leurs bandes laterales, pretes a etre comparees.
pub struct Lignes {
    octets: Vec<u8>,
    /// Octets d'une ligne entiere, et de chaque bande ecartee.
    ligne: usize,
    bande: usize,
    /// L'empreinte exacte de chaque ligne (FNV-1a) : le cas courant, une ligne
    /// inchangee, se tranche sans lire ses pixels.
    empreintes: Vec<u64>,
}

impl Lignes {
    pub fn de(vue: &RgbaImage) -> Self {
        let ligne = vue.width() as usize * 4;
        let bande = bande_laterale(vue.width()) as usize * 4;
        let octets = vue.as_raw().clone();
        let empreintes = octets
            .chunks_exact(ligne)
            .map(|l| l[bande..ligne - bande].iter().fold(0xcbf2_9ce4_8422_2325u64, |h, &o| (h ^ o as u64).wrapping_mul(0x100_0000_01b3)))
            .collect();
        Lignes { octets, ligne, bande, empreintes }
    }

    fn len(&self) -> usize {
        self.empreintes.len()
    }

    fn pixel(&self, x: usize, y: usize) -> &[u8] {
        &self.octets[y * self.ligne + x * 4..][..4]
    }

    fn utiles(&self, y: usize) -> &[u8] {
        &self.octets[y * self.ligne + self.bande..(y + 1) * self.ligne - self.bande]
    }

    /// La ligne `y` montre le meme contenu que la ligne `z` de `autre`, a
    /// l'animation pres.
    fn pareille(&self, y: usize, autre: &Lignes, z: usize) -> bool {
        if self.empreintes[y] == autre.empreintes[z] {
            return true;
        }
        let (a, b) = (self.utiles(y), autre.utiles(z));
        let toleres = a.len() / 4 / PIXELS_TOLERES;
        let mut changes = 0;
        for (pa, pb) in a.chunks_exact(4).zip(b.chunks_exact(4)) {
            if pa[..3].iter().zip(&pb[..3]).any(|(x, y)| x.abs_diff(*y) > ECART_PIXEL) {
                changes += 1;
                if changes > toleres {
                    return false;
                }
            }
        }
        true
    }

    /// Les deux vues montrent la meme chose, a l'animation pres : relue sans
    /// avoir defile.
    fn meme_vue(&self, autre: &Lignes) -> bool {
        self.len() == autre.len() && ecarts(self, autre, 0, 0..self.len()).is_some()
    }
}

/// Les lignes `r` de `n` qui s'ecartent de la ligne `r + d` de `p`, et celles qui
/// sont identiques ; `None` des que les ecarts depassent une ligne sur 10.
fn ecarts(p: &Lignes, n: &Lignes, d: usize, lignes: std::ops::Range<usize>) -> Option<(usize, usize)> {
    let toleres = lignes.len() / LIGNES_TOLEREES;
    let (mut ecarts, mut exactes) = (0, 0);
    for r in lignes {
        if n.empreintes[r] == p.empreintes[r + d] {
            exactes += 1;
        } else if !n.pareille(r, p, r + d) {
            ecarts += 1;
            if ecarts > toleres {
                return None;
            }
        }
    }
    Some((ecarts, exactes))
}

/// Le decalage `d` tel que la ligne `r` de `n` soit la ligne `r + d` de `p`, en
/// dehors de l'en-tete et du pied fixes ; et les hauteurs `t` de cet en-tete et `b` de ce pied. Le moins
/// d'ecarts gagne, puis le plus de lignes identiques ; a egalite (zone unie, motif
/// repete), le plus proche de `prefere`, le plus petit sans preference.
pub fn decalage(p: &Lignes, n: &Lignes, prefere: Option<u32>) -> Result<(u32, u32, u32), Pas> {
    match decalage_exact(&p.empreintes, &n.empreintes, prefere) {
        Err(Pas::Introuvable) => decalage_tolerant(p, n, prefere),
        trouve => trouve,
    }
}

/// Le recollage au pixel pres (ADR-015) : une page qui ne bouge pas s'y tranche en
/// quelques lignes lues, quand la recherche tolerante lit des lignes entieres
/// (16 s pour un plein ecran dans l'exe de debug, mesure le 13 octobre 2026).
fn decalage_exact(p: &[u64], n: &[u64], prefere: Option<u32>) -> Result<(u32, u32, u32), Pas> {
    let h = p.len().min(n.len());
    let t = (0..h).take_while(|&y| p[y] == n[y]).count();
    if t == h {
        return Err(Pas::Immobile);
    }
    let b = (0..h).rev().take_while(|&y| p[y] == n[y]).count();
    let bande = h - t - b;
    let recouvrement = (bande / 8).max(8);
    // ponytail: recherche en O(bande²) au pire, qui s'arrete au premier ecart ; une vue fait au plus la hauteur de l'ecran.
    let valides = (1..bande.saturating_sub(recouvrement) + 1).filter(|&d| (t..h - b - d).all(|r| n[r] == p[r + d]));
    let choisi = match prefere {
        Some(q) => valides.min_by_key(|&d| (d as i64 - q as i64).abs()),
        None => valides.min(),
    };
    choisi.map(|d| (d as u32, t as u32, b as u32)).ok_or(Pas::Introuvable)
}

fn decalage_tolerant(p: &Lignes, n: &Lignes, prefere: Option<u32>) -> Result<(u32, u32, u32), Pas> {
    let h = p.len().min(n.len());
    let t = (0..h).take_while(|&y| n.pareille(y, p, y)).count();
    if t == h {
        return Err(Pas::Immobile);
    }
    let b = (0..h).rev().take_while(|&y| n.pareille(y, p, y)).count();
    let bande = h - t - b;
    let recouvrement = (bande / 8).max(8);
    let proximite = |d: usize| prefere.map_or(d as i64, |q| (d as i64 - q as i64).abs());
    // ponytail: recherche en O(bande²) lignes au pire, chaque decalage s'arrete des qu'il depasse les ecarts toleres ; une vue fait au plus la hauteur de l'ecran.
    let choisi = (1..bande.saturating_sub(recouvrement) + 1)
        .filter_map(|d| ecarts(p, n, d, t..h - b - d).map(|(e, x)| (d, e, x, h - b - d - t)))
        .min_by_key(|&(d, e, x, l)| (e * h / l, std::cmp::Reverse(x * h / l), proximite(d)));
    // Relue sans avoir defile, l'animation en plus : moins d'ecarts sur place qu'au meilleur decalage.
    if let Some((e0, _)) = ecarts(p, n, 0, 0..h) {
        if choisi.map_or(true, |(_, e, _, l)| e0 * l <= e * h) {
            return Err(Pas::Immobile);
        }
    }
    choisi.map(|(d, ..)| (d as u32, t as u32, b as u32)).ok_or(Pas::Introuvable)
}

/// Le sens du defilement (ADR-019). Le recollage travaille toujours « vers le
/// bas » : chaque vue y est ramenee, l'image recollee en revient.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Sens {
    Bas,
    Haut,
    Droite,
    Gauche,
}

/// Les quatre sens, dans l'ordre ou la detection les essaie.
pub const TOUS: [Sens; 4] = [Sens::Bas, Sens::Haut, Sens::Droite, Sens::Gauche];

fn retournee(a: &RgbaImage) -> RgbaImage {
    let h = a.height();
    RgbaImage::from_fn(a.width(), h, |x, y| *a.get_pixel(x, h - 1 - y))
}

fn transposee(a: &RgbaImage) -> RgbaImage {
    RgbaImage::from_fn(a.height(), a.width(), |x, y| *a.get_pixel(y, x))
}

/// La vue ramenee dans le repere « vers le bas » : retournee vers le haut,
/// transposee vers la droite, les deux vers la gauche.
pub fn vers_bas(vue: &RgbaImage, sens: Sens) -> RgbaImage {
    match sens {
        Sens::Bas => vue.clone(),
        Sens::Haut => retournee(vue),
        Sens::Droite => transposee(vue),
        Sens::Gauche => retournee(&transposee(vue)),
    }
}

/// L'inverse exact de `vers_bas`.
pub fn depuis_bas(image: &RgbaImage, sens: Sens) -> RgbaImage {
    match sens {
        Sens::Bas => image.clone(),
        Sens::Haut => retournee(image),
        Sens::Droite => transposee(image),
        Sens::Gauche => transposee(&retournee(image)),
    }
}

/// L'image recollee, et la derniere vue qui y est entree. Tout y est dans le
/// repere « vers le bas » de `sens`.
pub struct Recollage {
    sens: Sens,
    largeur: u32,
    /// Lignes RGBA du resultat, bout a bout.
    pixels: Vec<u8>,
    precedente: Lignes,
    dernier: Option<u32>,
    /// Pour chaque colonne de la bande droite, du bord vers l'interieur : les lignes
    /// recouvertes ou elle a change d'une vue a l'autre ; et les lignes comparees.
    barre: Vec<u64>,
    comparees: u64,
}

impl Recollage {
    pub fn nouveau(premiere: &RgbaImage, sens: Sens) -> Self {
        let premiere = vers_bas(premiere, sens);
        let barre = vec![0; bande_laterale(premiere.width()) as usize];
        Recollage { sens, largeur: premiere.width(), precedente: Lignes::de(&premiere), pixels: premiere.into_raw(), dernier: None, barre, comparees: 0 }
    }

    /// La taille de l'image sur l'axe du defilement.
    pub fn longueur(&self) -> u32 {
        (self.pixels.len() / (self.largeur as usize * 4)) as u32
    }

    /// Recolle `vue` (de la taille de la premiere) : le pied de la vue precedente
    /// est retire, puis le bas de `vue` ajoute ; le resultat finit donc toujours
    /// sur le pied de la derniere vue. Rien ne change hors d'un `Decale`.
    pub fn ajouter(&mut self, vue: &RgbaImage) -> Pas {
        let vue = &vers_bas(vue, self.sens);
        let nouvelles = Lignes::de(vue);
        let (d, t, b) = match decalage(&self.precedente, &nouvelles, self.dernier) {
            Ok(trouve) => trouve,
            Err(pas) => return pas,
        };
        let ligne = self.largeur as usize * 4;
        let h = vue.height() as usize;
        let recouvertes = t as usize..h - b as usize - d as usize;
        for (k, changes) in self.barre.iter_mut().enumerate() {
            let x = self.largeur as usize - 1 - k;
            *changes += recouvertes
                .clone()
                .filter(|&r| nouvelles.pixel(x, r)[..3].iter().zip(&self.precedente.pixel(x, r + d as usize)[..3]).any(|(a, b)| a.abs_diff(*b) > ECART_BARRE))
                .count() as u64;
        }
        self.comparees += recouvertes.len() as u64;
        self.pixels.truncate(self.pixels.len() - b as usize * ligne);
        self.pixels.extend_from_slice(&vue.as_raw()[(h - b as usize - d as usize) * ligne..]);
        self.precedente = nouvelles;
        self.dernier = Some(d);
        Pas::Decale(d)
    }

    /// La colonne ou commence la barre de defilement : chaque vue y montrait son
    /// curseur a une autre place, l'image recollee en aurait plusieurs.
    fn debut_barre(&self) -> Option<u32> {
        let bouge = |k: &usize| self.barre[*k] * LIGNES_BARRE > self.comparees;
        let premiere = (0..self.barre.len()).take(BORD_FIXE + 1).find(bouge)?;
        let largeur = (premiere..self.barre.len()).take_while(bouge).count();
        Some(self.largeur - (premiere + largeur) as u32)
    }

    /// L'image recollee, dans le repere de l'ecran, sans la barre de defilement ni
    /// ce qui la suit jusqu'au bord.
    pub fn image(self) -> RgbaImage {
        let longueur = self.longueur();
        let debut_barre = self.debut_barre();
        let mut image = RgbaImage::from_raw(self.largeur, longueur, self.pixels).expect("des lignes entieres");
        if let Some(x) = debut_barre {
            image = image::imageops::crop_imm(&image, 0, 0, x, longueur).to_image();
        }
        depuis_bas(&image, self.sens)
    }
}

/// Le mode manuel (ADR-019) : le sens n'est pas connu d'avance. Les quatre
/// recollages partent de la meme premiere vue ; le premier qui recolle est
/// garde, les autres sont jetes.
pub struct Detection {
    candidats: Vec<(Sens, Recollage)>,
    /// La derniere vue lue, recollee ou non.
    lue: Lignes,
}

impl Detection {
    pub fn nouvelle(premiere: &RgbaImage) -> Self {
        Detection { candidats: TOUS.iter().map(|&s| (s, Recollage::nouveau(premiere, s))).collect(), lue: Lignes::de(premiere) }
    }

    /// Le sens garde, `None` tant que rien n'a recolle.
    #[cfg(test)]
    pub fn sens(&self) -> Option<Sens> {
        (self.candidats.len() == 1).then(|| self.candidats[0].0)
    }

    /// Un recollage qui ne rend pas `Decale` ne change pas : les essayer tous
    /// ne coute aucune copie. Une vue n'entre que relue la meme, a l'animation
    /// pres : la zone est lue pendant que la page se repeint, et une vue dechiree (le
    /// haut deja defile, le bas pas encore peint) recollerait un bas perime. En
    /// attendant : `Immobile`.
    pub fn ajouter(&mut self, vue: &RgbaImage) -> Pas {
        let lue = Lignes::de(vue);
        if !lue.meme_vue(&self.lue) {
            self.lue = lue;
            return Pas::Immobile;
        }
        self.lue = lue;
        if self.candidats.len() == 1 {
            return self.candidats[0].1.ajouter(vue);
        }
        let mut immobiles = true;
        for i in 0..self.candidats.len() {
            match self.candidats[i].1.ajouter(vue) {
                Pas::Decale(d) => {
                    let garde = self.candidats.swap_remove(i);
                    self.candidats = vec![garde];
                    return Pas::Decale(d);
                }
                Pas::Introuvable => immobiles = false,
                Pas::Immobile => {}
            }
        }
        if immobiles { Pas::Immobile } else { Pas::Introuvable }
    }

    pub fn longueur(&self) -> u32 {
        self.candidats[0].1.longueur()
    }

    pub fn image(mut self) -> RgbaImage {
        self.candidats.swap_remove(0).1.image()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const L: u32 = 200;
    /// Colonnes de la barre de defilement simulee, a droite.
    const BARRE: u32 = 10;

    /// Le pixel `x`, `y` d'un contenu dont chaque ligne differe franchement de ses
    /// voisines, comme du texte : la comparaison tolere les petits ecarts, un simple
    /// degrade y serait indiscernable.
    fn melange(x: u32, y: u32) -> image::Rgba<u8> {
        let k = (x.wrapping_mul(7919) ^ y.wrapping_mul(104_729)).wrapping_mul(2_654_435_761);
        image::Rgba([k as u8, (k >> 8) as u8, (k >> 16) as u8, 255])
    }

    /// Une page dont chaque ligne est unique ; `blanches` est une bande blanche
    /// (debut, longueur).
    fn page(hauteur: u32, blanches: Option<(u32, u32)>) -> RgbaImage {
        RgbaImage::from_fn(L, hauteur, |x, y| {
            if blanches.is_some_and(|(debut, n)| y >= debut && y < debut + n) {
                image::Rgba([255, 255, 255, 255])
            } else {
                melange(x, y)
            }
        })
    }

    /// `page` animee a la `phase` donnee, comme cette liste animee : tout
    /// le fond pulse de 12, une icone de 5 pixels et un cadre de 2 pixels changent
    /// franchement sur chaque ligne, et une ligne sur 100 (un filet) change entiere.
    fn animee(page: &RgbaImage, phase: u32) -> RgbaImage {
        RgbaImage::from_fn(page.width(), page.height(), |x, y| {
            let p = page.get_pixel(x, y).0;
            let pulse = |c: u8| if phase % 2 == 1 { c.saturating_add(12) } else { c.saturating_sub(12) };
            let franc = |c: u8| c.wrapping_add(90 + 40 * (phase % 3) as u8);
            if y % 100 == 50 || (20..25).contains(&x) || (170..172).contains(&x) {
                image::Rgba([franc(p[0]), franc(p[1]), franc(p[2]), 255])
            } else {
                image::Rgba([pulse(p[0]), pulse(p[1]), pulse(p[2]), 255])
            }
        })
    }

    /// La vue d'une fenetre de `h` lignes ouverte sur `page` a `defile` lignes du
    /// haut, avec un en-tete et un pied fixes et une barre qui bouge.
    fn vue(page: &RgbaImage, defile: u32, h: u32, entete: u32, pied: u32) -> RgbaImage {
        RgbaImage::from_fn(L, h, |x, y| {
            if x >= L - BARRE {
                image::Rgba([90, (defile % 256) as u8, 90, 255])
            } else if y < entete {
                image::Rgba([10, 200, 30, 255])
            } else if y >= h - pied {
                image::Rgba([200, 30, 10 + (x % 7) as u8, 255])
            } else {
                *page.get_pixel(x, defile + y - entete)
            }
        })
    }

    /// Recolle les vues prises aux defilements donnes ; rend l'image et les pas.
    fn recoller(page: &RgbaImage, defilements: &[u32], h: u32, entete: u32, pied: u32) -> (RgbaImage, Vec<Pas>) {
        let mut r = Recollage::nouveau(&vue(page, defilements[0], h, entete, pied), Sens::Bas);
        let pas = defilements[1..].iter().map(|&d| r.ajouter(&vue(page, d, h, entete, pied))).collect();
        (r.image(), pas)
    }

    /// L'image attendue : l'en-tete, la page jusqu'au bas de la derniere vue, le
    /// pied, sans la barre de defilement.
    fn attendue(page: &RgbaImage, dernier: u32, h: u32, entete: u32, pied: u32) -> RgbaImage {
        let contenu = h - entete - pied;
        RgbaImage::from_fn(L - BARRE, entete + dernier + contenu + pied, |x, y| {
            if y < entete {
                image::Rgba([10, 200, 30, 255])
            } else if y >= entete + dernier + contenu {
                image::Rgba([200, 30, 10 + (x % 7) as u8, 255])
            } else {
                *page.get_pixel(x, y - entete)
            }
        })
    }

    fn identiques(a: &RgbaImage, b: &RgbaImage) -> Result<(), String> {
        if a.dimensions() != b.dimensions() {
            return Err(format!("{:?} contre {:?}", a.dimensions(), b.dimensions()));
        }
        for y in 0..a.height() {
            for x in 0..a.width() {
                if a.get_pixel(x, y) != b.get_pixel(x, y) {
                    return Err(format!("premier ecart en {x},{y} : {:?} contre {:?}", a.get_pixel(x, y), b.get_pixel(x, y)));
                }
            }
        }
        Ok(())
    }

    #[test]
    fn defilante_bande_laterale_plancher_et_plafond() {
        assert_eq!(bande_laterale(200), 16);
        assert_eq!(bande_laterale(1000), 50);
        assert_eq!(bande_laterale(30), 10);
    }

    #[test]
    fn defilante_page_sans_element_fixe_recollee_exactement() {
        let p = page(1500, None);
        let (image, pas) = recoller(&p, &[0, 150, 300, 450, 600], 400, 0, 0);
        assert_eq!(pas, vec![Pas::Decale(150); 4]);
        identiques(&image, &attendue(&p, 600, 400, 0, 0)).unwrap();
    }

    #[test]
    fn defilante_en_tete_et_pied_fixes_une_seule_fois() {
        let p = page(1500, None);
        let (image, pas) = recoller(&p, &[0, 120, 240, 360], 400, 40, 30);
        assert_eq!(pas, vec![Pas::Decale(120); 3]);
        identiques(&image, &attendue(&p, 360, 400, 40, 30)).unwrap();
    }

    #[test]
    fn defilante_bande_blanche_et_pas_variables() {
        let p = page(1500, Some((500, 120)));
        let (image, pas) = recoller(&p, &[0, 100, 137, 347, 447, 547, 600], 400, 40, 30);
        assert_eq!(pas, vec![Pas::Decale(100), Pas::Decale(37), Pas::Decale(210), Pas::Decale(100), Pas::Decale(100), Pas::Decale(53)]);
        identiques(&image, &attendue(&p, 600, 400, 40, 30)).unwrap();
    }

    /// Une vue comme la liste du 13 octobre 2026 : la page, puis le rail d'une barre
    /// de 6 colonnes (29) ou le curseur (77) suit le defilement, puis 4 colonnes fixes
    /// jusqu'au bord de la fenetre.
    fn vue_a_curseur(page: &RgbaImage, defile: u32, h: u32) -> RgbaImage {
        let (haut, longueur) = (defile * h / page.height(), h * h / page.height());
        RgbaImage::from_fn(L, h, |x, y| match x {
            x if x < L - 10 => *page.get_pixel(x, defile + y),
            x if x < L - 4 => {
                let gris = if (haut..haut + longueur).contains(&y) { 77 } else { 29 };
                image::Rgba([gris, gris, gris, 255])
            }
            _ => image::Rgba([21, 21, 21, 255]),
        })
    }

    #[test]
    fn defilante_barre_de_defilement_ecartee_de_l_image() {
        let p = page(1500, None);
        for pas in [[0, 150, 300, 450], [0, 20, 40, 60]] {
            let mut r = Recollage::nouveau(&vue_a_curseur(&p, pas[0], 400), Sens::Bas);
            for &d in &pas[1..] {
                assert!(matches!(r.ajouter(&vue_a_curseur(&p, d, 400)), Pas::Decale(_)), "{pas:?} {d}");
            }
            let attendue = image::imageops::crop_imm(&p, 0, 0, L - 10, pas[3] + 400).to_image();
            identiques(&r.image(), &attendue).unwrap_or_else(|e| panic!("{pas:?} : {e}"));
        }
    }

    #[test]
    fn defilante_sans_barre_rien_n_est_rogne() {
        let p = page(1500, None);
        let sans_barre = |d: u32| image::imageops::crop_imm(&vue(&p, d, 400, 40, 30), 0, 0, L - BARRE, 400).to_image();
        let mut r = Recollage::nouveau(&sans_barre(0), Sens::Bas);
        for d in [150, 300] {
            assert_eq!(r.ajouter(&sans_barre(d)), Pas::Decale(150));
        }
        identiques(&r.image(), &attendue(&p, 300, 400, 40, 30)).unwrap();
    }

    #[test]
    fn defilante_vue_immobile_est_la_fin() {
        let p = page(1500, None);
        let (image, pas) = recoller(&p, &[0, 150, 150], 400, 40, 30);
        assert_eq!(pas, vec![Pas::Decale(150), Pas::Immobile]);
        identiques(&image, &attendue(&p, 150, 400, 40, 30)).unwrap();
    }

    #[test]
    fn defilante_vue_etrangere_sautee_puis_la_suite_recollee() {
        let p = page(1500, None);
        let autre = RgbaImage::from_fn(L, 400, |x, y| image::Rgba([(x * 3 % 256) as u8, (y * 5 % 256) as u8, 77, 255]));
        let mut r = Recollage::nouveau(&vue(&p, 0, 400, 0, 0), Sens::Bas);
        assert_eq!(r.ajouter(&autre), Pas::Introuvable);
        assert_eq!(r.longueur(), 400);
        assert_eq!(r.ajouter(&vue(&p, 150, 400, 0, 0)), Pas::Decale(150));
        identiques(&r.image(), &attendue(&p, 150, 400, 0, 0)).unwrap();
    }

    #[test]
    fn defilante_recouvrement_trop_court_refuse() {
        let p = page(1500, None);
        let mut r = Recollage::nouveau(&vue(&p, 0, 400, 0, 0), Sens::Bas);
        // 395 lignes de defilement : 5 lignes communes, sous le minimum de 50.
        assert_eq!(r.ajouter(&vue(&p, 395, 400, 0, 0)), Pas::Introuvable);
    }

    #[test]
    fn defilante_contenu_periodique_le_pas_le_plus_proche_du_precedent() {
        // Une page qui se repete toutes les 50 lignes : 20, 70, 120 et 170 recollent.
        let periodique = RgbaImage::from_fn(L, 1000, |x, y| melange(x, y % 50));
        let (p, n) = (Lignes::de(&vue(&periodique, 0, 400, 0, 0)), Lignes::de(&vue(&periodique, 120, 400, 0, 0)));
        assert_eq!(decalage(&p, &n, Some(118)), Ok((120, 0, 0)));
        assert_eq!(decalage(&p, &n, None), Ok((20, 0, 0)));
    }

    #[test]
    fn defilante_page_animee_recollee() {
        let p = page(1500, None);
        let mut r = Recollage::nouveau(&animee(&vue(&p, 0, 400, 40, 30), 0), Sens::Bas);
        let pas: Vec<Pas> = [(150, 1), (300, 2), (450, 3)].iter().map(|&(d, phase)| r.ajouter(&animee(&vue(&p, d, 400, 40, 30), phase))).collect();
        assert_eq!(pas, vec![Pas::Decale(150); 3]);
        let (image, attendue) = (Lignes::de(&r.image()), Lignes::de(&attendue(&p, 450, 400, 40, 30)));
        assert_eq!(image.len(), attendue.len());
        // Les filets animes ne ressemblent a rien de fixe : le reste doit etre a sa place.
        let filets = (0..image.len()).filter(|&y| !image.pareille(y, &attendue, y)).count();
        assert!(filets <= image.len() / 50, "{filets} lignes hors de leur place");
    }

    #[test]
    fn defilante_page_animee_relue_sur_place_est_immobile() {
        let p = page(1500, None);
        let mut r = Recollage::nouveau(&animee(&vue(&p, 150, 400, 40, 30), 0), Sens::Bas);
        assert_eq!(r.ajouter(&animee(&vue(&p, 150, 400, 40, 30), 1)), Pas::Immobile);
        assert_eq!(r.ajouter(&animee(&vue(&p, 150, 400, 40, 30), 2)), Pas::Immobile);
        assert_eq!(r.longueur(), 400);
    }

    #[test]
    fn defilante_detection_page_animee() {
        // En manuel, chaque lecture tombe sur une autre phase de l'animation.
        let p = page(1500, None);
        let mut d = Detection::nouvelle(&animee(&vue(&p, 0, 400, 0, 0), 0));
        assert_eq!(d.ajouter(&animee(&vue(&p, 0, 400, 0, 0), 1)), Pas::Immobile);
        assert_eq!(d.ajouter(&animee(&vue(&p, 120, 400, 0, 0), 2)), Pas::Immobile, "lue une fois, pas encore prise");
        assert_eq!(d.ajouter(&animee(&vue(&p, 120, 400, 0, 0), 0)), Pas::Decale(120));
        assert_eq!(d.sens(), Some(Sens::Bas));
    }

    #[test]
    fn defilante_page_trop_animee_refusee() {
        // Une ligne sur 5 qui change entiere : au-dela du tolere, rien ne recolle.
        let p = page(1500, None);
        let trop = |v: RgbaImage, phase: u32| {
            let a = animee(&v, phase);
            RgbaImage::from_fn(L, 400, |x, y| if y % 5 == 0 { image::Rgba([(phase * 97) as u8, x as u8, 3, 255]) } else { *a.get_pixel(x, y) })
        };
        let mut r = Recollage::nouveau(&trop(vue(&p, 0, 400, 0, 0), 0), Sens::Bas);
        assert_eq!(r.ajouter(&trop(vue(&p, 150, 400, 0, 0), 1)), Pas::Introuvable);
    }

    /// Deux lectures reelles d'une zone animee, immobile (`zone-a.png`, `zone-b.png`
    /// dans le dossier `EQUISHOT_VUES_ANIMEES`) : la seconde, coupee de 100 lignes
    /// en haut, doit recoller a 100 sous le haut de la premiere.
    #[test]
    #[ignore]
    fn defilante_lectures_reelles_animees() {
        let dossier = std::path::PathBuf::from(std::env::var("EQUISHOT_VUES_ANIMEES").expect("EQUISHOT_VUES_ANIMEES"));
        let a = image::open(dossier.join("zone-a.png")).unwrap().to_rgba8();
        let b = image::open(dossier.join("zone-b.png")).unwrap().to_rgba8();
        let (l, h) = a.dimensions();
        let haut = image::imageops::crop_imm(&a, 0, 0, l, h - 100).to_image();
        let bas = image::imageops::crop_imm(&b, 0, 100, l, h - 100).to_image();
        let mut r = Recollage::nouveau(&a, Sens::Bas);
        assert_eq!(r.ajouter(&b), Pas::Immobile, "relue sur place");
        let mut r2 = Recollage::nouveau(&haut, Sens::Bas);
        assert_eq!(r2.ajouter(&bas), Pas::Decale(100));
        assert_eq!(r2.longueur(), h);
    }

    /// Une image non carree dont chaque pixel dit sa place.
    fn reperee(l: u32, h: u32) -> RgbaImage {
        RgbaImage::from_fn(l, h, |x, y| image::Rgba([x as u8, y as u8, (x * 3 + y * 7) as u8, 255]))
    }

    #[test]
    fn defilante_repere_aller_retour_non_carre() {
        let a = reperee(37, 23);
        for sens in [Sens::Bas, Sens::Haut, Sens::Droite, Sens::Gauche] {
            let b = vers_bas(&a, sens);
            let attendu = if matches!(sens, Sens::Droite | Sens::Gauche) { (23, 37) } else { (37, 23) };
            assert_eq!(b.dimensions(), attendu, "{sens:?}");
            assert_eq!(depuis_bas(&b, sens), a, "{sens:?}");
        }
    }

    #[test]
    fn defilante_repere_haut_retourne_droite_transpose() {
        let a = reperee(37, 23);
        assert_eq!(vers_bas(&a, Sens::Haut).get_pixel(5, 0), a.get_pixel(5, 22));
        assert_eq!(vers_bas(&a, Sens::Droite).get_pixel(4, 9), a.get_pixel(9, 4));
        assert_eq!(vers_bas(&a, Sens::Gauche).get_pixel(4, 0), a.get_pixel(36, 4));
    }

    /// La vue de l'ecran pour `sens` : la vue « vers le bas » de `vue`, ramenee a l'ecran.
    fn vue_dans(sens: Sens, page: &RgbaImage, defile: u32, h: u32, entete: u32, pied: u32) -> RgbaImage {
        depuis_bas(&vue(page, defile, h, entete, pied), sens)
    }

    #[test]
    fn defilante_haut_droite_gauche_recolles_exactement() {
        let p = page(1500, Some((500, 120)));
        for sens in [Sens::Haut, Sens::Droite, Sens::Gauche] {
            let mut r = Recollage::nouveau(&vue_dans(sens, &p, 0, 400, 40, 30), sens);
            for d in [100, 137, 347, 447] {
                assert!(matches!(r.ajouter(&vue_dans(sens, &p, d, 400, 40, 30)), Pas::Decale(_)), "{sens:?} {d}");
            }
            assert_eq!(r.longueur(), 40 + 447 + 330 + 30, "{sens:?}");
            let image = vers_bas(&r.image(), sens);
            identiques(&image, &attendue(&p, 447, 400, 40, 30)).unwrap();
        }
    }

    #[test]
    fn defilante_detection_chaque_sens() {
        let p = page(1500, None);
        for sens in [Sens::Bas, Sens::Haut, Sens::Droite, Sens::Gauche] {
            let mut d = Detection::nouvelle(&vue_dans(sens, &p, 0, 400, 40, 30));
            assert_eq!(relue(&mut d, &vue_dans(sens, &p, 0, 400, 40, 30)), Pas::Immobile);
            assert_eq!(d.sens(), None);
            assert_eq!(relue(&mut d, &vue_dans(sens, &p, 120, 400, 40, 30)), Pas::Decale(120), "{sens:?}");
            assert_eq!(d.sens(), Some(sens));
        }
    }

    #[test]
    fn defilante_detection_droite_pas_prise_pour_bas() {
        let p = page(1500, None);
        let mut d = Detection::nouvelle(&vue_dans(Sens::Droite, &p, 0, 400, 0, 0));
        assert_eq!(relue(&mut d, &vue_dans(Sens::Droite, &p, 90, 400, 0, 0)), Pas::Decale(90));
        assert_eq!(d.sens(), Some(Sens::Droite));
    }

    #[test]
    fn defilante_detection_recul_puis_reprise() {
        let p = page(1500, None);
        let mut d = Detection::nouvelle(&vue(&p, 0, 400, 0, 0));
        assert_eq!(relue(&mut d, &vue(&p, 150, 400, 0, 0)), Pas::Decale(150));
        // Retour en arriere : introuvable dans le sens garde, rien n'est ajoute.
        assert_eq!(relue(&mut d, &vue(&p, 60, 400, 0, 0)), Pas::Introuvable);
        assert_eq!(d.sens(), Some(Sens::Bas));
        assert_eq!(d.longueur(), 550);
        // Saut trop grand puis retour pres de la derniere vue recollee : la suite reprend.
        assert_eq!(relue(&mut d, &vue(&p, 900, 400, 0, 0)), Pas::Introuvable);
        assert_eq!(relue(&mut d, &vue(&p, 300, 400, 0, 0)), Pas::Decale(150));
        identiques(&d.image(), &attendue(&p, 300, 400, 0, 0)).unwrap();
    }

    /// La vue lue, puis relue si elle n'a pas ete prise : la boucle relit la zone
    /// toutes les 150 ms, une vue posee est lue au moins deux fois.
    fn relue(d: &mut Detection, v: &RgbaImage) -> Pas {
        match d.ajouter(v) {
            Pas::Immobile => d.ajouter(v),
            pas => pas,
        }
    }

    /// Une vue lue pendant le repeint : le haut a deja defile de `apres - avant`
    /// lignes, le bas montre encore l'ancien contenu (ScrollWindow puis WM_PAINT).
    fn dechiree(page: &RgbaImage, avant: u32, apres: u32, h: u32) -> RgbaImage {
        let (vieille, neuve) = (vue(page, avant, h, 0, 0), vue(page, apres, h, 0, 0));
        let coupure = h - (apres - avant);
        RgbaImage::from_fn(L, h, |x, y| *if y < coupure { &neuve } else { &vieille }.get_pixel(x, y))
    }

    #[test]
    fn defilante_detection_vue_dechiree_pas_recollee() {
        // Parcours complet du 8 octobre 2026 : 120 lignes en double a la main.
        let p = page(1500, None);
        let mut d = Detection::nouvelle(&vue(&p, 0, 400, 0, 0));
        assert_eq!(relue(&mut d, &vue(&p, 150, 400, 0, 0)), Pas::Decale(150));
        // Lue une fois pendant le repeint, puis la page repeinte, relue.
        d.ajouter(&dechiree(&p, 150, 270, 400));
        relue(&mut d, &vue(&p, 270, 400, 0, 0));
        identiques(&d.image(), &attendue(&p, 270, 400, 0, 0)).unwrap();
    }
}

