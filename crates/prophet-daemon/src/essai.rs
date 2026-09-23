//! De quoi lancer un daemon dans un test et lui parler.
//!
//! Sept daemons ont le même test d'existence : est-ce que le programme démarre, ouvre son socket,
//! et répond ? Ce module porte la seule partie délicate — l'attente.
//!
//! **Attendre que le fichier de socket apparaisse ne prouve rien.** Il apparaît au `bind`, avant
//! que la boucle d'acceptation ne tourne, et un test qui se contente de le voir passe alors que le
//! daemon ne répond pas encore. On attend donc qu'une connexion aboutisse *et* qu'un appel
//! revienne. C'est la règle d'ADR-0006 : essayer, plutôt que constater un indice.

use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::time::{Duration, Instant};

use prophet_ipc::Client;
use serde_json::json;

/// Un daemon lancé pour la durée d'un test, tué à la fin quoi qu'il arrive — y compris si le test
/// échoue par panique, puisque `Drop` s'exécute pendant le déroulement de pile.
#[derive(Debug)]
pub struct Daemon {
    enfant: Child,
    socket: PathBuf,
}

impl Drop for Daemon {
    fn drop(&mut self) {
        let _ = self.enfant.kill();
        let _ = self.enfant.wait();
    }
}

impl Daemon {
    /// Lance un programme de daemon avec son socket et son état.
    ///
    /// `programme` vient de `env!("CARGO_BIN_EXE_<nom>")`, qui ne peut être développé que dans le
    /// crate de test.
    ///
    /// # Panics
    /// Si le programme ne peut pas être lancé : il n'y a alors rien à tester.
    #[must_use]
    pub fn lancer(programme: &str, socket: &Path, etat: &Path) -> Self {
        Self::lancer_avec(programme, socket, etat, &[])
    }

    /// Comme [`Daemon::lancer`], avec des variables d'environnement en plus.
    ///
    /// # Panics
    /// Si le programme ne peut pas être lancé.
    #[must_use]
    pub fn lancer_avec(
        programme: &str,
        socket: &Path,
        etat: &Path,
        environnement: &[(&str, &str)],
    ) -> Self {
        let mut commande = Command::new(programme);
        commande
            .env("PROPHET_SOCKET", socket)
            .env("STATE_DIRECTORY", etat)
            .env("RUST_LOG", "warn");
        for (cle, valeur) in environnement {
            commande.env(cle, valeur);
        }
        let enfant = commande
            .spawn()
            .unwrap_or_else(|e| panic!("{programme} doit pouvoir être lancé : {e}"));
        Self {
            enfant,
            socket: socket.to_path_buf(),
        }
    }

    /// Identifiant du processus, pour lire ce qu'il consomme dans `/proc`.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.enfant.id()
    }

    /// Attend que le daemon *réponde*, et non qu'il paraisse prêt.
    ///
    /// # Panics
    /// Au bout de dix secondes, en disant ce qui a échoué en dernier — sans quoi un test qui
    /// expire n'apprend rien à personne.
    pub async fn joindre(&self) -> Client {
        let limite = Instant::now() + Duration::from_secs(10);
        let mut derniere = "aucune tentative n'a encore eu lieu".to_owned();
        while Instant::now() < limite {
            match Client::connect(&self.socket).await {
                Ok(client) => match client.call("ping", json!({})).await {
                    Ok(_) => return client,
                    Err(e) => derniere = format!("connecté, mais ping a échoué : {}", e.message),
                },
                Err(e) => derniere = format!("connexion impossible : {e}"),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!(
            "{} n'a pas répondu en 10 s — {derniere}",
            self.socket.display()
        );
    }

    /// Attend qu'un daemon qui ne parle **pas** JSON-RPC réponde à une requête brute.
    ///
    /// Le proxy de sortie parle HTTP sur son socket ; `ping` n'y veut rien dire. On lui envoie donc
    /// la sonde donnée, et on attend une réponse — pas la simple existence du socket, qui ne dirait
    /// que « le bind a eu lieu ».
    ///
    /// # Panics
    /// Au bout de dix secondes, en disant ce qui a échoué en dernier.
    pub async fn attendre_reponse(&self, sonde: &[u8]) -> String {
        use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
        use tokio::net::UnixStream;

        let limite = Instant::now() + Duration::from_secs(10);
        let mut derniere = "aucune tentative n'a encore eu lieu".to_owned();
        while Instant::now() < limite {
            match UnixStream::connect(&self.socket).await {
                Ok(mut flux) => match flux.write_all(sonde).await {
                    Ok(()) => {
                        let mut ligne = String::new();
                        match BufReader::new(flux).read_line(&mut ligne).await {
                            Ok(n) if n > 0 => return ligne,
                            Ok(_) => derniere = "connecté, mais aucune réponse".to_owned(),
                            Err(e) => derniere = format!("lecture impossible : {e}"),
                        }
                    }
                    Err(e) => derniere = format!("écriture impossible : {e}"),
                },
                Err(e) => derniere = format!("connexion impossible : {e}"),
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
        panic!(
            "{} n'a pas répondu en 10 s — {derniere}",
            self.socket.display()
        );
    }
}

/// Le chemin d'un binaire de l'atelier depuis un test d'un **autre** crate.
///
/// `CARGO_BIN_EXE_<nom>` n'est défini que pour les binaires du paquet qui contient le test. Un test
/// qui fait dialoguer deux daemons — le proxy de sortie et le broker, par exemple — doit donc
/// trouver l'autre autrement. Le binaire de test vit dans `target/<profil>/deps/` ; ses voisins
/// sont un cran au-dessus.
///
/// # Panics
/// Si le binaire est introuvable, avec le chemin cherché. Un test qui continuerait sans lui
/// mesurerait autre chose que ce qu'il croit.
#[must_use]
pub fn binaire_voisin(nom: &str) -> PathBuf {
    let exe = std::env::current_exe().expect("le test a bien un chemin");
    let chemin = exe
        .parent()
        .and_then(std::path::Path::parent)
        .map(|repertoire| repertoire.join(nom))
        .unwrap_or_default();
    assert!(
        chemin.is_file(),
        "binaire « {nom} » introuvable en {} — construisez-le avant \
         (`cargo build --bin {nom}`), plutôt que de tester sans lui",
        chemin.display()
    );
    chemin
}
