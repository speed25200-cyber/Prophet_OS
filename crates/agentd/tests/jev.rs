//! Jev dans la chaîne réelle : capd, ledger et agentd en processus, un proxy de sortie simulé
//! qui joue le rôle de l'API de décision, et — quand un navigateur est là — une page réelle
//! opérée par son arbre sans qu'aucun modèle génératif ne soit appelé.
//!
//! Le proxy simulé est le seul faux de ces tests, et il l'est pour une raison précise : Jev est
//! un service distant, à clé, et rien ici ne doit dépendre d'un compte. Ce que le faux vérifie
//! est exactement ce que le vrai proxy exigerait : le jeton de la tâche dans l'en-tête interne,
//! et une référence de secret à la place de la clé.

use std::sync::{Arc, Mutex};

use prophet_daemon::essai::{Daemon, binaire_voisin};
use prophet_ipc::Client;
use serde_json::{Value, json};

const AGENTD: &str = env!("CARGO_BIN_EXE_prophet-agentd");

/// Le proxy de sortie parle HTTP, pas JSON-RPC : on l'attend avec une requête, pas un `ping`.
const SONDE: &[u8] = b"GET http://sonde.invalide/ HTTP/1.1\r\nHost: sonde.invalide\r\n\r\n";

/// Une décision de Jev, calculée à partir de la demande reçue.
type Decide = Arc<dyn Fn(&Value) -> Value + Send + Sync>;

/// Un faux proxy de sortie sur socket Unix : il lit une requête HTTP, note ses en-têtes, et
/// répond ce que le décideur rend pour le corps JSON reçu.
struct FauxEgress {
    heads: Arc<Mutex<Vec<String>>>,
    bodies: Arc<Mutex<Vec<Value>>>,
}

impl FauxEgress {
    async fn poser(socket: &std::path::Path, decide: Decide) -> Self {
        use tokio::io::{AsyncBufReadExt as _, AsyncReadExt as _, AsyncWriteExt as _, BufReader};
        let listener = tokio::net::UnixListener::bind(socket).unwrap();
        let heads = Arc::new(Mutex::new(Vec::new()));
        let bodies = Arc::new(Mutex::new(Vec::new()));
        let (h, b) = (heads.clone(), bodies.clone());
        tokio::spawn(async move {
            loop {
                let Ok((stream, _)) = listener.accept().await else {
                    return;
                };
                let mut reader = BufReader::new(stream);
                let mut head = String::new();
                let mut length = 0;
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                    if let Some(v) = line.to_ascii_lowercase().strip_prefix("content-length:") {
                        length = v.trim().parse().unwrap_or(0);
                    }
                    head.push_str(&line);
                }
                let mut body = vec![0; length];
                reader.read_exact(&mut body).await.unwrap();
                // Le navigateur piloté sort aussi par egress (ADR 0024) : ce qui ne vise pas
                // l'hôte de Jev est relayé vers la page témoin, comme le vrai proxy le ferait
                // pour un hôte accordé, et n'est pas compté parmi les décisions.
                let request_line = head.lines().next().unwrap_or_default().to_owned();
                if !request_line.contains(providers::jev::HOST) {
                    let reply = relayer(&request_line, &body).await;
                    let _ = reader.get_mut().write_all(&reply).await;
                    continue;
                }
                let request: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                h.lock().unwrap().push(head);
                b.lock().unwrap().push(request.clone());
                let answer = decide(&request).to_string();
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                    answer.len()
                );
                let _ = reader.get_mut().write_all(response.as_bytes()).await;
            }
        });
        Self { heads, bodies }
    }

    fn heads(&self) -> Vec<String> {
        self.heads.lock().unwrap().clone()
    }

    fn bodies(&self) -> Vec<Value> {
        self.bodies.lock().unwrap().clone()
    }
}

