# Jev, le décideur rapide de TypeSafe AI : optionnel, à clé, jamais dans le chemin principal.
#
# Rien ici ne donne la clé au service. Le propriétaire la dépose dans le coffre sous le nom
# configuré (`prophet secret put typesafe --host api.typesafe.ai`), agentd n'en connaît que le
# nom, et c'est le proxy de sortie qui la substitue dans la requête, au dernier moment, pour cet
# hôte et lui seul. Sans secret déposé, la première décision est refusée par le proxy et les
# missions continuent sans Jev, plus lentement.
#
# Le proxy apprend aussi que, pour cet hôte, un `POST` est une question et non une modification :
# sans cela, chaque décision attendrait une approbation humaine, et une boucle d'interface à
# plusieurs décisions par seconde n'existerait pas.
{ config, lib, ... }:
let
  cfg = config.prophet.jev;
in {
  options.prophet.jev = {
    enable = lib.mkEnableOption "le décideur Jev (TypeSafe AI) pour le routage et l'interface";
    secret = lib.mkOption {
      type = lib.types.strMatching "[A-Za-z0-9][A-Za-z0-9._-]{0,63}";
      default = "typesafe";
      description = "Nom du secret du coffre qui porte la clé d'API. Jamais la clé elle-même.";
    };
    model = lib.mkOption {
      type = lib.types.strMatching "[A-Za-z0-9][A-Za-z0-9._-]{0,63}";
      default = "jev-latest";
      description = "Modèle demandé à l'API ; la réponse nomme la version réellement servie.";
    };
    queryHosts = lib.mkOption {
      type = lib.types.listOf (lib.types.strMatching "[A-Za-z0-9*][A-Za-z0-9.*-]*(:[0-9]+)?");
      default = [ "api.typesafe.ai" ];
      description = ''
        Hôtes dont un `POST` est une interrogation et non un effet distant. Le proxy les
        contrôle comme des lectures : jeton, grant `net.egress` sur l'hôte, détection
        d'exfiltration et journal restent entiers ; seule la décision humaine par requête
        est levée, pour `POST` seulement. `*` est refusé par le proxy.
      '';
    };
  };

  config = lib.mkIf (config.prophet.enable && cfg.enable) {
    systemd.services.prophet-agentd.environment = {
      PROPHET_JEV_SECRET = cfg.secret;
      PROPHET_JEV_MODEL = cfg.model;
    };
    systemd.services.prophet-egress.environment.PROPHET_EGRESS_QUERY_HOSTS =
      lib.concatStringsSep "," cfg.queryHosts;
  };
}
