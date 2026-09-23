//! Calcul exact pour le modèle (ADR 0053).
//!
//! Un petit modèle recopie bien des nombres et les additionne mal : au banc M13, « le total de la
//! colonne montant » valait 425 et le modèle a écrit 325, 330, 3 305 et 33 450. L'outil calcule
//! à sa place : une expression arithmétique, ou une liste de nombres dont il rend la somme, le
//! compte, la moyenne, le minimum et le maximum. Il ne touche à rien : aucun fichier, aucun
//! réseau, aucun état.

use serde_json::{Value, json};

use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{Tool, ToolContext};

/// Ce que l'outil répond à une liste vide : au banc, le modèle l'appelle avant d'avoir lu le
/// fichier, comme si l'outil connaissait la colonne ; il faut lui dire où sont les nombres.
const VIDE: &str = "numbers est vide : calc.eval ne lit aucun fichier. Lisez d'abord le fichier \
    avec fs.read, puis passez ses nombres, par exemple numbers: [100, 125, 200]";

/// Longueur maximale d'une expression.
const MAX_EXPRESSION: usize = 4096;
/// Profondeur maximale des parenthèses.
const MAX_PROFONDEUR: usize = 64;
/// Nombres au plus dans une liste.
const MAX_NOMBRES: usize = 10_000;

/// Calcul exact : une expression, ou une liste de nombres.
#[derive(Debug)]
pub struct Calc;

impl Tool for Calc {
    fn spec(&self) -> ToolSpec {
        ToolSpec {
            name: "calc.eval".into(),
            description: "Calcule exactement, au lieu de compter de tête. `expression` : \
                nombres, + - * /, parenthèses (virgule décimale acceptée) et les fonctions \
                sum, min, max, mean, count, par exemple « (120 + 80,5) * 2 » ou \
                « sum(100, 125, 200) ». `numbers` : une liste de nombres dont l'outil rend la \
                somme, le compte, la moyenne, le minimum et le maximum ; une expression peut la \
                nommer, « sum(numbers) ». Ne lit ni n'écrit rien."
                .into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "expression": {"type": "string"},
                    "numbers": {"type": "array", "items": {"type": "number"}}
                },
                "additionalProperties": false
            }),
            meta: Some(ToolMeta {
                requires: "tool.call".into(),
                irreversible: false,
                external: false,
                sandbox_level_min: None,
            }),
        }
    }

    fn target(&self, _args: &Value, _context: &ToolContext) -> Option<String> {
        None
    }

    fn call(&self, args: &Value, _context: &ToolContext) -> CallResult {
        match calculer(args) {
            Ok(value) => CallResult::structured(value),
            Err(detail) => CallResult::error(ErrorCode::Invalid, detail),
        }
    }
}

/// Un nombre rendu au modèle : entier quand il l'est, sinon arrondi à douze chiffres
/// significatifs, pour ne pas lui montrer les artefacts du flottant (0,1 + 0,2).
fn nombre(x: f64) -> Value {
    if x.fract() == 0.0 && x.abs() < 1e15 {
        return json!(x as i64);
    }
    let magnitude = x.abs().log10().floor() as i32;
    let facteur = 10f64.powi(11 - magnitude);
    json!((x * facteur).round() / facteur)
}

/// Les agrégats d'une liste de nombres.
fn agregats(valeurs: &[f64]) -> Value {
    let somme: f64 = valeurs.iter().sum();
    let min = valeurs.iter().copied().fold(f64::INFINITY, f64::min);
    let max = valeurs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
    json!({
        "count": valeurs.len(),
        "sum": nombre(somme),
        "mean": nombre(somme / valeurs.len() as f64),
        "min": nombre(min),
        "max": nombre(max),
    })
}