/// Relaie une requête en forme absolue (`GET http://hôte:port/chemin HTTP/1.1`) vers son hôte
/// et rend la réponse entière ; une adresse illisible reçoit un refus du proxy.
async fn relayer(request_line: &str, body: &[u8]) -> Vec<u8> {
    use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
    let refus =
        b"HTTP/1.1 502 Bad Gateway\r\nContent-Length: 0\r\nConnection: close\r\n\r\n".to_vec();
    let mut parts = request_line.split_whitespace();
    let (Some(method), Some(url)) = (parts.next(), parts.next()) else {
        return refus;
    };
    let Some(rest) = url.strip_prefix("http://") else {
        return refus;
    };
    let (authority, path) = rest.split_at(rest.find('/').unwrap_or(rest.len()));
    let path = if path.is_empty() { "/" } else { path };
    let Ok(mut upstream) = tokio::net::TcpStream::connect(authority).await else {
        return refus;
    };
    let request = format!(
        "{method} {path} HTTP/1.1\r\nHost: {authority}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
        body.len()
    );
    if upstream.write_all(request.as_bytes()).await.is_err()
        || upstream.write_all(body).await.is_err()
    {
        return refus;
    }
    let mut reply = Vec::new();
    let _ = upstream.read_to_end(&mut reply).await;
    reply
}

struct Chain {
    dir: tempfile::TempDir,
    _capd: Daemon,
    _ledger: Daemon,
    _agentd: Daemon,
    agents: Client,
    journal: Client,
}

