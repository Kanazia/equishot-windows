//! La partie pure de la video (ADR-016) : le temps actif malgre les pauses, la
//! frise du son placee par l'horloge, le melange, le debit et la zone filmee.
//! Aucun appel a Windows : tout se teste ici.

/// Echantillons par seconde du son, par voie.
pub const TAUX: u32 = 48_000;
/// Images par seconde de la video.
pub const IPS: u32 = 30;
/// Sous cet ecart (5 ms), un decalage entre deux paquets est de la gigue.
const TOLERANCE: i64 = 240;

/// Les pauses d'un enregistrement, en secondes d'horloge murale depuis le depart.
#[derive(Debug, Default, Clone)]
pub struct Pauses {
    finies: Vec<(f64, f64)>,
    en_cours: Option<f64>,
}

impl Pauses {
    pub fn pauser(&mut self, mur: f64) {
        self.en_cours.get_or_insert(mur);
    }

    pub fn reprendre(&mut self, mur: f64) {
        if let Some(debut) = self.en_cours.take() {
            self.finies.push((debut, mur));
        }
    }

    pub fn en_pause(&self) -> bool {
        self.en_cours.is_some()
    }

    /// Le temps du fichier a l'instant `mur`, `None` pendant une pause.
    pub fn actif(&self, mur: f64) -> Option<f64> {
        if self.en_cours.is_some_and(|debut| mur >= debut) || self.finies.iter().any(|&(a, b)| mur >= a && mur < b) {
            return None;
        }
        Some(mur - self.finies.iter().filter(|&&(_, b)| b <= mur).map(|&(a, b)| b - a).sum::<f64>())
    }

    /// Le temps du fichier a l'instant `mur` ; dans une pause, celui de son debut.
    pub fn actif_borne(&self, mur: f64) -> f64 {
        let debut_de_pause = self.en_cours.filter(|&a| mur >= a).or_else(|| self.finies.iter().find(|&&(a, b)| mur >= a && mur < b).map(|&(a, _)| a));
        let m = debut_de_pause.unwrap_or(mur);
        m - self.finies.iter().filter(|&&(_, b)| b <= m).map(|&(a, b)| b - a).sum::<f64>()
    }

    /// L'instant mural ou le fichier atteint `actif`, pauses finies comprises.
    pub fn mur_de(&self, actif: f64) -> f64 {
        // Les pauses sont dans l'ordre : chacune passee repousse l'instant.
        self.finies.iter().fold(actif, |mur, &(a, b)| if mur >= a { mur + (b - a) } else { mur })
    }
}

/// Le son d'une source pose sur la frise du fichier, en trames stereo.
#[derive(Debug, Default)]
pub struct Frise {
    file: std::collections::VecDeque<f32>,
    /// Position, en trames du fichier, de `file[0]`.
    debut: i64,
}

impl Frise {
    /// Pose un paquet stereo entrelace a la position `pos` (en trames). Ce qui
    /// tombe avant `ecrit` est deja parti : il est coupe.
    pub fn placer(&mut self, pos: i64, echantillons: &[f32], ecrit: i64) {
        let (mut pos, mut s) = (pos, echantillons);
        if pos < ecrit {
            let k = ((ecrit - pos) as usize).min(s.len() / 2);
            s = &s[k * 2..];
            pos += k as i64;
        }
        if s.is_empty() {
            return;
        }
        if self.file.is_empty() {
            self.debut = pos;
        } else {
            let fin = self.debut + (self.file.len() / 2) as i64;
            if pos > fin + TOLERANCE {
                self.file.extend(std::iter::repeat_n(0.0f32, ((pos - fin) * 2) as usize));
            } else if pos < fin - TOLERANCE {
                let k = ((fin - pos) as usize).min(s.len() / 2);
                s = &s[k * 2..];
            }
        }
        self.file.extend(s.iter().copied());
    }

    /// Rend `n` trames a partir de `ecrit` : du silence la ou aucun paquet ne couvre.
    pub fn prendre(&mut self, ecrit: i64, n: usize) -> Vec<f32> {
        let mut sortie = vec![0f32; n * 2];
        if !self.file.is_empty() && self.debut < ecrit {
            let k = ((ecrit - self.debut) as usize).min(self.file.len() / 2);
            self.file.drain(..k * 2);
            self.debut += k as i64;
        }
        if !self.file.is_empty() {
            let decalage = (self.debut - ecrit) as usize;
            if decalage < n {
                let m = (n - decalage).min(self.file.len() / 2);
                for (i, v) in self.file.drain(..m * 2).enumerate() {
                    sortie[decalage * 2 + i] = v;
                }
                self.debut += m as i64;
            }
        }
        sortie
    }
}

/// Ajoute `b` a `a`, multiplie par `gain` (le volume de la source, 0 a 2).
pub fn melanger(a: &mut [f32], b: &[f32], gain: f32) {
    for (x, y) in a.iter_mut().zip(b) {
        *x += y * gain;
    }
}

