{
  description = "dq — universal infrastructure data query tool";

  # substrate.rust.library dispatches over Cargo.gen.lock (the slim gen delta,
  # reconstructed to the full BuildSpec in pure Nix) — no crate2nix, no Cargo.nix.
  inputs.substrate.url = "github:pleme-io/substrate";

  outputs = { substrate, ... }: let
    base = substrate.rust.library {
      src = ./.;
      member = "dq-cli";
    };
  in base // {
    packages = builtins.mapAttrs (_: p: p // { dq-full = p.dq; }) base.packages;
    apps = builtins.mapAttrs (system: a: a // {
      dq-full = {
        type = "app";
        program = "${base.packages.${system}.dq}/bin/dq";
      };
    }) base.apps;
  };
}
