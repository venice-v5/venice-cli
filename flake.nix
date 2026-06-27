{
  description = "Development shell for venice-cli";

  inputs = {
    nixpkgs.url = "github:NixOS/nixpkgs/nixos-unstable";
    flake-utils.url = "github:numtide/flake-utils";
  };

  outputs =
    {
      self,
      nixpkgs,
      flake-utils,
    }:
    flake-utils.lib.eachDefaultSystem (
      system:
      let
        pkgs = import nixpkgs { inherit system; };
      in
      {
        devShells.default = pkgs.mkShell {
          packages = with pkgs; [
            uv
            rustc
            cargo
            maturin
          ];

          shellHook = ''
            rm -rf .venv || true
            uv venv -p=3.13 .venv

            source .venv/bin/activate
          '';
        };
      }
    );
}
