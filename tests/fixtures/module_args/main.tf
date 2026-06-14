locals {
  base_cidrs = ["203.0.113.0/24", "198.51.100.0/24"]
}

module "net" {
  source  = "git::ssh://git@example/net.git?ref=v1.0.0"
  allowed = local.base_cidrs
}

module "glow" {
  source = "git::ssh://git@example/glowwiththeflow.git?ref=v1.0.0"

  vpc = "vpc-test"

  ressources = {
    app = { name = "app", type = "ec2" }
  }

  flows = {
    allowed_to_app_https = { from_cidr = module.net.allowed_cidrs, to = "app", port = 443 }
  }
}
