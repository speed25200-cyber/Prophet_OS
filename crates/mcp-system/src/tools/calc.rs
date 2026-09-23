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
                nombres, + - * /, parenthèses (la virgule décimale est acceptée), par exemple \
                « (120 + 80,5) * 2 ». `numbers` : une liste de nombres dont l'outil rend la \
                somme, le compte, la moyenne, le minimum et le maximum. Ne lit ni n'écrit rien."
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

fn calculer(args: &Value) -> Result<Value, String> {
    match (args.get("expression"), args.get("numbers")) {
        (Some(expression), None) => {
            let texte = expression.as_str().ok_or("expression doit être un texte")?;
            let valeur = evaluer(texte)?;
            Ok(json!({"expression": texte, "value": nombre(valeur)}))
        }
        (None, Some(liste)) => {
            let liste = liste.as_array().ok_or("numbers doit être une liste")?;
            if liste.is_empty() {
                return Err("numbers est vide".into());
            }
            if liste.len() > MAX_NOMBRES {
                return Err(format!("au plus {MAX_NOMBRES} nombres"));
            }
            let valeurs: Vec<f64> = liste
                .iter()
                .map(|v| v.as_f64().ok_or("numbers ne contient que des nombres"))
                .collect::<Result<_, _>>()?;
            let somme: f64 = valeurs.iter().sum();
            let min = valeurs.iter().copied().fold(f64::INFINITY, f64::min);
            let max = valeurs.iter().copied().fold(f64::NEG_INFINITY, f64::max);
            Ok(json!({
                "count": valeurs.len(),
                "sum": nombre(somme),
                "mean": nombre(somme / valeurs.len() as f64),
                "min": nombre(min),
                "max": nombre(max),
            }))
        }
        (Some(_), Some(_)) => Err("donnez expression ou numbers, pas les deux".into()),
        (None, None) => Err("donnez expression ou numbers".into()),
    }
}

/// Évalue une expression arithmétique : nombres, + - * /, parenthèses, signes unaires.
fn evaluer(texte: &str) -> Result<f64, String> {
    if texte.len() > MAX_EXPRESSION {
        return Err(format!("expression de plus de {MAX_EXPRESSION} caractères"));
    }
    let jetons = jetons(texte)?;
    let mut lecteur = Lecteur {
        jetons: &jetons,
        position: 0,
        profondeur: 0,
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
}

fn jetons(texte: &str) -> Result<Vec<Jeton>, String> {
    let mut sortie = Vec::new();
    let caracteres: Vec<char> = texte.chars().collect();
    let mut i = 0;
    while i < caracteres.len() {
        let c = caracteres[i];
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
                sortie.push(Jeton::Ouvrante);
                i += 1;
            }
            ')' => {
                sortie.push(Jeton::Fermante);
                i += 1;
            }
            c if c.is_ascii_digit() || c == '.' || c == ',' => {
                let debut = i;
                while i < caracteres.len()
                    && (caracteres[i].is_ascii_digit() || matches!(caracteres[i], '.' | ','))
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
            })
            .collect::<Vec<_>>()
            .join(" ")
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
            Some(Jeton::Ouvrante) => {
                self.profondeur += 1;
                if self.profondeur > MAX_PROFONDEUR {
                    return Err(format!("plus de {MAX_PROFONDEUR} parenthèses imbriquées"));
                }
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
        assert!(calculer(&json!({"expression": "1", "numbers": [1]})).is_err());
        assert!(calculer(&json!({})).is_err());
    }
}
