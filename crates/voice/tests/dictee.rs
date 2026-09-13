//! Une phrase française synthétisée par espeak-ng, transcrite par le vrai whisper.cpp avec un
//! vrai modèle : la chaîne de la parole, sans micro ni réseau. Ce que le test prouve : le
//! programme et le modèle configurés rendent un texte qui porte les mots-clés de la phrase.
//! Ce qu'il ne prouve pas : la qualité sur une voix humaine, ni l'enregistrement par PipeWire.
//!
//! Exigé : `PROPHET_WHISPER_MODEL` (modèle ggml), `PROPHET_WHISPER` ou `whisper-cli` sur le
//! chemin, et `PROPHET_TEST_ESPEAK` (programme espeak-ng).

#[test]
#[ignore = "needs_whisper_model: PROPHET_WHISPER_MODEL, whisper-cli et PROPHET_TEST_ESPEAK"]
fn une_phrase_synthetisee_est_transcrite_avec_ses_mots_cles() {
    let espeak = std::env::var("PROPHET_TEST_ESPEAK").unwrap();
    let tools = voice::Tools::from_env().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let wav = dir.path().join("phrase.wav");
    let status = std::process::Command::new(&espeak)
        .args(["-v", "fr", "-s", "150", "-w"])
        .arg(&wav)
        .arg("Écris une note de réunion dans mes documents, puis résume-la en trois points.")
        .status()
        .unwrap();
    assert!(status.success());
    let debut = std::time::Instant::now();
    let transcript = tools.transcribe(&wav, Some("fr")).unwrap();
    eprintln!(
        "transcription en {:.1} s ({} segments, langue {:?}) : « {} »",
        debut.elapsed().as_secs_f64(),
        transcript.segments,
        transcript.language,
        transcript.text
    );
    let texte = transcript.text.to_lowercase();
    assert!(texte.contains("note"), "{texte}");
    assert!(texte.contains("documents"), "{texte}");
    assert_eq!(transcript.language.as_deref(), Some("fr"));
    // La détection automatique reconnaît le français d'une voix de synthèse.
    let auto = tools.transcribe(&wav, None).unwrap();
    assert_eq!(auto.language.as_deref(), Some("fr"), "{auto:?}");
}
