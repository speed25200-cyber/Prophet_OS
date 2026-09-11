//! Sérialisation canonique : la seule forme d'octets sur laquelle on signe ou on hache.
//!
//! Règles (voir `docs/specs/capability-token.md`) :
//! - JSON compact, sans espace superflu ;
//! - clés d'objet triées par ordre lexicographique des points de code, récursivement ;
//! - les clés dont le nom figure dans `skip` sont retirées au niveau racine (par exemple `sig`
//!   pour un jeton, `hash` pour un événement) ;
//! - les nombres sont écrits par `serde_json` sous leur forme la plus courte.

use serde::Serialize;
use serde_json::{Map, Value};

/// Erreur de canonicalisation.
#[derive(Debug, thiserror::Error)]
pub enum CanonError {
    /// La valeur n'a pas pu être convertie en JSON.
    #[error("sérialisation JSON impossible : {0}")]
    Json(#[from] serde_json::Error),
    /// La racine n'est pas un objet alors que `skip` est non vide.
    #[error("la racine doit être un objet pour retirer des clés")]
    NotAnObject,
}

/// Trie récursivement les clés d'un `Value`.
fn sort_value(value: Value) -> Value {
    match value {
        Value::Object(map) => {
            let mut keys: Vec<String> = map.keys().cloned().collect();
            keys.sort_unstable();
            let mut out = Map::with_capacity(keys.len());
            let mut map = map;
            for key in keys {
                if let Some(v) = map.remove(&key) {
                    out.insert(key, sort_value(v));
                }
            }
            Value::Object(out)
        }
        Value::Array(items) => Value::Array(items.into_iter().map(sort_value).collect()),
        other => other,
    }
}

/// Produit les octets canoniques d'une valeur sérialisable, en retirant les clés racine `skip`.
///
/// # Erreurs
/// Retourne une erreur si la valeur n'est pas sérialisable en JSON, ou si `skip` est non vide
/// alors que la racine n'est pas un objet.
pub fn to_canonical_bytes<T: Serialize>(value: &T, skip: &[&str]) -> Result<Vec<u8>, CanonError> {
    let mut json = serde_json::to_value(value)?;
    if !skip.is_empty() {
        let Value::Object(map) = &mut json else {
            return Err(CanonError::NotAnObject);
        };
        for key in skip {
            map.remove(*key);
        }
    }
    let sorted = sort_value(json);
    Ok(serde_json::to_vec(&sorted)?)
}

/// Hache une valeur sous sa forme canonique et renvoie `blake3:<hex>`.
///
/// # Erreurs
/// Voir [`to_canonical_bytes`].
pub fn canonical_hash<T: Serialize>(value: &T, skip: &[&str]) -> Result<String, CanonError> {
    let bytes = to_canonical_bytes(value, skip)?;
    Ok(format!("blake3:{}", blake3::hash(&bytes).to_hex()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn trie_les_cles_recursivement() {
        let v = json!({"b": 1, "a": {"z": [ {"y": 1, "x": 2} ], "c": 3}});
        let bytes = to_canonical_bytes(&v, &[]).unwrap();
        assert_eq!(
            String::from_utf8(bytes).unwrap(),
            r#"{"a":{"c":3,"z":[{"x":2,"y":1}]},"b":1}"#
        );
    }

    #[test]
    fn retire_les_cles_racine_demandees() {
        let v = json!({"sig": "zzz", "a": 1});
        let bytes = to_canonical_bytes(&v, &["sig"]).unwrap();
        assert_eq!(String::from_utf8(bytes).unwrap(), r#"{"a":1}"#);
    }

    #[test]
    fn ordre_des_champs_sans_effet_sur_le_hash() {
        let a = json!({"x": 1, "y": {"p": true, "q": null}});
        let b = json!({"y": {"q": null, "p": true}, "x": 1});
        assert_eq!(
            canonical_hash(&a, &[]).unwrap(),
            canonical_hash(&b, &[]).unwrap()
        );
    }

    #[test]
    fn refuse_skip_sur_une_racine_non_objet() {
        let v = json!([1, 2, 3]);
        assert!(matches!(
            to_canonical_bytes(&v, &["sig"]),
            Err(CanonError::NotAnObject)
        ));
    }
}