impl Chain {
    async fn new(browser: Option<&str>) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("docs")).unwrap();
        let cap_socket = dir.path().join("cap.sock");
        let ledger_socket = dir.path().join("ledger.sock");
        let capd = Daemon::lancer_avec(
            binaire_voisin("prophet-capd").to_str().unwrap(),
            &cap_socket,
            &dir.path().join("cap-state"),
            &[("PROPHET_HOME", home.to_str().unwrap())],
        );
        drop(capd.joindre().await);
        let ledger = Daemon::lancer(
            binaire_voisin("prophet-ledger").to_str().unwrap(),
            &ledger_socket,
            &dir.path().join("ledger-state"),
        );
        let journal = ledger.joindre().await;
        let egress = dir.path().join("egress.sock");
        let mut env = vec![
            ("PROPHET_HOME", home.to_str().unwrap().to_owned()),
            (
                "PROPHET_CAPD_SOCKET",
                cap_socket.to_str().unwrap().to_owned(),
            ),
            (
                "PROPHET_LEDGER_SOCKET",
                ledger_socket.to_str().unwrap().to_owned(),
            ),
            ("PROPHET_EGRESS_SOCKET", egress.to_str().unwrap().to_owned()),
            // Aucun moteur génératif n'écoute : si un tour de LLM était nécessaire, la mission
            // échouerait. Une mission `done` prouve donc que Jev a tout décidé seul.
            ("PROPHET_LOCAL_ENDPOINT", "http://127.0.0.1:1/v1".to_owned()),
            ("PROPHET_JEV_SECRET", "typesafe".to_owned()),
        ];
        if let Some(program) = browser {
            env.push(("PROPHET_BROWSER", program.to_owned()));
        }
        let refs: Vec<(&str, &str)> = env.iter().map(|(k, v)| (*k, v.as_str())).collect();
        let agentd = Daemon::lancer_avec(
            AGENTD,
            &dir.path().join("agents.sock"),
            &dir.path().join("agent-state"),
            &refs,
        );
        let agents = agentd.joindre().await;
        Self {
            dir,
            _capd: capd,
            _ledger: ledger,
            _agentd: agentd,
            agents,
            journal,
        }
    }

    fn egress_socket(&self) -> std::path::PathBuf {
        self.dir.path().join("egress.sock")
    }

    async fn events(&self, task: &str) -> Vec<Value> {
        self.journal
            .call("ledger.query", json!({"task": task}))
            .await
            .unwrap()
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    async fn wait_terminal(&self, id: &str) -> Value {
        tokio::time::timeout(std::time::Duration::from_secs(120), async {
            loop {
                let status = self
                    .agents
                    .call("task.status", json!({"id": id}))
                    .await
                    .unwrap();
                if matches!(
                    status["state"].as_str(),
                    Some("done" | "failed" | "cancelled")
                ) {
                    return status;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await
        .unwrap()
    }
}

fn manifest(preferred: &[&str], privacy: &str, hosts: &[&str]) -> Value {
    let mut tool_call = vec!["fs.read", "fs.write"];
    if !hosts.is_empty() {
        tool_call.extend(["web.open", "web.tree", "web.act"]);
    }
    json!({
        "agent": {"id": "org.prophet.jev-test", "version": "1.0.0", "name": "Essai Jev",
                  "publisher_key": "ed25519:AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA="},
        "model": {"preferred": preferred, "privacy": privacy},
        "sandbox": {"min_level": 0},
        "capabilities": {"max": {
            "fs.read": ["~/docs/**"], "fs.write": ["~/docs/**"],
            "net.egress": hosts, "ui.read": ["browser"], "ui.act": ["browser"],
            "tool.call": tool_call,
        }},
        "budget": {"default": {"tokens": 40000, "wall_time": "100s", "approvals": 3}}
    })
}

fn requested(hosts: &[&str], web: bool) -> Vec<Value> {
    let mut grants = vec![
        json!({"res": "fs", "act": "read", "match": "~/docs/**"}),
        json!({"res": "fs", "act": "write", "match": "~/docs/**"}),
        json!({"res": "tool", "act": "call", "match": "fs.read"}),
        json!({"res": "tool", "act": "call", "match": "fs.write"}),
    ];
    for host in hosts {
        grants.push(json!({"res": "net", "act": "egress", "match": host}));
    }
    if web {
        grants.push(json!({"res": "ui", "act": "read", "match": "browser"}));
        grants.push(json!({"res": "ui", "act": "act", "match": "browser"}));
        for tool in ["web.open", "web.tree", "web.act"] {
            grants.push(json!({"res": "tool", "act": "call", "match": tool}));
        }
    }
    grants
}

/// Une réponse de routage : Jev préfère `second`.
fn routing_answer(request: &Value) -> Value {
    let options: Vec<String> = request["questions"]["model"]["criteria"]
        .as_object()
        .map(|o| o.keys().cloned().collect())
        .unwrap_or_default();
    let chosen = options
        .iter()
        .find(|o| o.ends_with(":second"))
        .cloned()
        .unwrap_or_else(|| options[0].clone());
    let probabilities: serde_json::Map<String, Value> = options
        .iter()
        .map(|o| (o.clone(), json!(if *o == chosen { 0.83 } else { 0.17 })))
        .collect();
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "model": {"type": "choice", "choice": chosen, "probabilities": probabilities, "confidence": 0.83},
            "difficulty": {"type": "score", "score": 2.4, "legend": {}, "probabilities": {}, "confidence": 0.7},
            "risk": {"type": "noul", "noul": 0.03}
        },
        "usage": {"input_tokens": 388, "output_tokens": 0}
    })
}

#[tokio::test]
async fn jev_route_la_mission_vers_le_modele_admissible_qu_il_prefere() {
    let chain = Chain::new(None).await;
    let egress = FauxEgress::poser(&chain.egress_socket(), Arc::new(routing_answer)).await;
    let plan = chain
        .agents
        .call(
            "task.spawn",
            json!({
                "id": "routee", "intent": "Refonds le module de paiement et migre ses tests", "user": "prophet",
                "manifest": manifest(&["local:first", "local:second"], "local-preferred", &["api.typesafe.ai"]),
                "requested": requested(&["api.typesafe.ai"], false),
                "scopes": ["~/docs"],
                "availability": {"local_models": ["first", "second"]}
            }),
        )
        .await
        .unwrap();
    assert_eq!(plan["choice"]["reference"], "local:second", "{plan}");
    assert_eq!(plan["route"]["decider"], "jev", "{plan}");
    assert!(
        plan["choice"]["reason"]
            .as_str()
            .unwrap()
            .starts_with("Jev : local:second (p = 0.83"),
        "{plan}"
    );
    assert!((plan["route"]["difficulty"].as_f64().unwrap() - 0.6).abs() < 1e-9);
    assert_eq!(plan["route"]["risk"], 0.03);

    let heads = egress.heads();
    assert_eq!(heads.len(), 1, "{heads:?}");
    assert!(
        heads[0].starts_with("POST https://api.typesafe.ai/v1/systemone HTTP/1.1"),
        "{}",
        heads[0]
    );
    assert!(
        heads[0].contains("Proxy-Authorization: Prophet "),
        "{}",
        heads[0]
    );
    assert!(
        heads[0].contains("Authorization: Bearer prophet-secret:typesafe\r\n"),
        "la clé ne doit jamais apparaître, seulement sa référence : {}",
        heads[0]
    );
    let body = &egress.bodies()[0];
    assert_eq!(body["model"], "jev-latest");
    assert_eq!(
        body["state"]["request"],
        "Refonds le module de paiement et migre ses tests"
    );
    assert_eq!(body["state"]["candidates"].as_array().unwrap().len(), 2);

    let events = chain.events("routee").await;
    let planned = events
        .iter()
        .find(|e| e["kind"] == "task.planned")
        .unwrap_or_else(|| panic!("{events:?}"));
    assert_eq!(planned["payload"]["provider"], "local:second");
    assert_eq!(planned["payload"]["route"]["decider"], "jev");
    assert_eq!(planned["payload"]["route"]["input_tokens"], 388);
    assert!(
        !events.iter().any(|e| e.to_string().contains("paiement")),
        "l'intention n'entre pas au journal"
    );
}

#[tokio::test]
async fn une_intention_local_only_ou_sans_sortie_vers_jev_reste_a_la_selection_statique() {
    let chain = Chain::new(None).await;
    let egress = FauxEgress::poser(&chain.egress_socket(), Arc::new(routing_answer)).await;
    let plan = chain
        .agents
        .call(
            "task.spawn",
            json!({
                "id": "privee", "intent": "Résume mes notes", "user": "prophet",
                "manifest": manifest(&["local:first", "local:second"], "local-only", &["api.typesafe.ai"]),
                "requested": requested(&["api.typesafe.ai"], false),
                "scopes": ["~/docs"],
                "availability": {"local_models": ["first", "second"]}
            }),
        )
        .await
        .unwrap();
    assert_eq!(plan["choice"]["reference"], "local:first", "{plan}");
    assert!(plan.get("route").is_none_or(Value::is_null), "{plan}");
    assert!(
        egress.heads().is_empty(),
        "rien ne doit être parti : {:?}",
        egress.heads()
    );

    let plan = chain
        .agents
        .call(
            "task.spawn",
            json!({
                "id": "sans-sortie", "intent": "Résume mes notes", "user": "prophet",
                "manifest": manifest(&["local:first", "local:second"], "any", &[]),
                "requested": requested(&[], false),
                "scopes": ["~/docs"],
                "availability": {"local_models": ["first", "second"]}
            }),
        )
        .await
        .unwrap();
    assert_eq!(plan["choice"]["reference"], "local:first", "{plan}");
    assert!(egress.heads().is_empty());

    let route = chain
        .agents
        .call(
            "task.route",
            json!({
                "intent": "Refonds le module de paiement",
                "manifest": manifest(&["local:first", "local:second"], "any", &["api.typesafe.ai"]),
                "availability": {"local_models": ["first", "second"]}
            }),
        )
        .await
        .unwrap();
    assert_eq!(route["decider"], "jev", "{route}");
    assert_eq!(route["choice"]["reference"], "local:second");
    assert_eq!(egress.heads().len(), 1);
    assert!(egress.heads()[0].contains("prophet-secret:typesafe"));

    let route = chain
        .agents
        .call(
            "task.route",
            json!({
                "intent": "Refonds le module de paiement",
                "manifest": manifest(&["local:first", "local:second"], "any", &[]),
                "availability": {"local_models": ["first", "second"]}
            }),
        )
        .await
        .unwrap();
    assert_eq!(route["decider"], "static", "{route}");
    assert!(
        route["fallback_reason"]
            .as_str()
            .unwrap()
            .contains("api.typesafe.ai"),
        "{route}"
    );
    assert_eq!(
        egress.heads().len(),
        1,
        "un manifeste sans sortie n'a rien envoyé"
    );
    assert!(chain.events("routee").await.is_empty());
}

fn chemin_du_navigateur() -> Option<String> {
    for candidat in [
        "/opt/pw-browsers/chromium-1194/chrome-linux/chrome",
        "/usr/bin/chromium",
        "/usr/bin/chromium-browser",
        "/usr/bin/google-chrome",
    ] {
        if std::path::Path::new(candidat).is_file() {
            return Some(candidat.to_owned());
        }
    }
    let depuis_environnement = std::env::var("PROPHET_BROWSER").ok();
    assert!(
        !(depuis_environnement.is_none()
            && std::env::var("PROPHET_EXIGER_NAVIGATEUR").as_deref() == Ok("1")),
        "aucun navigateur trouvé alors que PROPHET_EXIGER_NAVIGATEUR=1"
    );
    depuis_environnement
}

/// Deux pages : un formulaire avec un champ Ville et un lien, puis les horaires.
async fn site() -> u16 {
    use tokio::io::{AsyncBufReadExt as _, AsyncWriteExt as _, BufReader};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let port = listener.local_addr().unwrap().port();
    // Une tâche par connexion : une connexion ouverte d'avance sans requête ne fait pas
    // attendre la navigation.
    tokio::spawn(async move {
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            tokio::spawn(async move {
                let mut reader = BufReader::new(stream);
                let mut first = String::new();
                if reader.read_line(&mut first).await.unwrap_or(0) == 0 {
                    return;
                }
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 || line == "\r\n" {
                        break;
                    }
                }
                let body = if first.contains("/horaires") {
                    "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>Horaires de Paris</title></head><body><h1>Horaires de Paris</h1><p>Prochain départ à 9 h.</p></body></html>"
                } else {
                    "<!doctype html><html lang=\"fr\"><head><meta charset=\"utf-8\"><title>Réservation</title></head><body><h1>Réserver un billet</h1><label for=\"ville\">Ville</label><input id=\"ville\" name=\"ville\" type=\"text\"><a href=\"/horaires\">Voir les horaires</a></body></html>"
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: text/html; charset=utf-8\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = reader.get_mut().write_all(response.as_bytes()).await;
            });
        }
    });
    port
}

