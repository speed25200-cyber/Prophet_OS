//! Le réseau d'un client officiel en mission (ADR 0056) : les hôtes de son éditeur, et le jeton
//! de capd qui borne sa sortie à ces seuls hôtes.
//!
//! Le client tourne dans une cage sans réseau ; son seul proxy est un relais vers egress, qui
//! demande à capd, hôte par hôte, si la mission peut sortir. Le jeton émis ici ne porte que des
//! droits `net.egress` sur les hôtes du client — ni fichier, ni outil, ni approbation — et sa
//! mission pour sujet : ce qui sort s'inscrit au journal de la mission, et révoquer la mission
//! coupe aussi son réseau.

use std::collections::BTreeMap;

use base64::Engine as _;
use prophet_types::cap::{Act, Grant, Res, Token};
use prophet_types::manifest::Manifest;
use providers::official::ClientProfile;

/// Variable d'environnement : hôtes supplémentaires par client, en JSON
/// (`{"claude-code": ["hote.exemple"]}`), pour compléter la liste d'un éditeur sans reconstruire.
pub const HOTES_ENV: &str = "PROPHET_CLIENT_HOSTS";

/// Marge de durée du jeton au-delà de la durée de la mission.
pub const MARGE_SECONDES: i64 = 300;

/// Lit les hôtes supplémentaires.
///
/// # Errors
/// JSON illisible.
pub fn supplement(json: Option<&str>) -> Result<BTreeMap<String, Vec<String>>, String> {
    json.map_or_else(
        || Ok(BTreeMap::new()),
        |texte| serde_json::from_str(texte).map_err(|e| format!("{HOTES_ENV} illisible : {e}")),
    )
}

/// Les hôtes qu'un client peut joindre : ceux de son éditeur, puis ceux que l'administrateur
/// ajoute, sans doublon.
#[must_use]
pub fn hotes(client: &str, supplement: &BTreeMap<String, Vec<String>>) -> Vec<String> {
    let mut hotes: Vec<String> = ClientProfile::all()
        .into_iter()
        .find(|p| p.driver == client)
        .map(|p| p.hosts)
        .unwrap_or_default();
    for hote in supplement.get(client).into_iter().flatten() {
        if !hotes.contains(hote) {
            hotes.push(hote.clone());
        }
    }
    hotes
}

/// Le manifeste de la sortie d'un client : `net.egress` vers ses hôtes, rien d'autre.
///
/// # Errors
/// Manifeste invalide (hôte refusé par le schéma des manifestes).
pub fn manifeste(client: &str, hotes: &[String]) -> Result<Manifest, String> {
    let liste = hotes
        .iter()
        .map(|h| serde_json::to_string(h).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(", ");
    let texte = format!(
        r#"
[agent]
id = "org.prophet.client-reseau"
version = "1.0.0"
name = "Réseau d'un client officiel ({client})"
publisher_key = "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="
[model]
# Le schéma exige un modèle : c'est le client lui-même, qui parle à son éditeur.
preferred = ["driver:{client}"]
[capabilities.max]
"net.egress" = [{liste}]
"#
    );
    let manifeste = Manifest::from_toml(&texte).map_err(|e| e.to_string())?;
    manifeste.validate().map_err(|e| e.to_string())?;
    Ok(manifeste)
}

/// Les droits demandés : un par hôte.
#[must_use]
pub fn grants(hotes: &[String]) -> Vec<Grant> {
    hotes
        .iter()
        .map(|h| Grant::new(Res::Net, Act::Egress, h))
        .collect()
}

/// Le jeton tel que l'en-tête `Proxy-Authorization: Prophet …` le porte vers egress.
///
/// # Errors
/// Jeton impossible à sérialiser.
pub fn en_tete(jeton: &Token) -> Result<String, String> {
    serde_json::to_vec(jeton)
        .map(|json| base64::engine::general_purpose::STANDARD.encode(json))
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_client_joint_les_hotes_de_son_editeur_et_ceux_que_l_administrateur_ajoute() {
        let ajout =
            supplement(Some(r#"{"codex": ["proxy.entreprise.fr", "chatgpt.com"]}"#)).unwrap();
        let codex = hotes("codex", &ajout);
        assert!(codex.contains(&"chatgpt.com".to_owned()));
        assert!(codex.contains(&"proxy.entreprise.fr".to_owned()));
        assert_eq!(
            codex.iter().filter(|h| *h == "chatgpt.com").count(),
            1,
            "sans doublon"
        );
        assert!(hotes("claude-code", &ajout).contains(&"api.anthropic.com".to_owned()));
        assert!(hotes("inconnu", &ajout).is_empty());
        assert!(supplement(Some("pas du json")).is_err());
    }

    #[test]
    fn le_manifeste_ne_porte_que_la_sortie_vers_ces_hotes() {
        let hotes = hotes("claude-code", &BTreeMap::new());
        let manifeste = manifeste("claude-code", &hotes).unwrap();
        let droits = grants(&hotes);
        assert_eq!(droits.len(), hotes.len());
        assert!(
            droits
                .iter()
                .all(|g| g.res == Res::Net && g.act == Act::Egress)
        );
        assert_eq!(manifeste.agent.id, "org.prophet.client-reseau");
    }
}
