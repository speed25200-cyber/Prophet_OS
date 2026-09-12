//! Le vocabulaire visuel de Prophet OS.
//!
//! Trois règles, dont découle tout le reste.
//!
//! **Le noir est le fond, pas une couleur.** Une machine d'agents tourne souvent sans personne
//! devant l'écran ; un fond clair brûle des pixels et de l'attention pour rien. Le noir laisse la
//! lumière ne signifier que ce qui bouge.
//!
//! **L'or ne décore pas, il signale.** Sa luminosité dit l'activité : un courant vif est clair, un
//! courant arrêté s'éteint. Rien n'est doré parce que c'est joli.
//!
//! **Le rouge est réservé à ce qui exige un humain.** Il n'apparaît que pour une décision en
//! attente ou un refus. S'il servait aussi d'accent, il ne voudrait plus rien dire.

/// Une couleur en espace linéaire, telle que le GPU l'attend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Couleur {
    /// Rouge, de 0 à 1.
    pub r: f32,
    /// Vert, de 0 à 1.
    pub v: f32,
    /// Bleu, de 0 à 1.
    pub b: f32,
    /// Opacité, de 0 à 1.
    pub a: f32,
}

impl Couleur {
    /// Depuis une écriture hexadécimale en sRGB, convertie en linéaire.
    ///
    /// Les couleurs se choisissent à l'œil en sRGB et se mélangent correctement en linéaire ;
    /// confondre les deux donne des dégradés sales et des bords gris autour du doré.
    #[must_use]
    pub fn hex(code: u32) -> Self {
        let canal = |decalage: u32| {
            let v = ((code >> decalage) & 0xff) as f32 / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        Self {
            r: canal(16),
            v: canal(8),
            b: canal(0),
            a: 1.0,
        }
    }

    /// La même couleur, avec une autre opacité.
    #[must_use]
    pub const fn opacite(self, a: f32) -> Self {
        Self { a, ..self }
    }

    /// Les quatre composantes, prêtes pour un tampon uniforme.
    #[must_use]
    pub const fn tableau(self) -> [f32; 4] {
        [self.r, self.v, self.b, self.a]
    }
}

/// Le fond, presque noir mais pas tout à fait : un noir absolu écrase les dégradés.
#[must_use]
pub fn fond() -> Couleur {
    Couleur::hex(0x05_04_03)
}

/// Le fond d'un panneau, posé sur le fond général.
#[must_use]
pub fn panneau() -> Couleur {
    Couleur::hex(0x0d_0c_0a).opacite(0.82)
}

/// Le trait d'un panneau : présent, jamais appuyé.
#[must_use]
pub fn bordure() -> Couleur {
    Couleur::hex(0xd4_a4_37).opacite(0.16)
}

/// L'or vif, pour ce qui est actif.
#[must_use]
pub fn or() -> Couleur {
    Couleur::hex(0xe8_c0_6a)
}

/// L'or sourd, pour ce qui est en place mais au repos.
#[must_use]
pub fn or_eteint() -> Couleur {
    Couleur::hex(0x8a_6d_2f)
}

/// Le texte courant.
#[must_use]
pub fn texte() -> Couleur {
    Couleur::hex(0xe6_e1_d8)
}

/// Le texte secondaire : lisible, mais qui ne réclame pas le regard.
#[must_use]
pub fn texte_discret() -> Couleur {
    Couleur::hex(0x8b_85_7a)
}

/// Ce qui exige une décision humaine, et rien d'autre.
#[must_use]
pub fn attente() -> Couleur {
    Couleur::hex(0xd9_6a_4a)
}

/// Rayon des coins. Une seule valeur pour tout le système : deux rayons différents à l'écran se
/// remarquent avant qu'on sache pourquoi.
pub const RAYON: f32 = 10.0;

/// Écart de base. Toutes les distances en sont des multiples, pour que la grille se tienne sans
/// qu'on ait à la dessiner.
pub const PAS: f32 = 12.0;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn le_noir_et_le_blanc_traversent_la_conversion() {
        let noir = Couleur::hex(0x00_00_00);
        assert!((noir.r).abs() < 1e-6);
        let blanc = Couleur::hex(0xff_ff_ff);
        assert!((blanc.r - 1.0).abs() < 1e-6);
    }

    #[test]
    fn la_conversion_srgb_assombrit_les_tons_moyens() {
        // Un gris à mi-chemin en sRGB vaut environ 0,21 en linéaire. Prendre 0,5 donnerait des
        // dégradés délavés : c'est l'erreur classique, et elle se voit sur un fond noir.
        let gris = Couleur::hex(0x80_80_80);
        assert!(
            (gris.r - 0.216).abs() < 0.01,
            "gris linéaire obtenu : {}",
            gris.r
        );
    }

    #[test]
    fn l_opacite_ne_change_que_l_opacite() {
        let base = or();
        let transparent = base.opacite(0.3);
        assert!((transparent.a - 0.3).abs() < 1e-6);
        assert!((transparent.r - base.r).abs() < 1e-6);
    }
}
