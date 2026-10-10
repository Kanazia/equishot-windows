//! L'ecoute de la preparation (ADR-017) : les sources choisies s'ouvrent pour
//! nourrir les jauges ; rien n'est garde. Le fil tient ses clients WASAPI.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use wasapi::Direction;

use super::frise::crete;
use super::moteur::{tenir, Genre, Niveaux, Reglages, Source};

type Choix = (bool, Option<String>, bool, Option<String>);

fn choix(r: &Reglages) -> Choix {
    (r.micro, r.micro_id.clone(), r.son_systeme, r.sortie_id.clone())
}

pub struct Ecoute {
    fin: Arc<AtomicBool>,
    niveaux: Arc<Mutex<Niveaux>>,
    volumes: Arc<Mutex<(f32, f32)>>,
    notes: Arc<Mutex<Vec<String>>>,
    choix: Choix,
    fil: Option<JoinHandle<()>>,
}

impl Ecoute {
    pub fn lancer(r: &Reglages) -> Ecoute {
        let fin = Arc::new(AtomicBool::new(false));
        let niveaux = Arc::new(Mutex::new(Niveaux::default()));
        let volumes = Arc::new(Mutex::new((r.volume_micro.clamp(0.0, 2.0), r.volume_son.clamp(0.0, 2.0))));
        let notes = Arc::new(Mutex::new(Vec::new()));
        let c = choix(r);
        let (f, n, v, no, c2) = (fin.clone(), niveaux.clone(), volumes.clone(), notes.clone(), c.clone());
        let fil = thread::spawn(move || {
            let _ = wasapi::initialize_mta();
            let mut sources = Vec::new();
            for (voulue, direction, id, genre) in [(c2.0, Direction::Capture, c2.1, Genre::Micro), (c2.2, Direction::Render, c2.3, Genre::Son)] {
                if !voulue {
                    continue;
                }
                match Source::ouvrir(direction, id.as_deref(), genre, 1.0) {
                    // Le repli d'un peripherique absent, le panneau le dit deja depuis sa liste.
                    Ok((source, _repli)) => sources.push(source),
                    Err(e) => tenir(&no).push(format!("sans {} ({e})", genre.nom())),
                }
            }
            while !f.load(Ordering::Relaxed) {
                thread::sleep(Duration::from_millis(50));
                let (vm, vs) = *tenir(&v);
                let mut mesure = Niveaux::default();
                for source in sources.iter_mut() {
                    // ponytail: une source perdue en preparation montre une jauge vide ; la barre le dira au lancement.
                    let brut = source.lire_brut().unwrap_or_default();
                    match source.genre {
                        Genre::Micro => mesure.micro = crete(&brut, vm),
                        Genre::Son => mesure.son = crete(&brut, vs),
                    }
                }
                *tenir(&n) = mesure;
            }
            for source in &sources {
                source.arreter_flux();
            }
        });
        Ecoute { fin, niveaux, volumes, notes, choix: c, fil: Some(fil) }
    }

    pub fn niveaux(&self) -> Niveaux {
        *tenir(&self.niveaux)
    }

    /// Les sources qui n'ont pas pu s'ouvrir (« sans micro (...) »).
    pub fn notes(&self) -> Vec<String> {
        tenir(&self.notes).clone()
    }

    pub fn regler_volumes(&self, micro: f32, son: f32) {
        *tenir(&self.volumes) = (micro.clamp(0.0, 2.0), son.clamp(0.0, 2.0));
    }

    /// Vrai si les memes sources sont choisies : seul le volume a change.
    pub fn meme_choix(&self, r: &Reglages) -> bool {
        self.choix == choix(r)
    }

    /// Arrete les flux et attend le fil (bloquant : hors du thread principal).
    pub fn arreter(mut self) {
        self.fin.store(true, Ordering::Relaxed);
        if let Some(fil) = self.fil.take() {
            let _ = fil.join();
        }
    }
}

/// Une ecoute lachee sans `arreter` (barre impossible a ouvrir) s'arrete seule, sans attendre.
impl Drop for Ecoute {
    fn drop(&mut self) {
        self.fin.store(true, Ordering::Relaxed);
    }
}
