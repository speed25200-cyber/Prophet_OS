#!/usr/bin/env python3
"""Exerce le vrai parcours agentd avec un moteur et des poids explicitement fournis.

À lancer dans nix develop après cargo build --workspace --bins. Aucun téléchargement,
aucune réponse simulée. Les fichiers créés par les tests restent dans leurs répertoires
temporaires. Le serveur appartient à cet essai et est arrêté dans tous les cas.
"""
import argparse
import hashlib
import json
import os
from pathlib import Path
import socket
import subprocess
import time
import urllib.error
import urllib.request


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--llama-server", type=Path, required=True)
    parser.add_argument("--weights", type=Path, required=True)
    parser.add_argument("--model", required=True)
    parser.add_argument("--output", type=Path, required=True)
    parser.add_argument("--repetitions", type=int, default=3)
    parser.add_argument("--threads", type=int, default=4)
    parser.add_argument("--surface", action="store_true",
                        help="saisir et lancer la mission dans l'interface native (adaptateur Vulkan requis)")
    args = parser.parse_args()
    if not 1 <= args.repetitions <= 20 or not 1 <= args.threads <= 128:
        parser.error("repetitions : 1–20 ; threads : 1–128")
    engine = args.llama_server.resolve(strict=True)
    weights = args.weights.resolve(strict=True)
    if not engine.is_file() or not weights.is_file() or not args.model.strip():
        parser.error("binaire, poids et identifiant de modèle requis")
    args.output.mkdir(parents=True, exist_ok=False)
    with weights.open("rb") as stream:
        digest = hashlib.file_digest(stream, "sha256").hexdigest()
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    endpoint = f"http://127.0.0.1:{port}"
    command = [str(engine), "-m", str(weights), "--host", "127.0.0.1", "--port", str(port),
               "--alias", args.model, "--jinja", "--reasoning", "off", "-c", "4096",
               "-t", str(args.threads), "-np", "1", "--temp", "0.7", "--top-p", "0.8",
               "--top-k", "20", "--min-p", "0", "--presence-penalty", "1.5", "--seed", "2026"]
    report = {"command": command, "weights_sha256": digest,
              "parcours": "surface" if args.surface else "agentd", "runs": []}
    # Aucun proxy d'environnement ne doit détourner la sonde locale.
    opener = urllib.request.build_opener(urllib.request.ProxyHandler({}))
    with (args.output / "server.log").open("w") as log:
        server = subprocess.Popen(command, stdout=log, stderr=log)
        try:
            ready_by = time.monotonic() + 90
            while True:
                if server.poll() is not None:
                    raise RuntimeError("le moteur s'est arrêté avant de répondre")
                try:
                    with opener.open(endpoint + "/health", timeout=1) as response:
                        if response.status == 200:
                            break
                except (urllib.error.URLError, TimeoutError):
                    pass
                if time.monotonic() >= ready_by:
                    raise RuntimeError("le moteur ne répond pas après 90 secondes")
                time.sleep(.1)
            env = dict(os.environ, PROPHET_TEST_ENDPOINT=endpoint + "/v1",
                       PROPHET_TEST_MODEL=args.model, CARGO_TERM_COLOR="never")
            case = "une_mission_reelle_traverse_agentd_capd_ledger_et_survit_au_redemarrage"
            test_command = ["cargo", "test", "-p", "agentd", "--test", "local_daemon"]
            if args.surface:
                case = "une_intention_graphique_est_executee_par_un_modele_reel"
                test_command = ["cargo", "test", "-p", "surface", "--features", "real-model-tests",
                                "--test", "missions"]
            for number in range(1, args.repetitions + 1):
                started = time.monotonic()
                log_path = args.output / f"mission-{number}.log"
                if args.surface:
                    capture_path = (args.output / f"captures-{number}").resolve()
                    env["PROPHET_CAPTURE_DIR"] = str(capture_path)
                with log_path.open("w") as test_log:
                    result = subprocess.run(
                        test_command + [case, "--", "--ignored", "--exact", "--nocapture"],
                        env=env, stdout=test_log, stderr=test_log, check=False)
                # Un test renommé ne doit pas donner un faux vert avec zéro test exécuté.
                proof = log_path.read_text()
                confirmed = f"test {case} ... ok" in proof and "1 passed; 0 failed" in proof
                run = {"number": number, "exit_code": result.returncode or int(not confirmed),
                       "confirmed": confirmed,
                       "seconds": round(time.monotonic() - started, 3)}
                report["runs"].append(run)
                (args.output / "result.json").write_text(json.dumps(report, indent=2) + "\n")
                print(json.dumps(run), flush=True)
        finally:
            server.terminate()
            try:
                server.wait(timeout=10)
            except subprocess.TimeoutExpired:
                server.kill()
                server.wait()
    return int(any(run["exit_code"] != 0 for run in report["runs"]))


if __name__ == "__main__":
    raise SystemExit(main())
