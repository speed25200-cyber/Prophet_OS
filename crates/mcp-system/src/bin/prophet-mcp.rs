//! Serveur MCP système, lancé par les clients d'éditeurs et par la boucle native.
//!
//! Usage : `prophet-mcp <domaine>`. Le jeton de la tâche est lu dans le fichier désigné par
//! `PROPHET_TASK_AUTH_FILE`, jamais passé en argument : un argument se lit dans la liste des
//! processus, un fichier en mode 0600 ne se lit pas.

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("prophet-mcp : {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), Box<dyn std::error::Error>> {
    let domain = std::env::args().nth(1).unwrap_or_else(|| "all".to_owned());
    let auth_file = std::env::var("PROPHET_TASK_AUTH_FILE")
        .map_err(|_| "variable PROPHET_TASK_AUTH_FILE absente")?;
    let token_text = std::fs::read_to_string(&auth_file)
        .map_err(|e| format!("jeton illisible dans {auth_file} : {e}"))?;
    let _token: prophet_types::cap::Token =
        serde_json::from_str(token_text.trim()).map_err(|e| format!("jeton mal formé : {e}"))?;
    Err(format!(
        "domaine {domain} : ce point d'entrée exige la clé publique du broker en service, \
         fournie par agentd (M8). La bibliothèque est utilisable dès maintenant."
    )
    .into())
}
