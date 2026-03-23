{
  description = "dq — universal infrastructure data query tool";

  nixConfig = {
    allow-import-from-derivation = true;
  };

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-25.05";
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
    in {
      default = project.workspaceMembers."dq-cli".build;
      dq = project.workspaceMembers."dq-cli".build;
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
