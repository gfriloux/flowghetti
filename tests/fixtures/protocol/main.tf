#tfsec:ignore:aws-ec2-no-public-ingress-sgr
module "glow_with_the_flow" {
  source = "../../"

  vpc = "vpc-0cda00644e0506968"

  ressources = {
    dns = { name = "dns", type = "ec2" }
  }

  flows = {
    internet_to_dns_53 = { from_cidr = ["0.0.0.0/0"], to = "dns", port = 53, protocol = "udp" }
  }
}
