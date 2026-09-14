# L'accélération graphique des modèles locaux sur la machine installée (ADR 0037).
#
# Vide dans le dépôt : les modèles tournent sur processeur. Quand la machine sur laquelle il
# s'exécute a un périphérique Vulkan utilisable (une vraie carte, pas le rastériseur logiciel),
# l'installeur écrit ici `prophet.localEngine.gpu.enable = true`, et le moteur local est servi
# par la variante Vulkan de llama.cpp.
{ ... }: { }
