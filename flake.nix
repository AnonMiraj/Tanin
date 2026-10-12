{
  description = "Tanin - A TUI ambient sound generator";

  inputs.nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";

  outputs =
    { nixpkgs, ... }:
    let
      lib = nixpkgs.lib;

      # Linux only: the audio stack (alsa-lib, jack2) is not available on Darwin.
      systems = [
        "x86_64-linux"
        "aarch64-linux"
      ];

      version = (builtins.fromTOML (builtins.readFile ./Cargo.toml)).package.version;

      # Release assets produced by .github/workflows/release.yml. Update the
      # checksums whenever the version in Cargo.toml changes.
      targets = {
        x86_64-linux = "x86_64-unknown-linux-gnu";
        aarch64-linux = "aarch64-unknown-linux-gnu";
      };

      hashes = {
        x86_64-linux = "3f135a717605596635a579f6b8975d777ab9e86a80dff3ada299d2511b442068";
        aarch64-linux = "4a295caca064716929482add99947cf1c8c52ac11f46ca44b8651764caa8a84e";
      };

      audioLibs = pkgs: with pkgs; [
        alsa-lib
        jack2
        openssl
        libopus
      ];

      jackWrapper = pkgs: ''
        wrapProgram $out/bin/tanin --prefix LD_LIBRARY_PATH : "${lib.makeLibraryPath [ pkgs.jack2 ]}"
      '';

      meta = {
        description = "A TUI ambient sound generator written in Rust";
        homepage = "https://github.com/AnonMiraj/Tanin";
        license = lib.licenses.mit;
        mainProgram = "tanin";
      };

      # Option 1: compile from this flake's source.
      taninFor = pkgs: pkgs.rustPlatform.buildRustPackage {
        pname = "tanin";
        inherit version meta;
        src = ./.;
        cargoLock.lockFile = ./Cargo.lock;
        nativeBuildInputs = [
          pkgs.pkg-config
          pkgs.autoPatchelfHook
          pkgs.makeWrapper
        ];
        buildInputs = audioLibs pkgs ++ [ pkgs.stdenv.cc.cc.lib ];
        postInstall = jackWrapper pkgs;
      };

      # Option 2: unpack the prebuilt release asset, no compilation.
      taninBinFor =
        pkgs:
        let
          system = pkgs.stdenv.hostPlatform.system;
        in
        pkgs.stdenv.mkDerivation {
          pname = "tanin-bin";
          inherit version meta;
          src = pkgs.fetchurl {
            url = "https://github.com/AnonMiraj/Tanin/releases/download/v${version}/tanin-v${version}-${targets.${system}}.tar.gz";
            sha256 = hashes.${system};
          };
          sourceRoot = "."; # the tarball has no top-level directory
          nativeBuildInputs = [
            pkgs.autoPatchelfHook
            pkgs.makeWrapper
          ];
          buildInputs = audioLibs pkgs ++ [ pkgs.stdenv.cc.cc.lib ];
          dontConfigure = true;
          dontBuild = true;
          installPhase = ''
            install -Dm755 tanin $out/bin/tanin
            install -Dm644 LICENSE $out/share/licenses/tanin/LICENSE
          '';
          postFixup = jackWrapper pkgs;
        };

      packagesFor =
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
          tanin = taninFor pkgs;
        in
        {
          inherit tanin;
          tanin-bin = taninBinFor pkgs;
          default = tanin;
        };
    in
    {
      packages = lib.genAttrs systems packagesFor;

      # Shared libraries from buildInputs are not on the loader path in a shell.
      devShells = lib.genAttrs systems (
        system:
        let
          pkgs = nixpkgs.legacyPackages.${system};
        in
        {
          default = pkgs.mkShell {
            nativeBuildInputs = with pkgs; [
              cargo
              rustc
              pkg-config
              autoconf
              automake
              libtool
            ];
            buildInputs = audioLibs pkgs;
            shellHook = ''
              export LD_LIBRARY_PATH="${lib.makeLibraryPath (audioLibs pkgs)}:$LD_LIBRARY_PATH"
            '';
          };
        }
      );
    };
}
