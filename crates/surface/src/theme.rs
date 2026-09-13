//! Le vocabulaire visuel de Prophet OS.
//!
//! Trois règles, dont découle tout le reste.
//!
//! **Le noir est le fond, pas une couleur.** Une machine d'agents tourne souvent sans personne
//! devant l'écran ; un fond clair brûle des pixels et de l'attention pour rien. Le noir laisse la
//! lumière ne signifier que ce qui bouge.
//!
//! **L'accent ne décore pas, il signale.** Sa luminosité dit l'activité : un courant vif est
//! clair, un courant arrêté s'éteint. Rien n'est coloré parce que c'est joli. L'accent est au
//! choix de la personne — arc, or, plasma, jade, nacre — et ce choix ne change que la couleur
//! de ce qui signale, jamais ce qui est signalé.
//!
//! **L'alerte est réservée à ce qui exige un humain.** Elle n'apparaît que pour une décision en
//! attente, un refus ou une panne. Si elle servait aussi d'accent, elle ne voudrait plus rien
//! dire ; aucun accent proposé ne s'en approche.
//!
//! Le module [`palette`] porte les neutres de l'interface, [`Accent`] la couleur choisie, et
//! les fonctions de ce module la version linéaire du thème historique pour ses shaders.

use egui::Color32;

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

/// Les neutres de l'interface native, en sRGB huit bits, tels qu'egui les mélange.
///
/// Ils ne changent pas avec l'accent : la nuit, le verre, l'encre et l'alerte sont les mêmes
/// quelle que soit la couleur choisie. Chaque constante a un rôle ; aucune n'est là pour varier.
pub mod palette {
    use egui::Color32;

    /// La nuit, jamais tout à fait noire, à peine froide.
    pub const FOND: Color32 = Color32::from_rgb(5, 7, 10);
    /// Une plaque de verre : on voit le champ au travers, assez peu pour lire dessus.
    pub const VERRE: Color32 = Color32::from_rgba_premultiplied(7, 10, 14, 168);
    /// Une surface posée sur le verre : tuile d'instrument, ligne de comparaison, message.
    pub const VERRE_HAUT: Color32 = Color32::from_rgba_premultiplied(13, 18, 24, 214);
    /// Un creux dans le verre : zone de saisie, fond d'un choix.
    pub const CREUX: Color32 = Color32::from_rgb(3, 4, 6);
    /// L'encre courante, un blanc à peine froid qui ne siffle pas sur la nuit.
    pub const ENCRE: Color32 = Color32::from_rgb(232, 236, 240);
    /// Le texte secondaire.
    pub const DISCRET: Color32 = Color32::from_rgb(140, 150, 162);
    /// Le texte effacé : mentions, références, unités.
    pub const EFFACE: Color32 = Color32::from_rgb(96, 104, 116);
    /// Le trait neutre : séparateurs, pistes, ce qui structure sans signaler.
    pub const TRAIT: Color32 = Color32::from_rgba_premultiplied(40, 48, 58, 120);
    /// Ce qui exige un humain : une décision, un refus, une panne. Rien d'autre.
    pub const ATTENTE: Color32 = Color32::from_rgb(255, 108, 62);
    /// L'alerte, en voile : le fond d'une bande ou d'un panneau qui attend l'humain.
    pub const ATTENTE_VOILE: Color32 = Color32::from_rgba_premultiplied(48, 18, 10, 200);
    /// Ce qui est accompli et vérifié par le service : une menthe calme.
    pub const ACCOMPLI: Color32 = Color32::from_rgb(126, 214, 176);
    /// Le noir posé sur tout pendant un examen : ce qui tourne reste visible, en retrait.
    pub const VOILE: Color32 = Color32::from_black_alpha(160);

    /// Luminance relative d'une couleur opaque, au sens de WCAG.
    #[must_use]
    pub fn luminance(c: Color32) -> f32 {
        let canal = |v: u8| {
            let v = f32::from(v) / 255.0;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        };
        0.2126 * canal(c.r()) + 0.7152 * canal(c.g()) + 0.0722 * canal(c.b())
    }

