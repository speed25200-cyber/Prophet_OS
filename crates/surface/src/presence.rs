//! La preuve de présence de l'humain pour accorder (ADR 0057), côté surface.
//!
//! capd refuse d'accorder depuis la session de l'humain sans son code d'approbation. Quand il le
//! demande, la source réelle le dit ici ; la surface ouvre un champ, l'humain tape son code, et la
//! source obtient un ticket de dix minutes qu'elle garde en mémoire, puis refait l'accord.

/// Une demande de code en cours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Demande {
    /// La demande d'approbation à accorder une fois la preuve faite.
    pub id: String,
    /// Portée de l'accord : `once` ou `task`.
    pub portee: String,
    /// Ce que capd a dit la dernière fois : code faux, verrou…
    pub message: String,
    /// Aucun code n'est encore défini sur cette machine : l'humain en choisit un.
    pub definir: bool,
}

impl Demande {
    /// Choisir son code sans décision en attente : depuis la page Système, ou au premier
    /// lancement quand aucun n'est encore défini.
    #[must_use]
    pub fn definir_seulement() -> Self {
        Self {
            id: String::new(),
            portee: String::new(),
            message: String::new(),
            definir: true,
        }
    }

    /// Aucune décision n'attend ce code : il est seulement choisi.
    #[must_use]
    pub fn sans_decision(&self) -> bool {
        self.id.is_empty()
    }
}

/// Ce que capd dit du code d'approbation de cette machine (`approval.code_status`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EtatDuCode {
    /// Un code est défini.
    pub defini: bool,
    /// Secondes de verrou restantes après trop de codes faux.
    pub verrou_s: Option<i64>,
}

/// Le code d'approbation compte au moins autant de caractères (le même seuil que capd).
pub const LONGUEUR_MIN: usize = 6;
