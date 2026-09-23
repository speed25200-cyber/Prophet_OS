//! Les échecs d'outil qui se répètent (ADR 0052).
//!
//! Un petit modèle qui reçoit une erreur recommence souvent l'appel à l'identique : au banc M13,
//! jusqu'à vingt-quatre `fs.edit` refusés de suite, soixante-huit mille tokens pour rien. La
//! garde enveloppe l'exécuteur de la mission : dès le deuxième échec identique de suite (même
//! outil, même code), le résultat porte `repeated` et une note qui demande de changer
//! d'approche ; la boucle de mission, elle, s'arrête au [`ECHECS_MAX`]ᵉ appel échoué de suite.
//! La garde ne change ni les droits ni les résultats réussis.
use std::sync::Mutex;

use providers::native::ToolExecutor;
use serde_json::{Value, json};

/// Appels d'outil échoués de suite qui arrêtent une mission.
pub const ECHECS_MAX: u32 = 5;

/// L'exécuteur d'une mission, vu à travers ses échecs répétés.
pub struct Garde {
    inner: Box<dyn ToolExecutor>,
    /// Le dernier échec (outil, code) et combien de fois de suite il s'est produit.
    dernier: Mutex<(Option<(String, String)>, u32)>,
}

impl Garde {
    /// Enveloppe l'exécuteur d'une mission.
    #[must_use]
    pub fn new(inner: Box<dyn ToolExecutor>) -> Self {
        Self {
            inner,
            dernier: Mutex::new((None, 0)),
        }
    }
}

/// Le code d'erreur d'un résultat d'outil, tel que le registre le rend.
fn code(result: &Value) -> String {
    result
        .get("code")
        .and_then(Value::as_str)
        .unwrap_or("Erreur")
        .to_owned()
}

impl ToolExecutor for Garde {
    fn call(&self, tool: &str, arguments: &Value) -> (bool, Value) {
        let (ok, mut result) = self.inner.call(tool, arguments);
        let Ok(mut dernier) = self.dernier.lock() else {
            return (ok, result);
        };
        if ok {
            *dernier = (None, 0);
            return (ok, result);
        }
        let echec = (tool.to_owned(), code(&result));
        let fois = if dernier.0.as_ref() == Some(&echec) {
            dernier.1.saturating_add(1)
        } else {
            1
        };
        *dernier = (Some(echec.clone()), fois);
        if fois >= 2
            && let Some(objet) = result.as_object_mut()
        {
            objet.insert("repeated".into(), json!(fois));
            objet.insert(
                "note".into(),
                json!(format!(
                    "{tool} a échoué {fois} fois de suite de la même façon ({}) : ne le rappelez \
                     pas à l'identique. Relisez l'erreur et changez d'approche — un autre outil, \
                     un autre chemin, ou concluez en disant ce qui bloque.",
                    echec.1
                )),
            );
        }
        (ok, result)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use super::*;

    struct Script(Mutex<VecDeque<(bool, Value)>>);
    impl ToolExecutor for Script {
        fn call(&self, _: &str, _: &Value) -> (bool, Value) {
            self.0
                .lock()
                .expect("script")
                .pop_front()
                .expect("appel prévu")
        }
    }

    fn garde(resultats: Vec<(bool, Value)>) -> Garde {
        Garde::new(Box::new(Script(Mutex::new(resultats.into()))))
    }

    fn introuvable() -> (bool, Value) {
        (false, json!({"code": "NotFound", "detail": "absent"}))
    }

    #[test]
    fn un_echec_repete_porte_une_note_des_la_deuxieme_fois() {
        let garde = garde(vec![introuvable(), introuvable(), introuvable()]);
        let (_, premier) = garde.call("fs.edit", &json!({}));
        assert!(premier.get("note").is_none(), "{premier}");
        let (ok, deuxieme) = garde.call("fs.edit", &json!({}));
        assert!(!ok);
        assert_eq!(deuxieme["repeated"], 2);
        assert!(
            deuxieme["note"]
                .as_str()
                .is_some_and(|n| n.contains("changez d'approche")),
            "{deuxieme}"
        );
        assert_eq!(deuxieme["code"], "NotFound", "le résultat garde son code");
        let (_, troisieme) = garde.call("fs.edit", &json!({}));
        assert_eq!(troisieme["repeated"], 3);
    }

    #[test]
    fn un_succes_ou_un_autre_echec_remet_le_compte_a_zero() {
        let garde = garde(vec![
            introuvable(),
            (true, json!({"ok": true})),
            introuvable(),
            (false, json!({"code": "Invalid"})),
            introuvable(),
        ]);
        for _ in 0..5 {
            let (_, resultat) = garde.call("fs.edit", &json!({}));
            assert!(resultat.get("repeated").is_none(), "{resultat}");
        }
    }

    #[test]
    fn le_meme_code_sur_un_autre_outil_n_est_pas_une_repetition() {
        let garde = garde(vec![introuvable(), introuvable()]);
        garde.call("fs.edit", &json!({}));
        let (_, resultat) = garde.call("fs.read", &json!({}));
        assert!(resultat.get("repeated").is_none(), "{resultat}");
    }
}
