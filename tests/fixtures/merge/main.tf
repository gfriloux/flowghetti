module "glow" {
  source = "../../"

  ressources = {
    edgelb      = { name = "edge-lb", type = "nlb" },
    edgelbtwo   = { name = "edge-lb-two", type = "nlb" },
    edgelbthree = { name = "edge-lb-three", type = "nlb" },
    backend     = { name = "backend", type = "ec2" }
  }

  flows = {
    # two split parts reach the same target on the same port: after the merge
    # these collapse to a single edge (parallel dedup).
    edgelb_to_backend    = { from = "edgelb", to = "backend", port = 443 },
    edgelbtwo_to_backend = { from = "edgelbtwo", to = "backend", port = 443 },
    # a flow between two split parts: after the merge it becomes a self-loop and
    # is dropped.
    edgelbthree_to_edgelb = { from = "edgelbthree", to = "edgelb", port = 8080 },
    # a flow into a split part is remapped to the canonical node.
    backend_to_edgelbtwo = { from = "backend", to = "edgelbtwo", port = 22 }
  }
}

# flowghetti directives — valid, inert Terraform (an unused local). The three
# split NLB keys are folded into the canonical "edgelb".
locals {
  flowghetti = {
    merge = {
      edgelbtwo   = "edgelb"
      edgelbthree = "edgelb"
    }
  }
}
