{ lib
, stdenv
, craneLib
, rustPlatform
, pkg-config
, opus
, alsa-lib
, libgbm
, libglvnd
, libxcb
, pipewire
, wayland
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
    nativeBuildInputs = [
      pkg-config
    ] ++ lib.optionals stdenv.isLinux [
      rustPlatform.bindgenHook
    ];

    # Networking uses rustls + webpki-roots, so we do not need openssl or a
    # system CA bundle here. Darwin stdenv provides the SDK by default, so avoid
    # legacy darwin.apple_sdk framework stubs.
    buildInputs = [
      opus
    ] ++ lib.optionals stdenv.isLinux [
      alsa-lib
      libgbm
      libglvnd
      libxcb
      pipewire
      wayland
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