/// Le décideur d'interface : remplir la ville, ouvrir les horaires, conclure.
fn operating_answer(request: &Value) -> Value {
    let title = request["state"]["page"]["title"].as_str().unwrap_or("");
    let filled = request["state"]["page"]["fields"]
        .as_array()
        .is_some_and(|f| f.iter().any(|x| x["value"] == "Paris"));
    let options = request["questions"]["next"]["criteria"]
        .as_object()
        .unwrap();
    let pick = |prefix: &str, name: &str| {
        options
            .iter()
            .find(|(k, v)| k.starts_with(prefix) && v.to_string().contains(name))
            .map(|(k, _)| k.clone())
    };
    let (next, done) = if title.starts_with("Horaires") {
        ("done".to_owned(), 0.96)
    } else if !filled {
        (
            pick("fill:", "Paris").expect("une option de remplissage"),
            0.05,
        )
    } else {
        (
            pick("click:", "horaires").expect("un lien vers les horaires"),
            0.1,
        )
    };
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "next": {"type": "choice", "choice": next, "probabilities": {next.clone(): 0.91}, "confidence": 0.91},
            "done": {"type": "noul", "noul": done},
            "blocked": {"type": "noul", "noul": 0.01}
        },
        "usage": {"input_tokens": 730, "output_tokens": 0}
    })
}

