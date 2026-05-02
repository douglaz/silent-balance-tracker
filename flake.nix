{
  description = "Silent.link multi-account balance tracker with dashboard";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay.url = "github:oxalica/rust-overlay";
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        rustToolchain = pkgs.rust-bin.stable.latest.default.override {
          extensions = [ "rust-src" "rust-analyzer" ];
          targets = [ "x86_64-unknown-linux-musl" ];
        };

        silent-balance-tracker = pkgs.rustPlatform.buildRustPackage {
          pname = "silent-balance-tracker";
          version = "0.1.0";
          src = ./.;

          cargoLock = {
            lockFile = ./Cargo.lock;
          };

          nativeBuildInputs = with pkgs; [
            pkg-config
            rustToolchain
          ];

          # rusqlite is built with `bundled` so we don't need a system sqlite,
          # and reqwest uses rustls so we don't need openssl.
          buildInputs = with pkgs; [ ];

          CARGO_BUILD_TARGET = "x86_64-unknown-linux-musl";
          CARGO_TARGET_X86_64_UNKNOWN_LINUX_MUSL_LINKER = "${pkgs.pkgsStatic.stdenv.cc}/bin/${pkgs.pkgsStatic.stdenv.cc.targetPrefix}cc";

          doCheck = false;

          meta = with pkgs.lib; {
            description = "Silent.link multi-account balance tracker";
            license = licenses.mit;
          };
        };
      in
      {
        packages = {
          default = silent-balance-tracker;

          dockerImage = pkgs.dockerTools.buildImage {
            name = "silent-balance-tracker";
            tag = "latest";

            copyToRoot = pkgs.buildEnv {
              name = "image-root";
              paths = [
                silent-balance-tracker
                pkgs.cacert
                pkgs.busybox
              ];
              pathsToLink = [ "/bin" "/etc" "/share" ];
            };

            config = {
              Cmd = [ "/bin/silent-balance-tracker" "serve" ];
              ExposedPorts = {
                "8080/tcp" = {};
              };
              Env = [
                "RUST_LOG=info,silent_balance_tracker=debug"
                "SERVER_HOST=0.0.0.0"
                "SERVER_PORT=8080"
                "SSL_CERT_FILE=${pkgs.cacert}/etc/ssl/certs/ca-bundle.crt"
                "SYSTEM_CERTIFICATE_PATH=${pkgs.cacert}/etc/ssl/certs"
                "PATH=/bin"
              ];
              Labels = {
                "org.opencontainers.image.source" = "https://github.com/douglaz/silent-balance-tracker";
                "org.opencontainers.image.description" = "Silent.link multi-account balance tracker";
                "org.opencontainers.image.licenses" = "MIT";
              };
            };
          };
        };

        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            rustToolchain
            rust-analyzer
            cargo-watch
            cargo-edit
            pkg-config
            sqlite

            # API testing
            curl
            jq
            httpie
          ];

          RUST_BACKTRACE = "full";
          RUST_LOG = "debug";

          shellHook = ''
            echo "🦀 silent-balance-tracker dev shell"
            echo ""
            echo "Commands:"
            echo "  cargo run -- serve --config ./config.toml"
            echo "  cargo run -- poll  --config ./config.toml"
            echo "  cargo test"
            echo "  nix build .#dockerImage && docker load < result"
            echo ""

            if [ ! -f .env ] && [ -f .env.example ]; then
              cp .env.example .env
              echo "Created .env from .env.example"
            fi
          '';
        };

        apps.default = flake-utils.lib.mkApp {
          drv = silent-balance-tracker;
        };

        checks = {
          inherit silent-balance-tracker;

          format = pkgs.runCommand "format-check" {} ''
            cd ${./.}
            ${rustToolchain}/bin/cargo fmt --check
            touch $out
          '';

          clippy = pkgs.runCommand "clippy-check" {} ''
            cd ${./.}
            ${rustToolchain}/bin/cargo clippy --all-targets --all-features -- -D warnings
            touch $out
          '';
        };
      }
    );
}
