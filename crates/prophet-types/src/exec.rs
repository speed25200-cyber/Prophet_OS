//! Les utilitaires confinés : ce qu'un agent peut exécuter sans microVM.
//!
//! Une courte liste de programmes qui lisent, comptent, trient et comparent sans rien modifier,
//! désignés par leur nom. Sous sandboxd, ils tournent confinés sur place (espaces de noms,
//! Landlock, seccomp), dans l'espace de travail de la tâche ; tout autre programme est du code
//! arbitraire, donc microVM (ADR 0031). La liste vit ici pour que le manifeste, la politique et
//! l'outil disent la même chose.

/// Les programmes de la liste, par leur nom.
pub const SAFE_UTILITIES: &[&str] = &[
    "cat", "ls", "wc", "head", "tail", "sort", "uniq", "rg", "grep", "cut", "tr", "diff", "file",
];

/// Vrai si le programme, désigné par son nom seul, est un utilitaire de la liste.
///
/// Un chemin n'en est jamais un : `/tmp/x/cat` est un binaire choisi par l'appelant, pas le
/// `cat` que le PATH du service résout. Seul le nom nu ouvre l'exécution confinée sur place.
#[must_use]
pub fn is_safe_utility(program: &str) -> bool {
    !program.contains('/') && SAFE_UTILITIES.contains(&program)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn seul_le_nom_nu_designe_un_utilitaire_confine() {
        assert!(is_safe_utility("wc"));
        assert!(!is_safe_utility("/nix/store/abc-coreutils-9.5/bin/wc"));
        assert!(!is_safe_utility("/tmp/evil/cat"));
        assert!(!is_safe_utility("sh"));
        assert!(!is_safe_utility("/usr/bin/python3"));
        assert!(!is_safe_utility(""));
    }
}
