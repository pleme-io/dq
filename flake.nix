{
  description = "dq — universal infrastructure data query tool";

  inputs = {
    nixpkgs.follows = "substrate/nixpkgs";

    # substrate owns the Rust build pattern: lockfile-builder.nix reconstructs
    # the build graph from the committed `Cargo.gen.lock` (gen delta) in pure
    # Nix, auto-fetching its pinned `gen` only when the delta is stale. Replaces
    # the raw `crate2nix` + `import ./Cargo.nix` path (which required a
    # generated Nix file that was never committed).
    substrate = {
      url = "github:pleme-io/substrate";
    };
  };

  outputs = {
    self,
    nixpkgs,
    substrate,
    ...
  }: let
    supportedSystems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forEachSystem = f:
      nixpkgs.lib.genAttrs supportedSystems (system:
        f {
          inherit system;
          pkgs = import nixpkgs {inherit system;};
        });

    # The workspace build graph for a given pkgs, reconstructed from
    # Cargo.gen.lock by substrate's lockfile-builder.
    projectFor = pkgs:
      (import "${substrate}/lib/build/rust/lockfile-builder.nix" {inherit pkgs;}).mkProject {
        src = self;
        name = "dq";
      };
  in {
    packages = forEachSystem ({pkgs, ...}: let
      # `dq-cli` enables the `lisp` feature by default (gen+lockfile-builder
      # resolves one feature set into Cargo.gen.lock, and `dq verify-mermaid`
      # needs it). `dq`, `default`, and `dq-full` are the same binary;
      # `dq-full` is kept as an alias so existing `#dq-full` references resolve.
      dq = (projectFor pkgs).workspaceMembers."dq-cli".build;
    in {
      default = dq;
      dq = dq;
      dq-full = dq;
    });

    apps = forEachSystem ({pkgs, ...}: let
      dq = (projectFor pkgs).workspaceMembers."dq-cli".build;
    in {
      default = {
        type = "app";
        program = "${dq}/bin/dq";
      };
      dq = {
        type = "app";
        program = "${dq}/bin/dq";
      };
      # `nix run #dq-full` must resolve to an APP (program = bin/dq).
      # Without this, it falls back to packages.dq-full and nix infers the
      # binary name from the crate pname ("dq-cli"), executing a nonexistent
      # `bin/dq-cli`. The binary is `bin/dq`; consumers (e.g. the repo-docs
      # generator) call `nix run github:pleme-io/dq#dq-full`.
      dq-full = {
        type = "app";
        program = "${dq}/bin/dq";
      };
    });

    overlays.default = final: _prev: {
      dq = (projectFor final).workspaceMembers."dq-cli".build;
    };
  };
}