#[tokio::test]
async fn jev_opere_une_page_reelle_par_son_arbre_sans_aucun_modele_generatif() {
    let Some(program) = chemin_du_navigateur() else {
        eprintln!("aucun navigateur : test ignoré");
        return;
    };
    let port = site().await;
    let chain = Chain::new(Some(&program)).await;
    let egress = FauxEgress::poser(&chain.egress_socket(), Arc::new(operating_answer)).await;
    let intent = format!(
        "Ouvre http://127.0.0.1:{port}/reservation, indique la ville « Paris » puis affiche les horaires."
    );
    let plan = chain
        .agents
        .call(
            "task.spawn",
            json!({
                "id": "operee", "intent": intent, "user": "prophet",
                "manifest": manifest(&["local:absent"], "local-preferred", &["127.0.0.1", "api.typesafe.ai"]),
                "requested": requested(&["127.0.0.1", "api.typesafe.ai"], true),
                "scopes": ["~/docs"],
                "availability": {"local_models": ["absent"]}
            }),
        )
        .await
        .unwrap();
    assert_eq!(plan["choice"]["reference"], "local:absent");
    chain
        .agents
        .call("task.start", json!({"id": "operee"}))
        .await
        .unwrap();
    let status = chain.wait_terminal("operee").await;
    assert_eq!(status["state"], "done", "{status}");
    assert!(
        status["budget"]["spent"]["tokens"].as_u64().unwrap() >= 3 * 730,
        "{status}"
    );

    let result = chain
        .agents
        .call("task.result", json!({"id": "operee"}))
        .await
        .unwrap();
    assert!(
        result["text"]
            .as_str()
            .unwrap()
            .contains("Objectif atteint selon Jev (p = 0.96)"),
        "{result}"
    );
    let actions: Vec<&str> = result["jev"]["actions"]
        .as_array()
        .unwrap()
        .iter()
        .map(|a| a.as_str().unwrap())
        .collect();
    assert_eq!(actions.len(), 3, "{actions:?}");
    assert!(
        actions[0].starts_with("open:http://127.0.0.1:"),
        "{actions:?}"
    );
    assert!(actions[1].starts_with("fill:"), "{actions:?}");
    assert!(actions[2].starts_with("click:"), "{actions:?}");
    assert_eq!(result["jev"]["decisions"], 3);
    assert_eq!(result["jev"]["handovers"], json!([]));
    assert_eq!(result["tool_calls"], 3);

    // Les trois décisions sont passées par le proxy, sous le jeton et avec la référence de secret.
    let heads = egress.heads();
    assert_eq!(heads.len(), 3, "{heads:?}");
    assert!(
        heads
            .iter()
            .all(|h| h.contains("Proxy-Authorization: Prophet ")
                && h.contains("prophet-secret:typesafe"))
    );
    let bodies = egress.bodies();
    assert_eq!(bodies[0]["state"]["page"]["title"], "Réservation");
    assert_eq!(bodies[0]["state"]["values"][0]["value"], "Paris");
    assert_eq!(bodies[1]["state"]["page"]["fields"][0]["value"], "Paris");
    assert_eq!(bodies[2]["state"]["page"]["title"], "Horaires de Paris");
    assert_eq!(
        bodies[2]["state"]["actions_so_far"]
            .as_array()
            .unwrap()
            .len(),
        3
    );

    let events = chain.events("operee").await;
    let started = events
        .iter()
        .find(|e| e["kind"] == "provider.started")
        .unwrap();
    assert_eq!(started["payload"]["decider"], "jev:jev-latest", "{started}");
    let calls: Vec<&str> = events
        .iter()
        .filter(|e| e["kind"] == "tool.call")
        .map(|e| e["payload"]["tool"].as_str().unwrap())
        .collect();
    assert_eq!(calls, ["web.open", "web.act", "web.act"], "{events:?}");
    assert!(
        events
            .iter()
            .filter(|e| e["kind"] == "tool.result")
            .all(|e| e["payload"]["ok"] == true),
        "{events:?}"
    );
    assert!(
        !events.iter().any(|e| e.to_string().contains("Paris")),
        "le contenu ne va pas au journal"
    );
}

