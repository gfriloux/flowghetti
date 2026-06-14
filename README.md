# flowghetti

Export [glowwiththeflow](https://www.github.com/gfriloux/glowwiththeflow) Terraform code to a Graphviz
network-flow diagram, by **static analysis** of the HCL — no `terraform`, no
AWS, no network.

flowghetti reads a Terraform root directory, finds the glowwiththeflow module
call, resolves the `ressources` / `flows` it is given (including `local.*`,
`var.*` and, where present, module-output references), and emits a DOT graph of
the network flows.

## Usage

```bash
flowghetti path/to/terraform | dot -Tsvg > flows.svg

# orientation (rankdir): LR (default), TB, RL, BT
flowghetti --rankdir TB path/to/terraform | dot -Tpng > flows.png

# colour theme: light (default) or dark
flowghetti --theme dark path/to/terraform | dot -Tpng > flows.png

# include a legend documenting node categories
flowghetti --legend path/to/terraform | dot -Tsvg > flows.svg

# add a title at the top of the graph
flowghetti --title "staging / eu-west-1" path/to/terraform | dot -Tsvg > flows.svg
```

Nodes are shaped by category (load balancer, database, serverless, compute) and
edges are coloured by protocol/port (secure web, admin, database, neutral), with
permissive `all` rules flagged in bold red. The `--theme dark` palette renders
the same graph on a dark canvas.

Resources are grouped into their **VPC** zone (a `VPC <id>` cluster, derived from
the module's `vpc` argument), and every external endpoint (CIDR, prefix list,
unresolved reference) into an opposing **External** zone — so the flows between
the inside and the outside read at a glance. Named zones (see
[Directives](#directives-localflowghetti)) can peel specific endpoints out of
External into their own labelled clusters.

The tool prints DOT to stdout; rendering is delegated to Graphviz (`dot`).

## Directives (`local.flowghetti`)

flowghetti reads an optional `locals { flowghetti = { ... } }` block in the root
module. It is plain, inert Terraform — an unused local that never reaches the
real module input — yet flowghetti interprets it to shape the graph:

```hcl
locals {
  flowghetti = {
    # Fold split resources into a single node — e.g. an NLB split across several
    # security groups to dodge the AWS rule limit. Key = absorbed resource,
    # value = the canonical resource it merges into. Chains resolve transitively.
    merge = {
      prodappnlbsitea = "prodappnlb"
      prodappnlbsiteb = "prodappnlb"
    }

    # Route external endpoints into named zones instead of the catch-all
    # External. Key = zone label, value = the origin references (written exactly
    # as in the HCL) of the endpoints to place there.
    groups = {
      "Office" = ["module.cidrs.office"]
      "VPN"    = ["local.vpn_cidr"]
    }
  }
}
```

Both keys are best-effort: a `merge` target that is not a declared resource (or a
cycle) is ignored with a warning, and the resource keeps its own node. An endpoint
whose origin is listed under no zone stays in External; a literal CIDR with no
origin reference can only live in External. See `demo/v0.3.0/` for before/after
renders of each.

## Installation (Nix)

The flake exposes the binary and a Home Manager module.

```bash
# Run without installing
nix run github:gfriloux/flowghetti -- path/to/terraform | dot -Tsvg > flows.svg

# Build the package (dynamically linked)
nix build .#default

# Build a statically-linked (musl) binary for distribution
nix build .#static      # or: just build-static
# -> result/bin/flowghetti, fully static
```

### Home Manager

Import the module and enable it:

```nix
# flake inputs: flowghetti.url = "github:gfriloux/flowghetti";

# in your Home Manager configuration:
{
  imports = [ flowghetti.homeModules.default ];
  programs.flowghetti.enable = true;
  # programs.flowghetti.package = ...;  # override the package if needed
}
```

## How it works

A four-stage, decoupled pipeline:

```text
parse (HCL) -> resolve (refs) -> build (domain graph) -> render (DOT)
```

- **parse** — load every `*.tf`, collect `locals` / `variable` defaults, extract
  the glowwiththeflow module's `ressources` and `flows` as raw HCL expressions.
- **resolve** — turn those expressions into concrete values (best-effort
  cascade: literal → `local.*` → `var.*` → `module.*.output` → symbolic node).
- **build** — assemble the domain graph: apply the `local.flowghetti` directives
  (merge resources, route endpoints into named zones), dedupe external endpoints,
  one edge per flow, group nodes into VPC / named / External zones.
- **render** — emit deterministic Graphviz DOT.

See [`DESIGN.md`](DESIGN.md) for the architecture and invariants, and
[`PROCEDURE_PLANS.md`](PROCEDURE_PLANS.md) for the maintenance procedures.

## Development

```bash
nix develop          # Rust toolchain, just, graphviz
just ci              # fmt + clippy + test + build
just bless           # regenerate golden test files (then review the diff)
```