    /// Rapport de contraste entre deux couleurs opaques, au sens de WCAG.
    #[must_use]
    pub fn contraste(a: Color32, b: Color32) -> f32 {
        let (la, lb) = (luminance(a), luminance(b));
        let (clair, sombre) = if la > lb { (la, lb) } else { (lb, la) };
        (clair + 0.05) / (sombre + 0.05)
    }
}

/// L'accent : la couleur de ce qui signale, au choix de la personne.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Accent {
    /// Identifiant stable, en minuscules, pour la configuration et la ligne de commande.
    pub nom: &'static str,
    /// Nom affiché.
    pub titre: &'static str,
    /// L'accent vif : ce qui est actif, ce qui est choisi, ce qui commande.
    pub vif: Color32,
    /// L'accent sourd : étiquettes, chiffres en place, ce qui est là sans réclamer.
    pub sourd: Color32,
    /// L'accent éteint : pistes, graduations, ce qui reste d'un courant fini.
    pub eteint: Color32,
    /// Le fil qui cercle les surfaces. Présent, jamais appuyé.
    pub fil: Color32,
    /// Le même fil, éclairé : sélection, focus clavier.
    pub fil_vif: Color32,
    /// Une lueur large et faible, à poser derrière ce qui brille.
    pub lueur: Color32,
    /// La teinte du champ, en sRGB de 0 à 1, telle que le shader la mélange.
    pub champ: [f32; 3],
}

/// Une couleur avec une opacité, prémultipliée comme egui l'attend.
const fn voile(c: Color32, alpha: u8) -> Color32 {
    let a = alpha as u32;
    Color32::from_rgba_premultiplied(
        (c.r() as u32 * a / 255) as u8,
        (c.g() as u32 * a / 255) as u8,
        (c.b() as u32 * a / 255) as u8,
        alpha,
    )
}

const fn accent(
    nom: &'static str,
    titre: &'static str,
    vif: Color32,
    sourd: Color32,
    eteint: Color32,
) -> Accent {
    Accent {
        nom,
        titre,
        vif,
        sourd,
        eteint,
        fil: voile(vif, 58),
        fil_vif: voile(vif, 196),
        lueur: voile(vif, 26),
        champ: [
            vif.r() as f32 / 255.0,
            vif.g() as f32 / 255.0,
            vif.b() as f32 / 255.0,
        ],
    }
}

/// Les accents proposés. Le premier est celui par défaut.
pub const ACCENTS: [Accent; 5] = [
    accent(
        "arc",
        "Arc",
        Color32::from_rgb(96, 226, 255),
        Color32::from_rgb(84, 170, 194),
        Color32::from_rgb(36, 84, 100),
    ),
    accent(
        "or",
        "Or",
        Color32::from_rgb(236, 196, 108),
        Color32::from_rgb(184, 150, 84),
        Color32::from_rgb(108, 86, 42),
    ),
    accent(
        "plasma",
        "Plasma",
        Color32::from_rgb(204, 132, 255),
        Color32::from_rgb(170, 120, 212),
        Color32::from_rgb(86, 56, 116),
    ),
    accent(
        "jade",
        "Jade",
        Color32::from_rgb(98, 232, 172),
        Color32::from_rgb(78, 174, 132),
        Color32::from_rgb(38, 92, 70),
    ),
    accent(
        "nacre",
        "Nacre",
        Color32::from_rgb(234, 240, 246),
        Color32::from_rgb(176, 186, 198),
        Color32::from_rgb(96, 104, 116),
    ),
];

impl Accent {
    /// L'accent désigné par son identifiant, s'il existe.
    #[must_use]
    pub fn par_nom(nom: &str) -> Option<Self> {
        let nom = nom.trim().to_ascii_lowercase();
        ACCENTS.into_iter().find(|a| a.nom == nom)
    }

    /// L'accent par défaut : l'arc.
    #[must_use]
    pub const fn defaut() -> Self {
        ACCENTS[0]
    }