/// Le niveau crete d'un morceau apres volume, borne a 1 : ce que montre la jauge.
pub fn crete(b: &[f32], gain: f32) -> f32 {
    b.iter().fold(0f32, |m, v| m.max((v * gain).abs())).min(1.0)
}

/// Flottants vers PCM 16 bits petit-boutiste, ecretes a [-1, 1] : l'entree
/// reelle de l'encodeur (`AudioEncodingProperties::CreatePcm(48000, 2, 16)`).
pub fn en_pcm16(echantillons: &[f32]) -> Vec<u8> {
    echantillons.iter().flat_map(|v| ((v.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes()).collect()
}

/// Le debit video selon la taille : 0,07 bit par pixel et par image, borne.
pub fn debit(largeur: u32, hauteur: u32) -> u32 {
    (largeur as f64 * hauteur as f64 * IPS as f64 * 0.07).round().clamp(2_000_000.0, 16_000_000.0) as u32
}

/// La zone filmee, rognee a l'ecran et de taille paire (exigence du H.264).
/// `None` si elle sort de l'ecran ou fait moins de 16 x 16.
pub fn zone_paire(x: i64, y: i64, largeur: i64, hauteur: i64, ecran: (u32, u32)) -> Option<(u32, u32, u32, u32)> {
    let (x0, y0) = (x.max(0), y.max(0));
    let (x1, y1) = ((x + largeur).min(ecran.0 as i64), (y + hauteur).min(ecran.1 as i64));
    let (l, h) = ((x1 - x0) & !1, (y1 - y0) & !1);
    (l >= 16 && h >= 16).then_some((x0 as u32, y0 as u32, l as u32, h as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stereo(valeur: f32, trames: usize) -> Vec<f32> {
        vec![valeur; trames * 2]
    }

    #[test]
    fn sans_pause_le_temps_actif_est_le_temps_mural() {
        let p = Pauses::default();
        assert_eq!(p.actif(3.5), Some(3.5));
        assert_eq!(p.mur_de(3.5), 3.5);
        assert!(!p.en_pause());
    }

    #[test]
    fn pause_jetee_et_temps_decale() {
        let mut p = Pauses::default();
        p.pauser(4.0);
        assert!(p.en_pause());
        assert_eq!(p.actif(5.0), None, "pendant la pause en cours");
        p.reprendre(6.0);
        assert!(!p.en_pause());
        assert_eq!(p.actif(3.0), Some(3.0));
        assert_eq!(p.actif(5.0), None, "dans la pause finie");
        assert_eq!(p.actif(8.0), Some(6.0));
        assert_eq!(p.mur_de(6.0), 8.0);
        assert_eq!(p.mur_de(3.0), 3.0);
    }

    #[test]
    fn actif_borne_au_debut_de_la_pause() {
        let mut p = Pauses::default();
        p.pauser(1.0);
        p.reprendre(2.0);
        p.pauser(4.0);
        assert_eq!(p.actif_borne(5.0), 3.0, "pause en cours : son debut, moins la pause finie");
        p.reprendre(6.0);
        assert_eq!(p.actif_borne(5.0), 3.0, "dans une pause finie");
        assert_eq!(p.actif_borne(1.5), 1.0);
        assert_eq!(p.actif_borne(7.0), 4.0);
    }

    #[test]
    fn deux_pauses_se_cumulent() {
        let mut p = Pauses::default();
        p.pauser(1.0);
        p.reprendre(2.0);
        p.pauser(3.0);
        p.reprendre(5.0);
        assert_eq!(p.actif(6.0), Some(3.0));
        assert_eq!(p.mur_de(3.0), 6.0);
        assert_eq!(p.mur_de(1.5), 2.5);
    }

    #[test]
    fn pause_et_reprise_repetees_sont_sans_effet() {
        let mut p = Pauses::default();
        p.reprendre(1.0);
        p.pauser(2.0);
        p.pauser(3.0);
        p.reprendre(4.0);
        p.reprendre(5.0);
        assert_eq!(p.actif(6.0), Some(4.0), "une seule pause, de 2 a 4");
    }

    #[test]
    fn paquets_consecutifs_sans_trou() {
        let mut f = Frise::default();
        f.placer(0, &stereo(0.5, 480), 0);
        f.placer(480, &stereo(0.25, 480), 0);
        let son = f.prendre(0, 960);
        assert_eq!(son[0], 0.5);
        assert_eq!(son[959 * 2], 0.25);
    }

    #[test]
    fn trou_comble_par_le_silence() {
        let mut f = Frise::default();
        f.placer(0, &stereo(0.5, 480), 0);
        f.placer(4800, &stereo(0.5, 480), 0);
        let son = f.prendre(0, 9600);
        assert_eq!(son.len(), 9600 * 2);
        assert_eq!(son[479 * 2], 0.5);
        assert_eq!(son[480 * 2], 0.0, "le trou est du silence");
        assert_eq!(son[4799 * 2], 0.0);
        assert_eq!(son[4800 * 2], 0.5, "le second paquet tombe a sa place, pas a la suite du premier");
    }

    #[test]
    fn rien_de_place_rend_du_silence() {
        let mut f = Frise::default();
        let son = f.prendre(1000, 480);
        assert!(son.iter().all(|&v| v == 0.0));
    }

    #[test]
    fn paquet_tardif_pose_apres_un_silence_rendu() {
        let mut f = Frise::default();
        let _ = f.prendre(0, 480);
        f.placer(960, &stereo(0.5, 480), 480);
        let son = f.prendre(480, 960);
        assert_eq!(son[479 * 2], 0.0);
        assert_eq!(son[480 * 2], 0.5);
    }

    #[test]
    fn gigue_toleree() {
        let mut f = Frise::default();
        f.placer(0, &stereo(0.5, 480), 0);
        // 100 trames de retard (2 ms) : de la gigue, le paquet suit sans trou.
        f.placer(580, &stereo(0.25, 480), 0);
        let son = f.prendre(0, 960);
        assert_eq!(son[480 * 2], 0.25, "pas de silence insere pour 2 ms");
    }

    #[test]
    fn retard_coupe() {
        let mut f = Frise::default();
        let _ = f.prendre(0, 480);
        // Un paquet de 0 a 960 alors que 480 trames sont deja parties.
        f.placer(0, &stereo(0.5, 960), 480);
        let son = f.prendre(480, 480);
        assert_eq!(son[0], 0.5);
    }

    #[test]
    fn recouvrement_coupe() {
        let mut f = Frise::default();
        f.placer(0, &stereo(0.5, 480), 0);
        // Ce paquet chevauche le precedent de 400 trames : on garde la suite seulement.
        f.placer(80, &stereo(0.25, 480), 0);
        let son = f.prendre(0, 560);
        assert_eq!(son[479 * 2], 0.5);
        assert_eq!(son[480 * 2], 0.25);
        assert_eq!(son[559 * 2], 0.25);
    }

    #[test]
    fn melange_additionne() {
        let mut a = vec![0.25, -0.5, 0.0];
        melanger(&mut a, &[0.25, -0.25, 1.0], 1.0);
        assert_eq!(a, vec![0.5, -0.75, 1.0]);
    }

    #[test]
    fn melanger_applique_le_volume() {
        let mut a = vec![0.1, 0.1];
        melanger(&mut a, &[0.2, -0.2], 0.0);
        assert_eq!(a, vec![0.1, 0.1]);
        melanger(&mut a, &[0.2, -0.2], 2.0);
        assert!((a[0] - 0.5).abs() < 1e-6 && (a[1] + 0.3).abs() < 1e-6);
    }

    #[test]
    fn crete_bornee_a_un() {
        assert_eq!(crete(&[], 1.0), 0.0);
        assert!((crete(&[0.1, -0.4, 0.2], 1.0) - 0.4).abs() < 1e-6);
        assert_eq!(crete(&[0.8], 2.0), 1.0);
    }

    #[test]
    fn pcm16_ecrete_et_petit_boutiste() {
        let pcm = en_pcm16(&[0.0, 1.0, -1.0, 2.0, -2.0, 0.5]);
        let lus: Vec<i16> = pcm.chunks_exact(2).map(|c| i16::from_le_bytes([c[0], c[1]])).collect();
        assert_eq!(lus, vec![0, 32767, -32767, 32767, -32767, 16383]);
    }

    #[test]
    fn debit_borne() {
        assert_eq!(debit(320, 240), 2_000_000, "plancher");
        assert_eq!(debit(1920, 1080), 4_354_560);
        assert_eq!(debit(7680, 4320), 16_000_000, "plafond");
    }

    #[test]
    fn zone_paire_dans_l_ecran() {
        assert_eq!(zone_paire(100, 50, 801, 601, (3840, 1080)), Some((100, 50, 800, 600)));
        assert_eq!(zone_paire(0, 0, 3840, 1080, (3840, 1080)), Some((0, 0, 3840, 1080)));
    }

    #[test]
    fn zone_paire_au_bord() {
        // Au bord droit : rognee a l'ecran, puis paire.
        assert_eq!(zone_paire(3000, 900, 1000, 300, (3840, 1080)), Some((3000, 900, 840, 180)));
        // Commence avant l'ecran : rognee au bord gauche.
        assert_eq!(zone_paire(-10, -10, 111, 111, (3840, 1080)), Some((0, 0, 100, 100)));
        // Trop petite une fois rognee.
        assert_eq!(zone_paire(3837, 0, 5, 300, (3840, 1080)), None);
        // Hors de l'ecran.
        assert_eq!(zone_paire(4000, 0, 200, 200, (3840, 1080)), None);
    }
}
