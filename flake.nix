{
  description = "babangida — underground RU hip-hop соцсеть по инвайтам. Полностью Rust-стек (ADR-0001).";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
    rust-overlay = {
      url = "github:oxalica/rust-overlay";
      inputs.nixpkgs.follows = "nixpkgs";
    };
  };

  outputs = { self, nixpkgs, flake-utils, rust-overlay }:
    flake-utils.lib.eachDefaultSystem (system:
      let
        pkgs = import nixpkgs {
          inherit system;
          overlays = [ (import rust-overlay) ];
        };

        # Тулчейн из rust-toolchain.toml: pinned stable + clippy/rustfmt/rust-src + wasm32 (для Leptos).
        rustToolchain = pkgs.rust-bin.fromRustupToolchainFile ./rust-toolchain.toml;

        postgres = pkgs.postgresql_16;
        pgPort = "5433";
        databaseUrl = "postgres://postgres@127.0.0.1:${pgPort}/babangida";

        # Локальный postgres в каталоге проекта (.pg). Идемпотентно: initdb при первом
        # запуске, затем start; создаёт БД babangida. nix develop вызывает pg-start.
        pgStart = pkgs.writeShellApplication {
          name = "pg-start";
          runtimeInputs = [ postgres ];
          text = ''
            PGDATA="$PWD/.pg/data"
            PGSOCK="$PWD/.pg/sockets"
            mkdir -p "$PGSOCK"
            if [ ! -d "$PGDATA" ]; then
              initdb -D "$PGDATA" -U postgres --auth=trust --encoding=UTF8 --locale=C >/dev/null
            fi
            if ! pg_ctl -D "$PGDATA" status >/dev/null 2>&1; then
              pg_ctl -D "$PGDATA" -l "$PWD/.pg/server.log" \
                -o "-p ${pgPort} -k $PGSOCK -c listen_addresses=127.0.0.1" start
            fi
            createdb -h 127.0.0.1 -p ${pgPort} -U postgres babangida 2>/dev/null || true
            echo "postgres готов: ${databaseUrl}"
          '';
        };

        pgStop = pkgs.writeShellApplication {
          name = "pg-stop";
          runtimeInputs = [ postgres ];
          text = ''
            pg_ctl -D "$PWD/.pg/data" stop 2>/dev/null || true
          '';
        };

        # Шаблон CI: одна команда — fmt + clippy + check + test (включая интеграционные
        # тесты против локального postgres). CI и локально: `nix run .#ci`.
        ci = pkgs.writeShellApplication {
          name = "ci";
          runtimeInputs = [ rustToolchain postgres pgStart ];
          text = ''
            export DATABASE_URL="${databaseUrl}"
            pg-start
            cargo fmt --all -- --check
            cargo clippy --workspace --all-targets -- -D warnings
            cargo check --workspace
            cargo test --workspace
          '';
        };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = [
            rustToolchain
            pkgs.sqlx-cli
            postgres
            pgStart
            pgStop
            pkgs.pkg-config
            pkgs.openssl
            # frontend (Leptos SSR, ADR-0006): сборка web через `cargo leptos`.
            pkgs.cargo-leptos
            pkgs.tailwindcss
            pkgs.binaryen
            # генерация JS/WASM-бандла; версия обязана совпадать с крейтом wasm-bindgen
            # (закреплён в Cargo.toml под версию из nixpkgs).
            pkgs.wasm-bindgen-cli
          ];

          shellHook = ''
            export DATABASE_URL="${databaseUrl}"
            export PGHOST=127.0.0.1 PGPORT=${pgPort} PGUSER=postgres PGDATABASE=babangida
            # nix develop поднимает БД локально; в CI она не нужна для check/clippy/test.
            if [ -z "''${CI:-}" ]; then
              pg-start || echo "pg-start не удался — подними БД вручную: pg-start"
            fi
            echo "babangida dev shell · $(rustc --version) · DB: $DATABASE_URL (pg-start/pg-stop)"
          '';
        };

        packages.ci = ci;
        apps.ci = {
          type = "app";
          program = "${ci}/bin/ci";
          meta.description = "fmt + clippy + check + test";
        };
        formatter = pkgs.nixpkgs-fmt;
      });
}