// ---------------------------------------------------------------------------------------------
// Toute la chaîne, sans aucun faux : capd, ledger, coffre, proxy de sortie et agentd.
// ---------------------------------------------------------------------------------------------

/// Toute la chaîne en processus, et l'API réelle au bout du proxy. Le seul endroit où la clé
/// existe est le coffre : elle y entre par `vault.put`, comme par `prophet secret put`, bornée à
/// l'hôte de Jev.
struct RealChain {
    dir: tempfile::TempDir,
    _capd: Daemon,
    _ledger: Daemon,
    _vault: Daemon,
    _egress: Daemon,
    _agentd: Daemon,
    agents: Client,
}

impl RealChain {
    async fn new(cle: &str) -> Self {
        let dir = tempfile::tempdir().unwrap();
        let home = dir.path().join("home");
        std::fs::create_dir_all(home.join("docs")).unwrap();
        let cap_socket = dir.path().join("cap.sock");
        let ledger_socket = dir.path().join("ledger.sock");
        let vault_socket = dir.path().join("vault.sock");
        let egress_socket = dir.path().join("egress.sock");

        let capd = Daemon::lancer_avec(
            binaire_voisin("prophet-capd").to_str().unwrap(),
            &cap_socket,
            &dir.path().join("cap-state"),
            &[("PROPHET_HOME", home.to_str().unwrap())],
        );
        drop(capd.joindre().await);
        let ledger = Daemon::lancer(
            binaire_voisin("prophet-ledger").to_str().unwrap(),
            &ledger_socket,
            &dir.path().join("ledger-state"),
        );
        drop(ledger.joindre().await);

        let vault = Daemon::lancer(
            binaire_voisin("prophet-vault").to_str().unwrap(),
            &vault_socket,
            &dir.path().join("vault-state"),
        );
        let coffre = vault.joindre().await;
        coffre
            .call(
                "vault.put",
                json!({
                    "info": {
                        "name": "typesafe",
                        "domains": [providers::jev::HOST],
                        "header": "Authorization",
                        "description": "clé de l'API Jev"
                    },
                    "value": cle
                }),
            )
            .await
            .expect("le secret se dépose");
        drop(coffre);

        let egress = Daemon::lancer_avec(
            binaire_voisin("prophet-egress").to_str().unwrap(),
            &egress_socket,
            &dir.path().join("egress-state"),
            &[
                ("PROPHET_CAPD_SOCKET", cap_socket.to_str().unwrap()),
                ("PROPHET_VAULT_SOCKET", vault_socket.to_str().unwrap()),
                ("PROPHET_EGRESS_QUERY_HOSTS", providers::jev::HOST),
            ],
        );
        egress.attendre_reponse(SONDE).await;

        let agentd = Daemon::lancer_avec(
            AGENTD,
            &dir.path().join("agents.sock"),
            &dir.path().join("agent-state"),
            &[
                ("PROPHET_HOME", home.to_str().unwrap()),
                ("PROPHET_CAPD_SOCKET", cap_socket.to_str().unwrap()),
                ("PROPHET_LEDGER_SOCKET", ledger_socket.to_str().unwrap()),
                ("PROPHET_EGRESS_SOCKET", egress_socket.to_str().unwrap()),
                ("PROPHET_LOCAL_ENDPOINT", "http://127.0.0.1:1/v1"),
                ("PROPHET_JEV_SECRET", "typesafe"),
            ],
        );
        let agents = agentd.joindre().await;
        Self {
            dir,
            _capd: capd,
            _ledger: ledger,
            _vault: vault,
            _egress: egress,
            _agentd: agentd,
            agents,
        }
    }

