//! Les micros et les sorties son branches (ADR-017), lus par WASAPI. Seuls les
//! peripheriques actifs comptent : un casque debranche n'est pas propose.

use wasapi::{DeviceEnumerator, Direction};

#[derive(Debug, Clone, PartialEq, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peripherique {
    pub id: String,
    pub nom: String,
}

#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Peripheriques {
    pub micros: Vec<Peripherique>,
    pub sorties: Vec<Peripherique>,
    pub micro_defaut: Option<String>,
    pub sortie_defaut: Option<String>,
}

/// La collection de `wasapi` ne rend deja que les actifs (`DEVICE_STATE_ACTIVE`).
fn lister(e: &DeviceEnumerator, direction: &Direction) -> Result<(Vec<Peripherique>, Option<String>), String> {
    let collection = e.get_device_collection(direction).map_err(|e| e.to_string())?;
    let mut tous = Vec::new();
    for appareil in &collection {
        let Ok(appareil) = appareil else { continue };
        if let (Ok(id), Ok(nom)) = (appareil.get_id(), appareil.get_friendlyname()) {
            tous.push(Peripherique { id, nom });
        }
    }
    let defaut = e.get_default_device(direction).and_then(|d| d.get_id()).ok();
    Ok((tous, defaut))
}

/// Les deux listes. A appeler hors du thread principal (COM en MTA).
pub fn lister_tout() -> Result<Peripheriques, String> {
    let _ = wasapi::initialize_mta();
    let e = DeviceEnumerator::new().map_err(|e| format!("Périphériques son illisibles : {e}"))?;
    let (micros, micro_defaut) = lister(&e, &Direction::Capture)?;
    let (sorties, sortie_defaut) = lister(&e, &Direction::Render)?;
    Ok(Peripheriques { micros, sorties, micro_defaut, sortie_defaut })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "lit les peripheriques de la machine"]
    fn liste_reelle() {
        let p = lister_tout().expect("liste");
        println!("{p:?}");
        assert!(p.sorties.iter().all(|s| !s.id.is_empty() && !s.nom.is_empty()));
        if let Some(d) = &p.sortie_defaut {
            assert!(p.sorties.iter().any(|s| &s.id == d), "la sortie par defaut est dans la liste");
        }
    }
}
