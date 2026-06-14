variable "office_cidrs" {
  type    = list(string)
  default = ["0.0.0.0/0"]
}

module "glow" {
  source = "git::ssh://git@example/glowwiththeflow.git?ref=v1.0.0"

  vpc = "vpc-test"

  ressources = {
    app = { name = "app", type = "ec2" }
  }

  flows = {
    office_to_app_https = { from_cidr = var.office_cidrs, to = "app", port = 443 }
  }
}