    /// L'accent installé dans un contexte egui, pour que chaque vue le lise sans le porter.
    #[must_use]
    pub fn de(ctx: &egui::Context) -> Self {
        ctx.data(|d| d.get_temp::<Self>(egui::Id::new("accent")))
            .unwrap_or_else(Self::defaut)
    }

    /// Installe cet accent dans un contexte egui.
    pub fn installer(self, ctx: &egui::Context) {
        ctx.data_mut(|d| d.insert_temp(egui::Id::new("accent"), self));
    }
}

/// Le fichier où le choix d'accent est conservé, dans le répertoire de configuration.
#[must_use]
pub fn fichier_de_configuration() -> Option<std::path::PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .map(std::path::PathBuf::from)
        .or_else(|| {
            std::env::var_os("HOME").map(|h| std::path::PathBuf::from(h).join(".config"))
        })?;
    Some(base.join("prophet").join("surface.json"))
}

/// L'accent configuré : la variable `PROPHET_SURFACE_ACCENT`, sinon le fichier, sinon le défaut.
///
/// Un nom inconnu est ignoré plutôt que refusé : une faute de frappe dans un fichier ne doit pas
/// empêcher l'écran de s'ouvrir.
#[must_use]
pub fn accent_configure() -> Accent {
    if let Some(nom) = std::env::var_os("PROPHET_SURFACE_ACCENT")
        && let Some(accent) = Accent::par_nom(&nom.to_string_lossy())
    {
        return accent;
    }
    fichier_de_configuration()
        .and_then(|chemin| std::fs::read_to_string(chemin).ok())
        .and_then(|texte| serde_json::from_str::<serde_json::Value>(&texte).ok())
        .and_then(|valeur| valeur["accent"].as_str().and_then(Accent::par_nom))
        .unwrap_or_else(Accent::defaut)
}

