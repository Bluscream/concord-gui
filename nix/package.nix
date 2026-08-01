{ lib
, stdenv
, craneLib
, rustPlatform
, pkg-config
, nasm
, opus
, alsa-lib
, pipewire
, src
, pname
, version
, description
, homepage
,
}:

let
  commonArgs = {
    inherit pname version src;

    cargoExtraArgs = "--locked";

    # PipeWire generates bindings at build time. The bindgen hook supplies
    # libclang and its search path without hard-coding a Nix store location.
    # OpenH264 uses NASM on x86_64, while AArch64 builds its NEON sources with cc.
    nativeBuildInputs = [
      pkg-config
    ] ++ lib.optionals stdenv.isLinux [
      rustPlatform.bindgenHook
    ] ++ lib.optionals stdenv.hostPlatform.isx86_64 [
      nasm
    ];

    # Networking uses rustls + webpki-roots, so we do not need openssl or a
    # system CA bundle here. Darwin stdenv provides the SDK by default, so avoid
    # legacy darwin.apple_sdk framework stubs.
    buildInputs = [
      opus
    ] ++ lib.optionals stdenv.isLinux [
      alsa-lib
      pipewire
    ];

    # The unit tests in this repo do not require network or a TTY, but disable
    # them by default to keep `nix build` fast and reproducible. Run `cargo test`
    # inside `nix develop` for the full test suite.
    doCheck = false;
  };

  cargoArtifacts = craneLib.buildDepsOnly commonArgs;
in
craneLib.buildPackage (commonArgs // {
  inherit cargoArtifacts;

  meta = {
    inherit description homepage;
    license = lib.licenses.gpl3Only;
    mainProgram = "concord";
    platforms = lib.platforms.unix;
  };
})
