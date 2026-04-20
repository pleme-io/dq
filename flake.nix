{
  description = "dq — universal infrastructure data query tool";

  nixConfig = {
    allow-import-from-derivation = true;
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.11";
    crate2nix.url = "github:nix-community/crate2nix";
  };

  outputs = {
    self,
    nixpkgs,
    crate2nix,
    ...
  }: let
    supportedSystems = ["x86_64-linux" "aarch64-linux" "x86_64-darwin" "aarch64-darwin"];
    forEachSystem = f:
      nixpkgs.lib.genAttrs supportedSystems (system:
        f {
          pkgs = import nixpkgs {inherit system;};
          crate2nixPkg = crate2nix.packages.${system}.default;
        });
  in {
    packages = forEachSystem ({pkgs, ...}: let
      project = import ./Cargo.nix {inherit pkgs;};
      # `dq verify-mermaid` lives behind the `lisp` feature — it pulls
      # shikumi + tatara-lisp so the binary can parse Mermaid digest
      # Lisp files natively. Ships as `dq-full`; the default build
      # keeps the slimmer dep tree.
      dqFull = project.workspaceMembers."dq-cli".build.override {
        features = ["lisp"];
      };
    in {
      default = project.workspaceMembers."dq-cli".build;
      dq = project.workspaceMembers."dq-cli".build;
      dq-full = dqFull;
    });

    apps = forEachSystem ({pkgs, ...}: let
      project = import ./Cargo.nix {inherit pkgs;};
      dq = project.workspaceMembers."dq-cli".build;
    in {
      default = {
        type = "app";
        program = "${dq}/bin/dq";
      };
      dq = {
        type = "app";
        program = "${dq}/bin/dq";
      };
    });

    overlays.default = final: prev: {
      dq = (import ./Cargo.nix {pkgs = final;}).workspaceMembers."dq-cli".build;
    };
  };
}
