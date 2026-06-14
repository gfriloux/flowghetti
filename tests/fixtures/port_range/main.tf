#tfsec:ignore:aws-ec2-no-public-ingress-sgr
module "glow_with_the_flow" {
  source = "../../"

  vpc = "vpc-0cda00644e0506968"

  ressources = {
    ftp = { name = "ftp", type = "ec2" }
  }

  flows = {
    internet_to_ftp_21   = { from_cidr = ["0.0.0.0/0"], to = "ftp", port = 21 },
    internet_to_ftp_pasv = { from_cidr = ["0.0.0.0/0"], to = "ftp", port_range = { from = 10000, to = 10100 } }
  }
}
