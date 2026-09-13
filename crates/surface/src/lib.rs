//! La surface d'observation de Prophet OS.
//!
//! Ce que l'humain voit pendant que les agents travaillent.

pub mod atelier;
pub mod bureau;
pub mod depuis;
mod desk;
pub mod disposition;
pub mod fenetre;
mod file_review;
mod file_review_view;
mod glyphes;
pub mod gpu;
mod instruments;
mod mission_details;
pub mod missions;
pub mod preparation;
mod preparation_view;
pub mod reel;
pub mod rendu;
pub mod scene;
mod supervision;
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
