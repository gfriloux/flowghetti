module "cidrs" {
  source = "git::ssh://git@example/cidrs.git?ref=v1.0.0"
}

module "glow" {
  source = "git::ssh://git@example/glowwiththeflow.git?ref=v1.0.0"

  vpc = "vpc-test"

  ressources = {
    app = { name = "app", type = "ec2" }
  }

  flows = {
    office_to_app_ssh = { from_cidr = module.cidrs.office, to = "app", port = 22 },
    vpn_to_app_https  = { from_cidr = module.cidrs.offices_and_vpn, to = "app", port = 443 },
    app_to_world      = { from = "app", to_cidr = ["0.0.0.0/0"], port = 0, protocol = "all" }
  }
}
