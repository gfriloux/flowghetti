{
  config,
  lib,
  pkgs,
  ...
}: let
  cfg = config.programs.flowghetti;
in {
  options.programs.flowghetti = {
    enable = lib.mkEnableOption "flowghetti, the Terraform-flows to Graphviz exporter";

    package = lib.mkOption {
      type = lib.types.package;
      default = pkgs.callPackage ./package.nix {};
      defaultText = lib.literalExpression "pkgs.callPackage ./package.nix {}";
      description = "The flowghetti package to use.";
    };
  };

  config = lib.mkIf cfg.enable {
    home.packages = [cfg.package];
  };
}