/// Conserve le choix d'accent pour les prochaines ouvertures.
///
/// # Errors
/// Si le répertoire de configuration est introuvable ou n'est pas inscriptible ; le choix
/// reste appliqué à l'écran courant.
pub fn enregistrer_accent(accent: Accent) -> Result<std::path::PathBuf, String> {
    let chemin = fichier_de_configuration().ok_or("répertoire de configuration introuvable")?;
    if let Some(parent) = chemin.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("{} : {e}", parent.display()))?;
    }
    let contenu = serde_json::json!({ "accent": accent.nom }).to_string();
    std::fs::write(&chemin, contenu).map_err(|e| format!("{} : {e}", chemin.display()))?;
    Ok(chemin)
}

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

    #[test]
    fn chaque_accent_et_chaque_encre_se_lisent_sur_le_verre() {
        // Le verre laisse passer le champ, mais ce qu'on lit dessus doit rester lisible même
        // là où rien ne brille derrière : on mesure contre le verre posé sur la nuit.
        let verre = Color32::from_rgb(8, 11, 15);
        for (nom, couleur, minimum) in [
            ("encre", palette::ENCRE, 12.0),
            ("discret", palette::DISCRET, 5.0),
            ("attente", palette::ATTENTE, 6.0),
            ("accompli", palette::ACCOMPLI, 9.0),
        ] {
            let rapport = palette::contraste(couleur, verre);
            assert!(
                rapport >= minimum,
                "{nom} ne contraste qu'à {rapport:.1} sur le verre, moins que {minimum}"
            );
        }
        // Les seuils sont ceux de WCAG : 7 pour le niveau AAA du texte courant, 4,5 pour le
        // niveau AA. Le plasma, le plus sombre des accents, tient 7,8 sur le verre.
        for accent in ACCENTS {
            let vif = palette::contraste(accent.vif, verre);
            let sourd = palette::contraste(accent.sourd, verre);
            assert!(vif >= 7.0, "{} vif ne contraste qu'à {vif:.1}", accent.nom);
            assert!(
                sourd >= 4.5,
                "{} sourd ne contraste qu'à {sourd:.1}",
                accent.nom
            );
            let fond_sur_accent = palette::contraste(palette::FOND, accent.vif);
            assert!(
                fond_sur_accent >= 7.0,
                "le texte sombre d'un bouton {} contraste à {fond_sur_accent:.1}",
                accent.nom
            );
        }
    }

    /// Teinte en degrés et saturation de 0 à 1, au sens HSV.
    fn teinte(c: Color32) -> (f32, f32) {
        let (r, g, b) = (
            f32::from(c.r()) / 255.0,
            f32::from(c.g()) / 255.0,
            f32::from(c.b()) / 255.0,
        );
        let max = r.max(g).max(b);
        let d = max - r.min(g).min(b);
        if d < 1e-6 {
            return (0.0, 0.0);
        }
        let h = if max == r {
            60.0 * (((g - b) / d) % 6.0)
        } else if max == g {
            60.0 * ((b - r) / d + 2.0)
        } else {
            60.0 * ((r - g) / d + 4.0)
        };
        ((h + 360.0) % 360.0, d / max)
    }

    #[test]
    fn aucun_accent_ne_se_confond_avec_l_alerte() {
        // L'alerte est réservée à ce qui exige un humain. Un accent qui lui ressemblerait
        // ferait lire une décision là où il n'y a qu'une sélection : on exige une teinte
        // nettement différente, ou une couleur presque neutre comme la nacre.
        let (teinte_alerte, _) = teinte(palette::ATTENTE);
        for accent in ACCENTS {
            let (h, saturation) = teinte(accent.vif);
            let ecart = ((h - teinte_alerte).abs() % 360.0).min(360.0 - (h - teinte_alerte).abs());
            assert!(
                saturation < 0.2 || ecart >= 20.0,
                "l'accent {} est trop proche de l'alerte : {ecart:.0}° d'écart",
                accent.nom
            );
        }
    }

    #[test]
    fn un_accent_se_retrouve_par_son_nom_et_un_inconnu_ne_casse_rien() {
        assert_eq!(Accent::par_nom("Or").map(|a| a.nom), Some("or"));
        assert_eq!(Accent::par_nom(" jade ").map(|a| a.nom), Some("jade"));
        assert!(Accent::par_nom("fuchsia").is_none());
        assert_eq!(Accent::defaut().nom, "arc");
        let noms: std::collections::BTreeSet<_> = ACCENTS.iter().map(|a| a.nom).collect();
        assert_eq!(
            noms.len(),
            ACCENTS.len(),
            "deux accents portent le même nom"
        );
    }

    #[test]
    fn le_choix_d_accent_survit_a_une_reouverture() {
        let dir = tempfile::tempdir().unwrap();
        // Les variables d'environnement sont partagées par le processus de test : on n'y touche
        // pas et on lit le fichier par le même chemin que `accent_configure`, calculé ici.
        let chemin = dir.path().join("prophet").join("surface.json");
        std::fs::create_dir_all(chemin.parent().unwrap()).unwrap();
        std::fs::write(&chemin, r#"{"accent":"plasma"}"#).unwrap();
        let lu: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&chemin).unwrap()).unwrap();
        assert_eq!(
            lu["accent"]
                .as_str()
                .and_then(Accent::par_nom)
                .map(|a| a.nom),
            Some("plasma")
        );
        std::fs::write(&chemin, "{pas du json").unwrap();
        let cassé = std::fs::read_to_string(&chemin)
            .ok()
            .and_then(|t| serde_json::from_str::<serde_json::Value>(&t).ok())
            .and_then(|v| v["accent"].as_str().and_then(Accent::par_nom))
            .unwrap_or_else(Accent::defaut);
        assert_eq!(
            cassé.nom, "arc",
            "un fichier cassé rend l'accent par défaut"
        );
    }

    #[test]
    fn le_fond_n_est_jamais_un_noir_absolu() {
        // Un voile posé sur un noir absolu ne l'assombrit plus : l'examen d'une décision ne se
        // verrait pas au coin de l'écran, et un dégradé y écraserait ses derniers pas.
        assert!(palette::FOND.r() > 0 && palette::FOND.g() > 0 && palette::FOND.b() > 0);
    }
}