    /// Demande une route sans planifier, pour une intention qui a deux candidats admissibles.
    async fn route(&self) -> Value {
        let candidats = ["local:qwen3-1.7b", "local:qwen3-32b"];
        self.agents
            .call(
                "task.route",
                json!({
                    "intent": "Refonds le module de paiement et migre ses tests d'intégration",
                    "manifest": manifest(&candidats, "local-preferred", &[providers::jev::HOST]),
                    "availability": {"local_models": ["qwen3-1.7b", "qwen3-32b"]}
                }),
            )
            .await
            .unwrap()
    }

    /// Vérifie que ces octets ne sont écrits en clair nulle part par les cinq services.
    fn n_ecrit_nulle_part(&self, secret: &[u8]) {
        fn parcourir(dir: &std::path::Path, secret: &[u8]) {
            let Ok(entrees) = std::fs::read_dir(dir) else {
                return;
            };
            for entree in entrees.flatten() {
                let chemin = entree.path();
                if chemin.is_dir() {
                    parcourir(&chemin, secret);
                } else if let Ok(octets) = std::fs::read(&chemin) {
                    assert!(
                        !octets
                            .windows(secret.len())
                            .any(|fenetre| fenetre == secret),
                        "le secret apparaît en clair dans {}",
                        chemin.display()
                    );
                }
            }
        }
        parcourir(self.dir.path(), secret);
    }
}

/// Le coffre ne révèle une valeur qu'au compte système `egress`, et ce test tourne sous un autre
/// compte : le coffre refuse, le proxy arrête la requête avant qu'elle ne parte, et la route
/// retombe sur la sélection statique en disant pourquoi — au lieu de partir avec la référence,
/// ou de rester suspendue. C'est la chaîne de l'image, moins le compte ; avec lui, la même
/// requête part vers l'API réelle (voir `tools/lancer-sur-l-hote.sh`).
#[tokio::test]
async fn sans_le_compte_du_proxy_la_route_retombe_proprement_sur_la_selection_statique() {
    let secret = "valeur-que-personne-ici-ne-doit-lire";
    let chain = RealChain::new(secret).await;
    let route = tokio::time::timeout(std::time::Duration::from_secs(60), chain.route())
        .await
        .expect("une route retombe en un temps borné, elle ne reste pas suspendue");

    assert_eq!(route["decider"], "static", "{route}");
    assert_eq!(route["choice"]["reference"], "local:qwen3-1.7b", "{route}");
    let raison = route["fallback_reason"].as_str().unwrap();
    assert!(
        raison.contains("SecretRefused"),
        "le refus du coffre doit être nommé : {raison}"
    );
    assert!(route["probabilities"].is_null(), "{route}");
    chain.n_ecrit_nulle_part(secret.as_bytes());
}
