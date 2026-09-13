# La parole de l'humain (ADR 0036) : whisper.cpp et l'enregistreur de PipeWire dans la session,
# un modèle ggml local, et les variables que `prophet voice` lit. Rien ne sort de la machine.
{ config, lib, pkgs, ... }:
let
  cfg = config.prophet.voice;
in {
  options.prophet.voice = {
    enable = lib.mkEnableOption "la parole locale (whisper.cpp, PipeWire)" // { default = true; };
    model = lib.mkOption {
      type = lib.types.nullOr lib.types.path;
      default = null;
      description = "Modèle ggml de whisper.cpp (ggml-base.bin, multilingue, 148 Mo). null laisse la parole non configurée : `prophet voice` le dit. La configuration de référence du flake le télécharge à l'installation, comme les modèles de langue ; la variante d'intégration continue reste sans modèle.";
    };
  };

  config = lib.mkIf (config.prophet.enable && cfg.enable) {
    assertions = [{
      assertion = cfg.model == null || lib.hasPrefix "/nix/store/" (toString cfg.model)
        || lib.hasPrefix "/var/lib/prophet/models/" (toString cfg.model);
      message = "Le modèle de parole doit être placé dans /nix/store ou /var/lib/prophet/models.";
    }];
    environment.systemPackages = [ pkgs.whisper-cpp pkgs.pipewire ];
    environment.sessionVariables = {
      PROPHET_WHISPER = "${pkgs.whisper-cpp}/bin/whisper-cli";
      PROPHET_RECORDER = "${pkgs.pipewire}/bin/pw-record";
    } // lib.optionalAttrs (cfg.model != null) {
      PROPHET_WHISPER_MODEL = toString cfg.model;
    };
  };
}
