{
  lib,
  rustPlatform,
}:
rustPlatform.buildRustPackage {
  pname = "flowghetti";
  version = "0.1.0";

  src = lib.cleanSourceWith {
    src = ../.;
    filter = path: type:
      (lib.cleanSourceFilter path type)
      && (
        let
          base = baseNameOf path;
        in
          base != "target" && base != "result" && base != ".direnv" && base != ".claude"
      );
  };

  cargoLock.lockFile = ../Cargo.lock;

  meta = {
    description = "Export glowwiththeflow Terraform code to a Graphviz network-flow graph";
    mainProgram = "flowghetti";
  };
}
