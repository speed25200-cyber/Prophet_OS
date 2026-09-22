//! Outils fichiers : chemins logiques autorisés, accès physiques confinés par Linux.

use serde_json::{Value, json};

use super::confined::{self, Operation};
use crate::protocol::{CallResult, ErrorCode, ToolMeta, ToolSpec};
use crate::registry::{ResourceAccess, Tool, ToolContext};

macro_rules! file_tool {
    ($type:ident, $name:literal, $description:literal, $requires:literal, $operation:ident, $key:literal, $properties:expr) => {
        #[doc = $description]
        #[derive(Debug)]
        pub struct $type;

        impl Tool for $type {
            fn spec(&self) -> ToolSpec {
                let required = match Operation::$operation {
                    Operation::Write => vec!["path", "content"],
                    _ => vec![$key],
                };
                ToolSpec {
                    name: $name.into(), description: $description.into(),
                    input_schema: json!({"type":"object","properties":$properties,"required":required,"additionalProperties":false}),
                    meta: Some(ToolMeta { requires:$requires.into(), irreversible:false, external:false, sandbox_level_min:None }),
                }
            }
            fn target(&self, args: &Value, context: &ToolContext) -> Option<String> {
                confined::target(args.get($key)?.as_str()?, context)
            }
            fn call(&self, _args: &Value, _context: &ToolContext) -> CallResult {
                CallResult::error(ErrorCode::PolicyDenied, "l'accès fichiers exige le contrôleur de ressources du registre")
            }
            fn call_checked(&self, args: &Value, context: &ToolContext, access: &dyn ResourceAccess) -> CallResult {
                confined::execute(Operation::$operation, args, context, access)
            }
        }
    };
}

file_tool!(
    Read,
    "fs.read",
    "Lit un fichier texte autorisé, sans suivre de liens symboliques ni de liens physiques multiples. Rend au plus 256 Kio à partir de `offset` et signale la troncature ; `next_offset` dit où reprendre pour lire la suite par morceaux.",
    "fs.read",
    Read,
    "path",
    json!({
        "path":{"type":"string"}, "max_bytes":{"type":"integer","minimum":0,"description":"Plafond demandé, borné à 262144 octets."},
        "offset":{"type":"integer","minimum":0,"description":"Octet où commencer ; le `next_offset` d'une lecture tronquée."}
    })
);
file_tool!(
    Write,
    "fs.write",
    "Écrit atomiquement un fichier dans l'espace de travail de la tâche, sans modifier le fichier de l'utilisateur. Limite : 1 Mio. La validation des changements reste explicite.",
    "fs.write",
    Write,
    "path",
    json!({
        "path":{"type":"string"}, "content":{"type":"string"}
    })
);
file_tool!(
    List,
    "fs.list",
    "Liste les entrées autorisées d'un répertoire en fusionnant fichiers d'origine et fichiers de travail. Les liens et entrées privées sont exclus. Rend au plus 2000 entrées, avec indication de troncature.",
    "fs.list",
    List,
    "path",
    json!({"path":{"type":"string"}})
);
file_tool!(
    Stat,
    "fs.stat",
    "Donne les métadonnées d'un fichier ou répertoire autorisé, sans suivre de lien symbolique. Les fichiers à liens physiques multiples sont refusés.",
    "fs.read",
    Stat,
    "path",
    json!({"path":{"type":"string"}})
);
file_tool!(
    Search,
    "fs.search",
    "Cherche par nom ou contenu dans les descendants autorisés. Par contenu, rend pour chaque fichier ses cinq premières lignes trouvées (numéro et extrait), à lire ensuite avec fs.read offset si besoin. Sans liens ; 200 résultats, 256 Kio par fichier, 8 Mio de contenu au total. Une recherche incomplète est signalée.",
    "fs.read",
    Search,
    "root",
    json!({
        "root":{"type":"string"}, "name_contains":{"type":"string"}, "content_contains":{"type":"string"}
    })
);
