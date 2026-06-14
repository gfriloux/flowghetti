module "glow" {
  source = "../../"

  vpc = "vpc-demo"

  ressources = {
    app = { name = "app", type = "ec2" }
  }

  flows = {
    office_to_app = { from_cidr = local.office_cidr, to = "app", port = 443 },
    vpn_to_app    = { from_cidr = local.vpn_cidr, to = "app", port = 22 },
    # a raw literal with no origin reference: stays in the residual External zone.
    app_to_world = { from = "app", to_cidr = ["0.0.0.0/0"], port = 0, protocol = "all" }
  }
}

locals {
  office_cidr = ["198.51.100.0/24"]
  vpn_cidr    = ["203.0.113.0/24"]

  # flowghetti directives: route referenced CIDR endpoints into named zones by
  # their origin reference. Anything not listed (the 0.0.0.0/0 literal) falls
  # back to External.
  flowghetti = {
    groups = {
      "Office" = ["local.office_cidr"]
      "VPN"    = ["local.vpn_cidr"]
    }
  }
}