/// Le calcul demandé. Les deux paramètres se combinent : une expression peut nommer la liste
/// (`sum(numbers)`), et une liste sans expression rend ses agrégats. Un modèle passe souvent
/// les deux, ou une liste vide à côté d'une expression qui les porte : l'outil fait ce qui a
/// un sens, et dit ce qui n'en a pas.
fn calculer(args: &Value) -> Result<Value, String> {
    let liste: Vec<f64> = match args.get("numbers") {
        None | Some(Value::Null) => Vec::new(),
        Some(valeur) => {
            let liste = valeur.as_array().ok_or("numbers doit être une liste")?;
            if liste.len() > MAX_NOMBRES {
                return Err(format!("au plus {MAX_NOMBRES} nombres"));
            }
            liste
                .iter()
                .map(|v| v.as_f64().ok_or("numbers ne contient que des nombres"))
                .collect::<Result<_, _>>()?
        }
    };
    let expression = match args.get("expression") {
        None | Some(Value::Null) => None,
        Some(valeur) => Some(valeur.as_str().ok_or("expression doit être un texte")?)
            .filter(|texte| !texte.trim().is_empty()),
    };
    match expression {
        Some(texte) => {
            let valeur = evaluer(texte, &liste)?;
            let mut resultat = json!({"expression": texte, "value": nombre(valeur)});
            if !liste.is_empty()
                && let (Some(objet), Value::Object(agregats)) =
                    (resultat.as_object_mut(), agregats(&liste))
            {
                objet.extend(agregats);
            }
            Ok(resultat)
        }
        None if !liste.is_empty() => Ok(agregats(&liste)),
        None if args.get("numbers").is_some() => Err(VIDE.into()),
        None => Err("donnez expression ou numbers".into()),
    }
}

/// Évalue une expression arithmétique : nombres, + - * /, parenthèses, signes unaires, et les
/// fonctions `sum`, `min`, `max`, `mean` (`avg`) et `count` sur des arguments ou sur `numbers`.
fn evaluer(texte: &str, liste: &[f64]) -> Result<f64, String> {
    if texte.len() > MAX_EXPRESSION {
        return Err(format!("expression de plus de {MAX_EXPRESSION} caractères"));
    }
    let jetons = jetons(texte)?;
    let mut lecteur = Lecteur {
        jetons: &jetons,
        position: 0,
        profondeur: 0,
        liste,
    };
    let valeur = lecteur.somme()?;
    if lecteur.position != jetons.len() {
        return Err(format!(
            "expression incomplète ou mal formée près de « {} »",
            lecteur.reste()
        ));
    }
    if !valeur.is_finite() {
        return Err("résultat non fini".into());
    }
    Ok(valeur)
}

#[derive(Debug, Clone, PartialEq)]
enum Jeton {
    Nombre(f64),
    Operateur(char),
    Ouvrante,
    Fermante,
    Separateur,
    Nom(String),
}

/// Découpe l'expression. Dans les arguments d'une fonction, la virgule sépare
/// (`sum(1200, 4800)`) ; ailleurs, entre deux chiffres, elle est décimale (`80,5`).
fn jetons(texte: &str) -> Result<Vec<Jeton>, String> {
    let mut sortie = Vec::new();
    let caracteres: Vec<char> = texte.chars().collect();
    // Pour chaque parenthèse ouverte : vrai si elle ouvre les arguments d'une fonction.
    let mut ouvertes: Vec<bool> = Vec::new();
    let mut i = 0;
    while i < caracteres.len() {
        let c = caracteres[i];
        let dans_fonction = ouvertes.last().copied().unwrap_or(false);
        match c {
            c if c.is_whitespace() => i += 1,
            '+' | '-' | '*' | '/' | '×' | '÷' => {
                sortie.push(Jeton::Operateur(match c {
                    '×' => '*',
                    '÷' => '/',
                    autre => autre,
                }));
                i += 1;
            }
            '(' => {
                ouvertes.push(matches!(sortie.last(), Some(Jeton::Nom(_))));
                sortie.push(Jeton::Ouvrante);
                i += 1;
            }
            ')' => {
                ouvertes.pop();
                sortie.push(Jeton::Fermante);
                i += 1;
            }
            ',' | ';' if dans_fonction => {
                sortie.push(Jeton::Separateur);
                i += 1;
            }
            c if c.is_ascii_alphabetic() || c == '_' => {
                let debut = i;
                while i < caracteres.len()
                    && (caracteres[i].is_ascii_alphanumeric() || caracteres[i] == '_')
                {
                    i += 1;
                }
                sortie.push(Jeton::Nom(
                    caracteres[debut..i]
                        .iter()
                        .collect::<String>()
                        .to_lowercase(),
                ));
            }
            c if c.is_ascii_digit() || c == '.' || c == ',' => {
                let debut = i;
                while i < caracteres.len()
                    && (caracteres[i].is_ascii_digit()
                        || caracteres[i] == '.'
                        || (caracteres[i] == ',' && !dans_fonction))
                {
                    i += 1;
                }
                let brut: String = caracteres[debut..i].iter().collect();
                let normalise = brut.replace(',', ".");
                if normalise.matches('.').count() > 1 {
                    return Err(format!("nombre ambigu « {brut} »"));
                }
                let valeur = normalise
                    .parse::<f64>()
                    .map_err(|_| format!("nombre illisible « {brut} »"))?;
                sortie.push(Jeton::Nombre(valeur));
            }
            autre => return Err(format!("caractère inattendu « {autre} »")),
        }
    }
    if sortie.is_empty() {
        return Err("expression vide".into());
    }
    Ok(sortie)
}

