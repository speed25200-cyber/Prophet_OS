//! La surface d'observation de Prophet OS.
//!
//! Ce que l'humain voit pendant que les agents travaillent.

pub mod depuis;
pub mod disposition;
pub mod fenetre;
pub mod gpu;
pub mod reel;
pub mod rendu;
pub mod scene;
pub mod theme;

#[cfg(test)]
mod tests {
    #[test]
    fn un_contexte_graphique_est_disponible_ou_l_absence_est_nommee() {
        match crate::gpu::Contexte::hors_ecran() {
            Ok(c) => eprintln!("adaptateur : {}", c.adaptateur),
            Err(e) => eprintln!("pas de contexte : {e}"),
        }
    }
}
