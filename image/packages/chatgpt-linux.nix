# Paquet de compatibilité Linux expérimental : les fichiers applicatifs restent inchangés.
# Le runtime FHS fournit des chemins Linux usuels ; il ne remplace pas sandboxd.
{ lib, stdenvNoCC, fetchurl, dpkg, buildFHSEnv }:

let
  version = "26.908.40834";
  payload = stdenvNoCC.mkDerivation {
    pname = "chatgpt-linux-payload";
    inherit version;
    src = fetchurl {
      url = "https://persistent.oaistatic.com/codex-app-prod/linux/deb/pool/main/c/chatgpt/chatgpt_${version}_amd64.deb";
      sha256 = "da37b8e7bcefaaea019c478cacbe6c73ee1ddd15e0e1ebb3c7ef0a42dd818ac2";
    };
    nativeBuildInputs = [ dpkg ];
    dontUnpack = true;
    dontFixup = true;
    installPhase = ''
      mkdir -p "$out"
      dpkg-deb --extract "$src" "$out"
    '';
    meta = {
      license = lib.licenses.unfree;
      platforms = [ "x86_64-linux" ];
      sourceProvenance = [ lib.sourceTypes.binaryNativeCode ];
    };
  };
in
buildFHSEnv {
  pname = "chatgpt-linux";
  inherit version;
  executableName = "chatgpt";
  targetPkgs = pkgs: with pkgs; [
    alsa-lib at-spi2-core cairo cups dbus expat fontconfig freetype
    gdk-pixbuf glib gtk3 libdrm libgbm libGL libnotify libusb1
    libxkbcommon nspr nss pango systemd xdg-utils
    libx11 libxcomposite libxdamage libxext
    libxfixes libxrandr libxcb
    libsecret git
  ];
  runScript = "${payload}/usr/lib/chatgpt/codex-launcher";
  extraInstallCommands = ''
    mkdir -p "$out/share/applications" "$out/share/pixmaps"
    cp ${payload}/usr/share/applications/chatgpt.desktop "$out/share/applications/"
    cp ${payload}/usr/share/pixmaps/chatgpt.png "$out/share/pixmaps/"
  '';
  passthru = { inherit payload; };
  meta = {
    description = "Application officielle ChatGPT Linux dans un runtime de compatibilité FHS";
    homepage = "https://learn.chatgpt.com/docs/linux/linux-app";
    license = lib.licenses.unfree;
    platforms = [ "x86_64-linux" ];
  };
}