struct Lecteur<'a> {
    jetons: &'a [Jeton],
    position: usize,
    profondeur: usize,
    liste: &'a [f64],
}

impl Lecteur<'_> {
    fn suivant(&self) -> Option<&Jeton> {
        self.jetons.get(self.position)
    }

    fn reste(&self) -> String {
        self.jetons[self.position.min(self.jetons.len())..]
            .iter()
            .take(4)
            .map(|j| match j {
                Jeton::Nombre(n) => n.to_string(),
                Jeton::Operateur(o) => o.to_string(),
                Jeton::Ouvrante => "(".into(),
                Jeton::Fermante => ")".into(),
                Jeton::Separateur => ",".into(),
                Jeton::Nom(nom) => nom.clone(),
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    fn entrer(&mut self) -> Result<(), String> {
        self.profondeur += 1;
        if self.profondeur > MAX_PROFONDEUR {
            return Err(format!("plus de {MAX_PROFONDEUR} parenthèses imbriquées"));
        }
        Ok(())
    }

    fn somme(&mut self) -> Result<f64, String> {
        let mut valeur = self.produit()?;
        while let Some(Jeton::Operateur(o @ ('+' | '-'))) = self.suivant().cloned() {
            self.position += 1;
            let droite = self.produit()?;
            valeur = if o == '+' {
                valeur + droite
            } else {
                valeur - droite
            };
        }
        Ok(valeur)
    }

    fn produit(&mut self) -> Result<f64, String> {
        let mut valeur = self.facteur()?;
        while let Some(Jeton::Operateur(o @ ('*' | '/'))) = self.suivant().cloned() {
            self.position += 1;
            let droite = self.facteur()?;
            if o == '/' {
                if droite == 0.0 {
                    return Err("division par zéro".into());
                }
                valeur /= droite;
            } else {
                valeur *= droite;
            }
        }
        Ok(valeur)
    }

    /// Les arguments d'une fonction : des expressions, ou `numbers`, qui vaut la liste passée.
    fn arguments(&mut self) -> Result<Vec<f64>, String> {
        let mut valeurs = Vec::new();
        if self.suivant() == Some(&Jeton::Fermante) {
            return Ok(valeurs);
        }
        loop {
            if let Some(Jeton::Nom(nom)) = self.suivant().cloned()
                && nom == "numbers"
                && matches!(
                    self.jetons.get(self.position + 1),
                    Some(Jeton::Separateur | Jeton::Fermante)
                )
            {
                if self.liste.is_empty() {
                    return Err(VIDE.into());
                }
                self.position += 1;
                valeurs.extend_from_slice(self.liste);
            } else {
                valeurs.push(self.somme()?);
            }
            match self.suivant() {
                Some(Jeton::Separateur) => self.position += 1,
                _ => return Ok(valeurs),
            }
        }
    }

    fn fonction(&mut self, nom: &str) -> Result<f64, String> {
        // Le nom est lu ; la parenthèse ouvrante suit.
        self.entrer()?;
        self.position += 1;
        let valeurs = self.arguments()?;
        if self.suivant() != Some(&Jeton::Fermante) {
            return Err("parenthèse non fermée".into());
        }
        self.position += 1;
        self.profondeur -= 1;
        if valeurs.is_empty() && nom != "count" {
            return Err(format!("{nom}() sans nombre"));
        }
        let somme: f64 = valeurs.iter().sum();
        match nom {
            "sum" | "somme" | "total" => Ok(somme),
            "min" => Ok(valeurs.iter().copied().fold(f64::INFINITY, f64::min)),
            "max" => Ok(valeurs.iter().copied().fold(f64::NEG_INFINITY, f64::max)),
            "mean" | "avg" | "average" | "moyenne" => Ok(somme / valeurs.len() as f64),
            "count" | "len" => Ok(valeurs.len() as f64),
            autre => Err(format!(
                "fonction inconnue « {autre} » : sum, min, max, mean, count"
            )),
        }
    }

    fn facteur(&mut self) -> Result<f64, String> {
        match self.suivant().cloned() {
            Some(Jeton::Operateur('-')) => {
                self.position += 1;
                Ok(-self.facteur()?)
            }
            Some(Jeton::Operateur('+')) => {
                self.position += 1;
                self.facteur()
            }
            Some(Jeton::Nombre(n)) => {
                self.position += 1;
                Ok(n)
            }
            Some(Jeton::Nom(nom)) => {
                self.position += 1;
                if self.suivant() == Some(&Jeton::Ouvrante) {
                    self.fonction(&nom)
                } else if nom == "numbers" {
                    Err("numbers s'emploie dans une fonction, par exemple sum(numbers)".into())
                } else {
                    Err(format!("nom inconnu « {nom} »"))
                }
            }
            Some(Jeton::Ouvrante) => {
                self.entrer()?;
                self.position += 1;
                let valeur = self.somme()?;
                if self.suivant() != Some(&Jeton::Fermante) {
                    return Err("parenthèse non fermée".into());
                }
                self.position += 1;
                self.profondeur -= 1;
                Ok(valeur)
            }
            _ => Err(format!("nombre attendu près de « {} »", self.reste())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn expression(texte: &str) -> Result<Value, String> {
        calculer(&json!({"expression": texte}))
    }

    #[test]
    fn une_expression_se_calcule_avec_les_priorites() {
        assert_eq!(expression("100 + 125 + 200").unwrap()["value"], 425);
        assert_eq!(expression("(120 + 80,5) * 2").unwrap()["value"], 401);
        assert_eq!(expression("2 + 3 * 4").unwrap()["value"], 14);
        assert_eq!(expression("-(2 - 5) / 2").unwrap()["value"], 1.5);
        assert_eq!(expression("7 × 6 ÷ 3").unwrap()["value"], 14);
        assert_eq!(expression("0.1 + 0.2").unwrap()["value"], 0.3);
    }

    #[test]
    fn une_expression_mal_formee_est_dite() {
        for mauvaise in [
            "",
            "2 +",
            "2 3",
            "(1 + 2",
            "1,2,3 + 1",
            "4 / 0",
            "rm -rf /",
            "1e9999",
        ] {
            assert!(expression(mauvaise).is_err(), "{mauvaise}");
        }
        let profonde = format!("{}1{}", "(".repeat(80), ")".repeat(80));
        assert!(expression(&profonde).unwrap_err().contains("imbriquées"));
    }

    #[test]
    fn une_liste_rend_somme_compte_moyenne_et_extremes() {
        let r = calculer(&json!({"numbers": [100, 125, 200]})).unwrap();
        assert_eq!(r["sum"], 425);
        assert_eq!(r["count"], 3);
        assert_eq!(r["min"], 100);
        assert_eq!(r["max"], 200);
        assert_eq!(r["mean"], 141.666666667);
        assert!(calculer(&json!({"numbers": []})).is_err());
        assert!(calculer(&json!({"numbers": ["1"]})).is_err());
        assert!(calculer(&json!({})).is_err());
    }

    #[test]
    fn les_formes_qu_un_modele_emploie_sont_comprises() {
        // Relevées au banc : une fonction, les deux paramètres ensemble, la liste nommée.
        assert_eq!(
            expression("sum(1200, 4800, 950, 3100)").unwrap()["value"],
            10050
        );
        let r = calculer(&json!({"expression": "sum(1200, 4800, 950, 3100)", "numbers": [1200, 4800, 950, 3100]}))
            .unwrap();
        assert_eq!(r["value"], 10050);
        assert_eq!(r["sum"], 10050);
        let r = calculer(
            &json!({"expression": "sum(numbers) / count(numbers)", "numbers": [100, 125, 200]}),
        )
        .unwrap();
        assert_eq!(r["value"], 141.666666667);
        assert_eq!(expression("max(3, 9,5 - 1)").unwrap()["value"], 9);
        assert_eq!(expression("mean(2; 4)").unwrap()["value"], 3);
        assert_eq!(expression("(120 + 80,5) * 2").unwrap()["value"], 401);
        // Une liste vide à côté d'une expression qui la porte : l'erreur dit quoi faire.
        let vide = calculer(&json!({"expression": "sum(numbers)", "numbers": []})).unwrap_err();
        assert!(vide.contains("fs.read"), "{vide}");
        assert!(
            expression("numbers + 1")
                .unwrap_err()
                .contains("sum(numbers)")
        );
        assert!(expression("median(1, 2)").unwrap_err().contains("inconnue"));
        // Une expression vide et une liste : les agrégats.
        let r = calculer(&json!({"expression": "", "numbers": [1, 2]})).unwrap();
        assert_eq!(r["sum"], 3);
    }
}
